use crate::cursor::{
    CursorError, CursorIdentityResolver, CursorScopeFingerprint, CursorSealingKey,
    CursorSemanticRowId, PageCursor, bounded_page_position, capture_fingerprint, open_cursor,
    seal_cursor,
};
use crate::effect_stream::TransientEffectSemanticValidator;
use boon_data::{
    Bytes, NumberTextFormat, format_number_ascii_text, format_number_text, number_bit_width,
};
use boon_list_access::{
    AccessError, AccessMetrics, AccessStream, ClosedTag as AccessClosedTag,
    CursorKey as AccessCursorKey, Direction as AccessDirection, IndexPlanId as AccessIndexPlanId,
    IndexResourceLimits as AccessIndexResourceLimits, KeyComponent, KeyKind as AccessKeyKind,
    KeySchema, MutationOutcome, OrderedIndex, OrderedIndexIntegrityPoll, OrderedIndexIntegrityTask,
    RowId as AccessRowId, SourceOrderToken, StructuralKey, StructuralValue,
    WorkLimits as AccessWorkLimits, WorkTracker as AccessWorkTracker,
};
use boon_plan::{
    DataTypePlan, DistributedArgumentId, DistributedCallInstanceId, DistributedCallInstanceRow,
    DistributedCallMode, EffectInvocationId, EffectInvocationPlan, ExportId, FieldId, FiniteReal,
    ImportId, ListId, ListInitializerKind, ListStorageSlot, MachinePlan, OutputListFieldRef,
    OwnerInstanceId, OwnerInstanceRow, PlanBoundedListPage, PlanConstantId, PlanConstantValue,
    PlanContextualIndexedAccess, PlanContextualOperationKind, PlanDerivedExpression,
    PlanDerivedKind, PlanInfixOp, PlanInitialListFieldInitializer, PlanIntrinsic, PlanListAccess,
    PlanListAccessSelection, PlanListIndex, PlanListIndexId, PlanListIndexKey,
    PlanListIndexKeyKind, PlanListIndexKeyMultiplicity, PlanListMap, PlanListMutation,
    PlanListPage, PlanListProjection, PlanLocalId, PlanMaterializedRowFieldCopy, PlanOp, PlanOpId,
    PlanOpKind, PlanOrderDirection, PlanOrderOperationKind, PlanOwner, PlanRowBuiltin,
    PlanRowCallArg, PlanRowExpressionArena, PlanRowExpressionId, PlanRowExpressionNode,
    PlanRowSelectPattern, PlanStaticOwnerId, PlanValueListAuthority, ProducerFunctionInstancePlan,
    RemoteCallSiteId, RemoteCallSitePlan, RootOutputDemand, ScalarInitializerPlan,
    ScalarStorageSlot, ScopeId, SourceId, SourcePayloadField, SourceRoute, SourceRouteToken,
    StateId, ValueRef, verify_plan,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::ops::{Bound, Range};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SESSION_LAUNCH_EPOCH: AtomicU64 = AtomicU64::new(1);

pub const MAX_SESSION_INFO_TEXT_BYTES: usize = 1024;
pub const MAX_SESSION_INFO_ROLE_COUNT: usize = 64;
pub const TRANSIENT_EFFECT_FIRST_RESULT_SEQUENCE: u64 = 0;

pub struct HostValueIssuer {
    issuer: [u8; 32],
}

impl HostValueIssuer {
    pub const fn new(issuer: [u8; 32]) -> Self {
        Self { issuer }
    }

    pub fn mint(&self, handle: [u8; 32], generation: u32) -> Result<HostValueBinding, Error> {
        if generation == 0 {
            return Err(Error::Evaluation(
                "host value binding generation must be positive".to_owned(),
            ));
        }
        Ok(HostValueBinding {
            issuer: self.issuer,
            handle,
            generation,
        })
    }

    pub fn open(&self, binding: &HostValueBinding) -> Option<([u8; 32], u32)> {
        (binding.issuer == self.issuer).then_some((binding.handle, binding.generation))
    }
}

impl fmt::Debug for HostValueIssuer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostValueIssuer(<opaque>)")
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HostValueBinding {
    issuer: [u8; 32],
    handle: [u8; 32],
    generation: u32,
}

impl HostValueBinding {
    #[doc(hidden)]
    pub const fn new_issuer(issuer: [u8; 32]) -> HostValueIssuer {
        HostValueIssuer::new(issuer)
    }
}

impl fmt::Debug for HostValueBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostValueBinding(<opaque>)")
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Value {
    Null,
    Bool(bool),
    Number(FiniteReal),
    Text(String),
    Bytes(Bytes),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    MappedRow {
        id: RowId,
        fields: BTreeMap<String, Value>,
    },
    Row {
        id: RowId,
        fields: BTreeMap<FieldId, Value>,
    },
    Error {
        code: String,
    },
    #[serde(skip)]
    HostBound {
        visible: Box<Value>,
        binding: HostValueBinding,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpressionLocalBinding {
    pub owner: PlanStaticOwnerId,
    pub local: PlanLocalId,
    pub value: Value,
}

impl Value {
    pub fn integer(value: i64) -> Result<Self, Error> {
        FiniteReal::from_i64_exact(value)
            .map(Self::Number)
            .map_err(|error| Error::Evaluation(error.to_string()))
    }

    pub fn from_data(value: &boon_data::Value) -> Self {
        runtime_value_from_data(value)
    }

    pub fn to_data(&self) -> Result<boon_data::Value, Error> {
        runtime_value_to_data(self)
    }

    pub fn host_bound(visible: Value, binding: HostValueBinding) -> Self {
        Self::HostBound {
            visible: Box::new(visible),
            binding,
        }
    }

    pub fn visible(&self) -> &Value {
        match self {
            Self::HostBound { visible, .. } => visible.visible(),
            value => value,
        }
    }

    pub fn host_binding(&self) -> Option<&HostValueBinding> {
        match self {
            Self::HostBound { binding, .. } => Some(binding),
            _ => None,
        }
    }

    pub fn contains_host_binding(&self) -> bool {
        match self {
            Self::HostBound { .. } => true,
            Self::List(values) => values.iter().any(Self::contains_host_binding),
            Self::Record(fields) | Self::MappedRow { fields, .. } => {
                fields.values().any(Self::contains_host_binding)
            }
            Self::Row { fields, .. } => fields.values().any(Self::contains_host_binding),
            Self::Null
            | Self::Bool(_)
            | Self::Number(_)
            | Self::Text(_)
            | Self::Bytes(_)
            | Self::Error { .. } => false,
        }
    }

    fn into_visible_facade(self) -> Self {
        match self {
            Self::HostBound { visible, .. } => visible.into_visible_facade(),
            Self::List(values) => {
                Self::List(values.into_iter().map(Self::into_visible_facade).collect())
            }
            Self::Record(fields) => Self::Record(
                fields
                    .into_iter()
                    .map(|(name, value)| (name, value.into_visible_facade()))
                    .collect(),
            ),
            Self::MappedRow { id, fields } => Self::MappedRow {
                id,
                fields: fields
                    .into_iter()
                    .map(|(name, value)| (name, value.into_visible_facade()))
                    .collect(),
            },
            Self::Row { id, fields } => Self::Row {
                id,
                fields: fields
                    .into_iter()
                    .map(|(field, value)| (field, value.into_visible_facade()))
                    .collect(),
            },
            value @ (Self::Null
            | Self::Bool(_)
            | Self::Number(_)
            | Self::Text(_)
            | Self::Bytes(_)
            | Self::Error { .. }) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RowId {
    pub list: ListId,
    pub key: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourcePayload {
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEvent {
    pub sequence: u64,
    pub route: SourceRouteToken,
    pub source: SourceId,
    pub target: Option<RowId>,
    pub payload: SourcePayload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TriggerCause {
    Source(SourceId),
    State(StateId),
    Effect(EffectInvocationId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveTrigger {
    cause: TriggerCause,
    owner_plan: PlanOwner,
    owner: OwnerInstanceId,
    target: Option<RowId>,
    sequence: u64,
}

#[derive(Clone, Debug)]
struct TriggerFrame<'a> {
    active: ActiveTrigger,
    source_event: Option<&'a SourceEvent>,
}

impl<'a> TriggerFrame<'a> {
    fn source(event: &'a SourceEvent, owner_plan: PlanOwner) -> Self {
        Self {
            active: ActiveTrigger {
                cause: TriggerCause::Source(event.source),
                owner_plan,
                owner: event.route.owner.clone(),
                target: event.target,
                sequence: event.sequence,
            },
            source_event: Some(event),
        }
    }

    fn for_target(&self, target: Option<RowId>) -> Self {
        Self {
            active: ActiveTrigger {
                target,
                ..self.active.clone()
            },
            source_event: self.source_event,
        }
    }

    fn effect(
        invocation: EffectInvocationId,
        owner_plan: PlanOwner,
        owner: OwnerInstanceId,
        target: Option<RowId>,
        sequence: u64,
    ) -> Self {
        Self {
            active: ActiveTrigger {
                cause: TriggerCause::Effect(invocation),
                owner_plan,
                owner,
                target,
                sequence,
            },
            source_event: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ValueTarget {
    State(StateId),
    Field(FieldId),
    RowField { row: RowId, field: FieldId },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowSnapshot {
    pub id: RowId,
    pub fields: BTreeMap<FieldId, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Delta {
    SetValue {
        target: ValueTarget,
        value: Value,
    },
    SetDistributedImport {
        import_id: ImportId,
        value: Value,
    },
    InsertRow {
        row: RowSnapshot,
    },
    RemoveRow {
        row: RowId,
    },
    BindSource {
        row: RowId,
        source: SourceId,
        binding_id: u64,
    },
    UnbindSource {
        row: RowId,
        source: SourceId,
        binding_id: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthorityDelta {
    SetRoot {
        state: StateId,
        value: Value,
    },
    SetRowField {
        row: RowId,
        owner_ancestors: Vec<OwnerInstanceRow>,
        materialization_origin: Option<Vec<OwnerInstanceRow>>,
        field: FieldId,
        value: Value,
    },
    ReplaceList {
        list_id: ListId,
        authority: ListAuthority,
    },
    InsertRow {
        row: RowAuthority,
        index: u64,
        next_key: u64,
    },
    RemoveRow {
        row: RowId,
        next_key: u64,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TurnMetrics {
    pub dirty_state_count: usize,
    pub dirty_field_count: usize,
    pub recomputed_field_count: usize,
    pub recomputed_list_count: usize,
    pub changed_row_count: usize,
    pub dependency_fanout_count: usize,
    pub indexed_access_count: usize,
    pub index_candidate_count: usize,
    pub list_find_scan_count: usize,
    pub access_index_seek_count: u64,
    pub access_cursor_seek_count: u64,
    pub access_key_range_count: u64,
    pub access_key_count: u64,
    pub access_candidate_count: u64,
    pub access_kernel_returned_count: u64,
    pub access_result_count: u64,
    pub access_branch_poll_count: u64,
    pub access_union_duplicate_skip_count: u64,
    pub access_intersection_candidate_skip_count: u64,
    pub access_full_scan_count: u64,
    pub access_work_limit_failure_count: u64,
    pub bounded_page_scan_count: u64,
    pub bounded_page_candidate_count: u64,
    pub ordered_index_full_rebuild_count: u64,
    pub ordered_index_incremental_row_count: u64,
    pub ordered_index_key_evaluation_count: u64,
    pub ordered_index_insert_count: u64,
    pub ordered_index_update_count: u64,
    pub ordered_index_remove_count: u64,
    pub ordered_index_rebuild_logical_row_count: u64,
    pub ordered_index_rebuild_entry_count: u64,
    pub ordered_index_rebuild_expanded_key_count: u64,
    pub ordered_index_rebuild_encoded_key_bytes: u64,
    pub ordered_index_rebuild_structural_key_bytes: u64,
    pub ordered_index_rebuild_payload_bytes: u64,
    pub ordered_index_current_count: u64,
    pub ordered_index_current_logical_row_count: u64,
    pub ordered_index_current_entry_count: u64,
    pub ordered_index_current_expanded_key_count: u64,
    pub ordered_index_current_encoded_key_bytes: u64,
    pub ordered_index_current_structural_key_bytes: u64,
    pub ordered_index_current_payload_bytes: u64,
    pub ordered_index_affected_fanout_max: u64,
    pub ordered_index_resource_limit_failure_count: u64,
    pub source_order_location_update_count: u64,
    pub source_order_location_update_max: u64,
    pub source_order_block_split_count: u64,
    pub source_order_block_merge_count: u64,
    pub source_order_tree_visit_count: u64,
    pub source_order_tree_visit_max: u64,
    pub source_order_relabel_operation_count: u64,
    pub source_order_relabel_row_count: u64,
    pub source_order_relabel_window_max: u64,
    pub work_unit_count: u64,
    pub recomputed_targets: Vec<ValueTarget>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Turn {
    pub sequence: u64,
    pub source_sequence: Option<u64>,
    pub deltas: Vec<Delta>,
    pub authority_deltas: Vec<AuthorityDelta>,
    pub durable_changes: Vec<boon_persistence::DurableChange>,
    pub outbox_changes: Vec<boon_persistence::DurableOutboxChange>,
    pub transient_effects: Vec<TransientEffectInvocation>,
    pub cancelled_transient_effects: Vec<TransientEffectCallId>,
    pub transient_effect_credit_grants: Vec<TransientEffectCreditGrant>,
    pub distributed_invocations: Vec<DistributedInvocation>,
    pub metrics: TurnMetrics,
}

impl fmt::Debug for Turn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Turn")
            .field("sequence", &self.sequence)
            .field("delta_count", &self.deltas.len())
            .field("authority_delta_count", &self.authority_deltas.len())
            .field("durable_change_count", &self.durable_changes.len())
            .field("outbox_change_count", &self.outbox_changes.len())
            .field("transient_effect_count", &self.transient_effects.len())
            .field(
                "cancelled_transient_effect_count",
                &self.cancelled_transient_effects.len(),
            )
            .field(
                "transient_effect_credit_grant_count",
                &self.transient_effect_credit_grants.len(),
            )
            .field(
                "distributed_invocation_count",
                &self.distributed_invocations.len(),
            )
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DistributedInvocation {
    pub call_site_id: RemoteCallSiteId,
    pub call_instance_id: DistributedCallInstanceId,
    pub arguments: BTreeMap<DistributedArgumentId, Value>,
    pub result_route: SourceRouteToken,
}

impl fmt::Debug for DistributedInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DistributedInvocation(..)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DistributedCurrentCallInstance {
    pub call_instance_id: DistributedCallInstanceId,
    pub arguments: BTreeMap<DistributedArgumentId, Value>,
}

impl fmt::Debug for DistributedCurrentCallInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DistributedCurrentCallInstance(..)")
    }
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TransientEffectCallId {
    launch_epoch: u64,
    sequence: u64,
}

impl TransientEffectCallId {
    /// Reconstructs an opaque ID at a trusted runtime/host protocol boundary.
    ///
    /// Application data must never carry these parts. The type deliberately
    /// remains non-serializable and redacts both fields from diagnostics.
    pub const fn from_host_parts(launch_epoch: u64, sequence: u64) -> Self {
        Self {
            launch_epoch,
            sequence,
        }
    }

    pub fn launch_epoch(self) -> u64 {
        self.launch_epoch
    }

    pub fn sequence(self) -> u64 {
        self.sequence
    }
}

impl fmt::Debug for TransientEffectCallId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TransientEffectCallId(..)")
    }
}

impl fmt::Display for TransientEffectCallId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransientEffectInvocation {
    pub call_id: TransientEffectCallId,
    pub invocation_id: EffectInvocationId,
    pub effect_id: boon_plan::EffectId,
    pub trigger_sequence: u64,
    pub authority_turn_sequence: u64,
    pub owner: OwnerInstanceId,
    pub target: Option<RowId>,
    pub intent: Value,
    pub delivery: boon_plan::EffectDeliveryCardinality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransientEffectCreditGrant {
    pub call_id: TransientEffectCallId,
    pub credits: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub states: BTreeMap<StateId, Value>,
    pub fields: BTreeMap<FieldId, Value>,
    pub lists: BTreeMap<ListId, Vec<RowSnapshot>>,
}

/// Authoritative scalar state as distinct from a derived inspection value.
///
/// `touched` is persisted even when `value` equals the current program default.
/// This prevents a later default change from overwriting an explicit user choice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScalarAuthority {
    pub touched: bool,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RowAuthority {
    pub id: RowId,
    pub source_order_token: u128,
    pub owner_ancestors: Vec<OwnerInstanceRow>,
    pub materialization_origin: Option<Vec<OwnerInstanceRow>>,
    pub fields: BTreeMap<FieldId, Value>,
    pub touched_fields: BTreeSet<FieldId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListAuthority {
    pub touched: bool,
    pub revision: u64,
    pub next_key: u64,
    pub next_order_token: u128,
    pub rows: Vec<RowAuthority>,
}

/// Runtime-ID authority image used at the machine-instance boundary.
///
/// Durable storage translates the runtime IDs through `MachinePlan::persistence`;
/// derived values, indexes, source bindings, and currentness caches never enter
/// this image.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthoritySnapshot {
    pub through_turn_sequence: u64,
    pub states: BTreeMap<StateId, ScalarAuthority>,
    pub lists: BTreeMap<ListId, ListAuthority>,
}

impl Snapshot {
    pub fn value(&self, target: ValueTarget) -> Option<&Value> {
        match target {
            ValueTarget::State(state) => self.states.get(&state),
            ValueTarget::Field(field) => self.fields.get(&field),
            ValueTarget::RowField { row, field } => self
                .lists
                .get(&row.list)?
                .iter()
                .find(|candidate| candidate.id == row)?
                .fields
                .get(&field),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionConnectionStatus {
    Connecting,
    #[default]
    Current,
    Stale,
    Failed {
        code: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionPrincipal {
    #[default]
    Anonymous,
    Authenticated {
        subject: String,
        roles: Vec<String>,
    },
}

impl SessionPrincipal {
    pub fn authenticated(
        subject: impl Into<String>,
        roles: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, Error> {
        let mut roles = roles.into_iter().map(Into::into).collect::<Vec<_>>();
        roles.sort();
        roles.dedup();
        let principal = Self::Authenticated {
            subject: subject.into(),
            roles,
        };
        validate_session_principal(&principal)?;
        Ok(principal)
    }

    pub fn validate(&self) -> Result<(), Error> {
        validate_session_principal(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionContext {
    Unavailable,
    Available {
        status: SessionConnectionStatus,
        principal: SessionPrincipal,
    },
}

impl Default for SessionContext {
    fn default() -> Self {
        Self::Available {
            status: SessionConnectionStatus::Current,
            principal: SessionPrincipal::Anonymous,
        }
    }
}

impl SessionContext {
    pub fn validate(&self) -> Result<(), Error> {
        validate_session_context(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributedImportUpdate {
    pub import_id: ImportId,
    pub content_revision: u64,
    pub value: Value,
}

impl DistributedImportUpdate {
    pub fn new(import_id: ImportId, content_revision: u64, value: Value) -> Self {
        Self {
            import_id,
            content_revision,
            value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionOptions {
    pub require_monotonic_sequences: bool,
    pub program_revision: u64,
    pub session_context: SessionContext,
    /// Deterministic executor work allowed for one startup, source turn, or
    /// host-owned currentness transaction. Trusted applications leave this
    /// unbounded; capability hosts set it for restricted programs.
    pub max_work_units_per_transaction: Option<u64>,
    /// Deterministic limits for typed list seeks, candidates, and results.
    pub list_access_work_limits: AccessWorkLimits,
    /// Host-private key for opaque application pagination cursors. When absent,
    /// the machine generates an ephemeral key and cursors expire on restart.
    pub cursor_sealing_key: Option<CursorSealingKey>,
    /// Host-private Session/tenant/authorization identity. Scoped hosts set a
    /// stable fingerprint for the lifetime in which their cursors are valid.
    pub cursor_scope_fingerprint: Option<CursorScopeFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecoveryDistributedImport {
    pub revision: Option<u64>,
    pub value: boon_data::Value,
}

/// Host-private execution state needed to resume one exact compiled machine.
///
/// This image contains semantic authority and distributed imports, but never
/// process-owned effect correlations. Restoring it always allocates fresh
/// transient effect identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MachineRecoveryImage {
    pub authority: boon_persistence::RestoreImage,
    pub last_source_sequence: Option<u64>,
    pub session_context: SessionContext,
    pub distributed_imports: BTreeMap<ImportId, RecoveryDistributedImport>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            require_monotonic_sequences: true,
            program_revision: 1,
            session_context: SessionContext::default(),
            max_work_units_per_transaction: None,
            list_access_work_limits: AccessWorkLimits::default(),
            cursor_sealing_key: None,
            cursor_scope_fingerprint: None,
        }
    }
}

fn validate_session_options(options: &SessionOptions) -> Result<(), Error> {
    if options.program_revision == 0 {
        return Err(Error::InvalidOptions(
            "program revision must be positive".to_owned(),
        ));
    }
    validate_session_context(&options.session_context)
}

fn validate_session_context(context: &SessionContext) -> Result<(), Error> {
    let SessionContext::Available { status, principal } = context else {
        return Ok(());
    };
    if let SessionConnectionStatus::Failed { code } = status
        && (code.is_empty()
            || code.len() > MAX_SESSION_INFO_TEXT_BYTES
            || !code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    {
        return Err(Error::InvalidOptions(format!(
            "SessionInfo failure code must be 1..={MAX_SESSION_INFO_TEXT_BYTES} ASCII identifier bytes"
        )));
    }
    validate_session_principal(principal)
}

fn validate_session_principal(principal: &SessionPrincipal) -> Result<(), Error> {
    let SessionPrincipal::Authenticated { subject, roles } = principal else {
        return Ok(());
    };
    if subject.is_empty() || subject.len() > MAX_SESSION_INFO_TEXT_BYTES {
        return Err(Error::InvalidOptions(format!(
            "SessionInfo principal subject must be 1..={MAX_SESSION_INFO_TEXT_BYTES} UTF-8 bytes"
        )));
    }
    if roles.len() > MAX_SESSION_INFO_ROLE_COUNT {
        return Err(Error::InvalidOptions(format!(
            "SessionInfo principal has {} roles; limit is {MAX_SESSION_INFO_ROLE_COUNT}",
            roles.len()
        )));
    }
    if roles.windows(2).any(|pair| pair[0] >= pair[1])
        || roles
            .iter()
            .any(|role| role.is_empty() || role.len() > MAX_SESSION_INFO_TEXT_BYTES)
    {
        return Err(Error::InvalidOptions(
            "SessionInfo principal roles must be sorted, unique, and bounded non-empty text"
                .to_owned(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidOptions(String),
    InvalidPlan(String),
    InvalidEvent(String),
    Unsupported { op: PlanOpId, detail: String },
    Cycle { field: FieldId, row: Option<RowId> },
    ListCycle { list: ListId },
    WorkBudgetExceeded { limit: u64, attempted: u64 },
    Evaluation(String),
    NotDemanded(FieldId),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(detail) => write!(formatter, "invalid SessionOptions: {detail}"),
            Self::InvalidPlan(detail) => write!(formatter, "invalid MachinePlan: {detail}"),
            Self::InvalidEvent(detail) => write!(formatter, "invalid source event: {detail}"),
            Self::Unsupported { op, detail } => {
                write!(formatter, "unsupported plan op {}: {detail}", op.0)
            }
            Self::Cycle { field, row } => match row {
                Some(row) => write!(
                    formatter,
                    "derived cycle at field {} in row {}:{}:{}",
                    field.0, row.list.0, row.key, row.generation
                ),
                None => write!(formatter, "derived cycle at root field {}", field.0),
            },
            Self::ListCycle { list } => write!(formatter, "derived cycle at list {}", list.0),
            Self::WorkBudgetExceeded { limit, attempted } => write!(
                formatter,
                "executor work budget exceeded: attempted {attempted} units with a {limit}-unit transaction limit"
            ),
            Self::Evaluation(detail) => write!(formatter, "evaluation failed: {detail}"),
            Self::NotDemanded(field) => {
                write!(
                    formatter,
                    "root field {} is not in the demand plan",
                    field.0
                )
            }
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Currentness {
    Current,
    Dirty,
    Evaluating,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RowFieldAvailability {
    Stored,
    DefaultExpression,
    RowComputation,
    Missing,
}

#[derive(Clone, Debug)]
struct DerivedCell {
    currentness: Currentness,
    value: Option<Value>,
}

#[derive(Clone, Debug)]
struct DerivedListCell {
    currentness: Currentness,
    items: Option<Vec<EvalValue>>,
    window: Option<DerivedListWindow>,
}

#[derive(Clone, Debug)]
struct DerivedListWindow {
    logical_len: u64,
    values_current: bool,
    rows_by_index: BTreeMap<u64, RowId>,
}

impl Default for DerivedListCell {
    fn default() -> Self {
        Self {
            currentness: Currentness::Dirty,
            items: None,
            window: None,
        }
    }
}

impl Default for DerivedCell {
    fn default() -> Self {
        Self {
            currentness: Currentness::Dirty,
            value: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Row {
    owner_ancestors: Vec<OwnerInstanceRow>,
    materialization_origin: Option<Vec<OwnerInstanceRow>>,
    fields: BTreeMap<FieldId, Value>,
    derived: BTreeMap<FieldId, Currentness>,
    default_fields: BTreeSet<FieldId>,
    bindings: BTreeMap<SourceId, u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ListOrderMaintenance {
    row_location_update_count: usize,
    block_split_count: usize,
    block_merge_count: usize,
    tree_visit_count: usize,
    tree_visit_max: usize,
}

impl ListOrderMaintenance {
    fn record_tree_visits(&mut self, visits: usize) {
        self.tree_visit_count = self.tree_visit_count.saturating_add(visits);
        self.tree_visit_max = self.tree_visit_max.max(visits);
    }

    fn merge(&mut self, other: Self) {
        self.row_location_update_count = self
            .row_location_update_count
            .saturating_add(other.row_location_update_count);
        self.block_split_count = self
            .block_split_count
            .saturating_add(other.block_split_count);
        self.block_merge_count = self
            .block_merge_count
            .saturating_add(other.block_merge_count);
        self.tree_visit_count = self.tree_visit_count.saturating_add(other.tree_visit_count);
        self.tree_visit_max = self.tree_visit_max.max(other.tree_visit_max);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ListOrderLocation {
    block: u64,
    offset: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ListOrderBlock {
    previous: Option<u64>,
    next: Option<u64>,
    left: Option<u64>,
    right: Option<u64>,
    parent: Option<u64>,
    height: u16,
    subtree_rows: usize,
    rows: Vec<RowId>,
}

/// A linked sequence of bounded row chunks indexed by a deterministic AVL tree.
///
/// Mutations are prepared in a copy-on-write overlay. Only one bounded chunk
/// and the AVL path to it are copied, validated, and fault-checked before an
/// infallible patch commit changes canonical authority.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ListOrder {
    blocks: BTreeMap<u64, ListOrderBlock>,
    locations: BTreeMap<RowId, ListOrderLocation>,
    root: Option<u64>,
    first_block: Option<u64>,
    last_block: Option<u64>,
    next_block: u64,
    len: usize,
    version: u64,
}

impl ListOrder {
    #[cfg(test)]
    const TARGET_BLOCK_ROWS: usize = 128;
    const MAX_BLOCK_ROWS: usize = 256;
    const MIN_BLOCK_ROWS: usize = 64;
    const MAX_TREE_HEIGHT: u16 = 96;

    #[cfg(test)]
    fn from_rows(rows: Vec<RowId>) -> Result<Self, Error> {
        let mut order = Self::default();
        let mut seen = BTreeSet::new();
        let chunks = rows
            .chunks(Self::TARGET_BLOCK_ROWS)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();
        let block_count = u64::try_from(chunks.len())
            .map_err(|_| Error::Evaluation("list order has too many blocks".to_owned()))?;
        order.next_block = if block_count == 0 {
            0
        } else {
            block_count.checked_add(1).ok_or_else(|| {
                Error::Evaluation("list order exhausted its block identity space".to_owned())
            })?
        };
        order.first_block = (block_count > 0).then_some(1);
        order.last_block = (block_count > 0).then_some(block_count);
        for (chunk_index, chunk) in chunks.into_iter().enumerate() {
            let block = u64::try_from(chunk_index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| {
                    Error::Evaluation("list order block identity overflow".to_owned())
                })?;
            for (offset, row) in chunk.iter().copied().enumerate() {
                if !seen.insert(row) {
                    return Err(Error::InvalidPlan(format!(
                        "list order repeats row {}:{}:{}",
                        row.list.0, row.key, row.generation
                    )));
                }
                order
                    .locations
                    .insert(row, ListOrderLocation { block, offset });
            }
            order.len = order
                .len
                .checked_add(chunk.len())
                .ok_or_else(|| Error::Evaluation("list order row count overflow".to_owned()))?;
            order.blocks.insert(
                block,
                ListOrderBlock {
                    previous: block.checked_sub(1).filter(|value| *value > 0),
                    next: (block < block_count).then_some(block + 1),
                    rows: chunk,
                    ..ListOrderBlock::default()
                },
            );
        }
        let block_ids = (1..=block_count).collect::<Vec<_>>();
        order.root = Self::build_balanced_tree(&block_ids, None, &mut order.blocks)?;
        order.validate()?;
        Ok(order)
    }

    #[cfg(test)]
    fn build_balanced_tree(
        block_ids: &[u64],
        parent: Option<u64>,
        blocks: &mut BTreeMap<u64, ListOrderBlock>,
    ) -> Result<Option<u64>, Error> {
        if block_ids.is_empty() {
            return Ok(None);
        }
        let middle = block_ids.len() / 2;
        let block = block_ids[middle];
        let left = Self::build_balanced_tree(&block_ids[..middle], Some(block), blocks)?;
        let right = Self::build_balanced_tree(&block_ids[middle + 1..], Some(block), blocks)?;
        let left_state = left.and_then(|id| blocks.get(&id));
        let right_state = right.and_then(|id| blocks.get(&id));
        let height = 1_u16
            .checked_add(
                left_state
                    .map(|state| state.height)
                    .unwrap_or(0)
                    .max(right_state.map(|state| state.height).unwrap_or(0)),
            )
            .ok_or_else(|| Error::Evaluation("list order tree height overflow".to_owned()))?;
        if height > Self::MAX_TREE_HEIGHT {
            return Err(Error::Evaluation(
                "list order exceeds its deterministic tree-height bound".to_owned(),
            ));
        }
        let subtree_rows = blocks
            .get(&block)
            .map(|state| state.rows.len())
            .unwrap_or_default()
            .checked_add(left_state.map(|state| state.subtree_rows).unwrap_or(0))
            .and_then(|rows| {
                rows.checked_add(right_state.map(|state| state.subtree_rows).unwrap_or(0))
            })
            .ok_or_else(|| Error::Evaluation("list order tree row count overflow".to_owned()))?;
        let state = blocks
            .get_mut(&block)
            .ok_or_else(|| Error::InvalidPlan("list order build block is missing".to_owned()))?;
        state.parent = parent;
        state.left = left;
        state.right = right;
        state.height = height;
        state.subtree_rows = subtree_rows;
        Ok(Some(block))
    }

    fn len(&self) -> usize {
        self.len
    }

    fn iter(&self) -> ListOrderIter<'_> {
        ListOrderIter {
            order: self,
            block: self.first_block,
            offset: 0,
        }
    }

    fn get(&self, index: usize) -> Option<&RowId> {
        let (block, offset) = self.locate_index(index)?;
        self.blocks.get(&block)?.rows.get(offset)
    }

    fn range(&self, range: Range<usize>) -> Vec<RowId> {
        if range.start >= range.end || range.start >= self.len {
            return Vec::new();
        }
        let end = range.end.min(self.len);
        let Some((block, offset)) = self.locate_index(range.start) else {
            return Vec::new();
        };
        ListOrderIter {
            order: self,
            block: Some(block),
            offset,
        }
        .take(end - range.start)
        .copied()
        .collect()
    }

    fn to_vec(&self) -> Vec<RowId> {
        self.iter().copied().collect()
    }

    fn position(&self, row: RowId) -> Option<usize> {
        self.position_with_visits(row).map(|(position, _)| position)
    }

    fn position_with_visits(&self, row: RowId) -> Option<(usize, usize)> {
        let location = self.locations.get(&row)?;
        let state = self.blocks.get(&location.block)?;
        if state.rows.get(location.offset) != Some(&row) {
            return None;
        }
        let mut position = self
            .subtree_rows(state.left)
            .saturating_add(location.offset);
        let mut current = location.block;
        let mut visits = 1_usize;
        while let Some(parent) = self.blocks.get(&current)?.parent {
            visits = visits.saturating_add(1);
            let parent_state = self.blocks.get(&parent)?;
            if parent_state.right == Some(current) {
                position = position
                    .saturating_add(self.subtree_rows(parent_state.left))
                    .saturating_add(parent_state.rows.len());
            } else if parent_state.left != Some(current) {
                return None;
            }
            current = parent;
        }
        (self.root == Some(current)).then_some((position, visits))
    }

    fn positions(&self, rows: &[RowId]) -> Option<Vec<(RowId, usize)>> {
        self.positions_with_visits(rows)
            .map(|(positions, _, _)| positions)
    }

    fn positions_with_visits(&self, rows: &[RowId]) -> Option<(Vec<(RowId, usize)>, usize, usize)> {
        let mut visit_count = 0_usize;
        let mut visit_max = 0_usize;
        let positions = rows
            .iter()
            .map(|row| {
                let (position, visits) = self.position_with_visits(*row)?;
                visit_count = visit_count.saturating_add(visits);
                visit_max = visit_max.max(visits);
                Some((*row, position))
            })
            .collect::<Option<Vec<_>>>()?;
        Some((positions, visit_count, visit_max))
    }

    fn minimum_position(&self, rows: &[RowId]) -> Option<usize> {
        self.positions(rows)?
            .into_iter()
            .map(|(_, position)| position)
            .min()
    }

    #[cfg(test)]
    fn push(&mut self, row: RowId) -> Result<ListOrderMaintenance, Error> {
        let prepared = self.prepare_push(row, None)?;
        let maintenance = prepared.maintenance.clone();
        prepared.commit(self);
        Ok(maintenance)
    }

    #[cfg(test)]
    fn insert(&mut self, index: usize, row: RowId) -> Result<ListOrderMaintenance, Error> {
        let prepared = self.prepare_insert(index, row, None)?;
        let maintenance = prepared.maintenance.clone();
        prepared.commit(self);
        Ok(maintenance)
    }

    #[cfg(test)]
    fn remove(&mut self, row: RowId) -> Result<Option<(usize, ListOrderMaintenance)>, Error> {
        let Some(position) = self.position(row) else {
            return Ok(None);
        };
        let Some(prepared) = self.prepare_remove(row, None)? else {
            return Ok(None);
        };
        let maintenance = prepared.maintenance.clone();
        prepared.commit(self);
        Ok(Some((position, maintenance)))
    }

    fn locate_index(&self, index: usize) -> Option<(u64, usize)> {
        self.locate_index_with_visits(index)
            .map(|(block, offset, _)| (block, offset))
    }

    fn locate_index_with_visits(&self, mut index: usize) -> Option<(u64, usize, usize)> {
        if index >= self.len {
            return None;
        }
        let mut block = self.root;
        let mut visits = 0_usize;
        while let Some(block_id) = block {
            visits = visits.saturating_add(1);
            let state = self.blocks.get(&block_id)?;
            let left_rows = self.subtree_rows(state.left);
            if index < left_rows {
                block = state.left;
            } else if index < left_rows.saturating_add(state.rows.len()) {
                return Some((block_id, index - left_rows, visits));
            } else {
                index = index
                    .saturating_sub(left_rows)
                    .saturating_sub(state.rows.len());
                block = state.right;
            }
        }
        None
    }

    fn subtree_rows(&self, block: Option<u64>) -> usize {
        block
            .and_then(|block| self.blocks.get(&block))
            .map(|block| block.subtree_rows)
            .unwrap_or(0)
    }

    fn prepare_push(
        &self,
        row: RowId,
        fault: Option<ListOrderFaultPoint>,
    ) -> Result<PreparedListOrderMutation, Error> {
        let mut draft = ListOrderDraft::new(self, fault);
        draft.push(row)?;
        draft.finish()
    }

    #[cfg(test)]
    fn prepare_insert(
        &self,
        index: usize,
        row: RowId,
        fault: Option<ListOrderFaultPoint>,
    ) -> Result<PreparedListOrderMutation, Error> {
        let mut draft = ListOrderDraft::new(self, fault);
        draft.insert(index, row)?;
        draft.finish()
    }

    fn prepare_remove(
        &self,
        row: RowId,
        fault: Option<ListOrderFaultPoint>,
    ) -> Result<Option<PreparedListOrderMutation>, Error> {
        let mut draft = ListOrderDraft::new(self, fault);
        let Some(_) = draft.remove(row)? else {
            return Ok(None);
        };
        draft.finish().map(Some)
    }

    #[cfg(test)]
    fn validate(&self) -> Result<(), Error> {
        if self.len != self.locations.len() {
            return Err(Error::InvalidPlan(
                "list order row count and location count differ".to_owned(),
            ));
        }
        let mut rows = 0_usize;
        let mut block = self.first_block;
        let mut previous = None;
        let mut visited = BTreeSet::new();
        while let Some(block_id) = block {
            if !visited.insert(block_id) {
                return Err(Error::InvalidPlan(
                    "list order block chain cycles".to_owned(),
                ));
            }
            let state = self.blocks.get(&block_id).ok_or_else(|| {
                Error::InvalidPlan("list order block chain references a missing block".to_owned())
            })?;
            if state.previous != previous || state.rows.len() > Self::MAX_BLOCK_ROWS {
                return Err(Error::InvalidPlan(
                    "list order block chain is inconsistent".to_owned(),
                ));
            }
            for (offset, row) in state.rows.iter().copied().enumerate() {
                if self.locations.get(&row)
                    != Some(&ListOrderLocation {
                        block: block_id,
                        offset,
                    })
                {
                    return Err(Error::InvalidPlan(
                        "list order row location is inconsistent".to_owned(),
                    ));
                }
            }
            rows = rows.checked_add(state.rows.len()).ok_or_else(|| {
                Error::Evaluation("list order validation row count overflow".to_owned())
            })?;
            previous = Some(block_id);
            block = state.next;
        }
        if previous != self.last_block || rows != self.len || visited.len() != self.blocks.len() {
            return Err(Error::InvalidPlan(
                "list order chain does not cover canonical blocks".to_owned(),
            ));
        }
        let (tree_rows, tree_height, tree_blocks) = self.validate_tree(self.root, None)?;
        if tree_rows != self.len || tree_blocks != self.blocks.len() {
            return Err(Error::InvalidPlan(
                "list order tree does not cover canonical rows".to_owned(),
            ));
        }
        if tree_height > Self::MAX_TREE_HEIGHT {
            return Err(Error::InvalidPlan(
                "list order tree exceeds its hard height bound".to_owned(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    fn validate_tree(
        &self,
        block: Option<u64>,
        parent: Option<u64>,
    ) -> Result<(usize, u16, usize), Error> {
        let Some(block) = block else {
            return Ok((0, 0, 0));
        };
        let state = self.blocks.get(&block).ok_or_else(|| {
            Error::InvalidPlan("list order tree references a missing block".to_owned())
        })?;
        if state.parent != parent {
            return Err(Error::InvalidPlan(
                "list order tree parent is inconsistent".to_owned(),
            ));
        }
        let (left_rows, left_height, left_blocks) = self.validate_tree(state.left, Some(block))?;
        let (right_rows, right_height, right_blocks) =
            self.validate_tree(state.right, Some(block))?;
        if left_height.abs_diff(right_height) > 1 {
            return Err(Error::InvalidPlan(
                "list order AVL balance invariant is violated".to_owned(),
            ));
        }
        let height = 1_u16
            .checked_add(left_height.max(right_height))
            .ok_or_else(|| Error::Evaluation("list order tree height overflow".to_owned()))?;
        let rows = left_rows
            .checked_add(state.rows.len())
            .and_then(|rows| rows.checked_add(right_rows))
            .ok_or_else(|| Error::Evaluation("list order tree row count overflow".to_owned()))?;
        if state.height != height || state.subtree_rows != rows {
            return Err(Error::InvalidPlan(
                "list order AVL metadata is inconsistent".to_owned(),
            ));
        }
        let blocks = left_blocks
            .checked_add(right_blocks)
            .and_then(|blocks| blocks.checked_add(1))
            .ok_or_else(|| Error::Evaluation("list order tree block count overflow".to_owned()))?;
        Ok((rows, height, blocks))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ListOrderFaultPoint {
    BlockAllocation,
    BlockSplit,
    TreeRebalance,
    Finalize,
}

#[derive(Clone, Debug)]
struct ListOrderPatchValue<T> {
    before: Option<T>,
    after: Option<T>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ListOrderMetadata {
    root: Option<u64>,
    first_block: Option<u64>,
    last_block: Option<u64>,
    next_block: u64,
    len: usize,
    version: u64,
}

impl ListOrderMetadata {
    fn read(order: &ListOrder) -> Self {
        Self {
            root: order.root,
            first_block: order.first_block,
            last_block: order.last_block,
            next_block: order.next_block,
            len: order.len,
            version: order.version,
        }
    }

    fn write(self, order: &mut ListOrder) {
        order.root = self.root;
        order.first_block = self.first_block;
        order.last_block = self.last_block;
        order.next_block = self.next_block;
        order.len = self.len;
        order.version = self.version;
    }
}

#[derive(Clone, Debug)]
struct ListOrderPatch {
    before: ListOrderMetadata,
    after: ListOrderMetadata,
    blocks: BTreeMap<u64, ListOrderPatchValue<ListOrderBlock>>,
    locations: BTreeMap<RowId, ListOrderPatchValue<ListOrderLocation>>,
}

impl ListOrderPatch {
    fn matches_before(&self, order: &ListOrder) -> bool {
        self.matches(order, self.before, false)
    }

    fn matches_after(&self, order: &ListOrder) -> bool {
        self.matches(order, self.after, true)
    }

    fn matches(&self, order: &ListOrder, metadata: ListOrderMetadata, after: bool) -> bool {
        ListOrderMetadata::read(order) == metadata
            && self.blocks.iter().all(|(id, change)| {
                order.blocks.get(id)
                    == if after {
                        change.after.as_ref()
                    } else {
                        change.before.as_ref()
                    }
            })
            && self.locations.iter().all(|(row, change)| {
                order.locations.get(row)
                    == if after {
                        change.after.as_ref()
                    } else {
                        change.before.as_ref()
                    }
            })
    }

    fn apply_forward(&self, order: &mut ListOrder) {
        assert!(
            self.matches_before(order),
            "prepared list-order patch lost its canonical base"
        );
        Self::apply_entries(order, &self.blocks, &self.locations, true);
        self.after.write(order);
    }

    fn apply_reverse_idempotent(&self, order: &mut ListOrder) -> Result<(), Error> {
        if self.matches_before(order) {
            return Ok(());
        }
        if !self.matches_after(order) {
            return Err(Error::InvalidPlan(
                "list-order rollback no longer matches its prepared authority".to_owned(),
            ));
        }
        Self::apply_entries(order, &self.blocks, &self.locations, false);
        self.before.write(order);
        Ok(())
    }

    fn apply_entries(
        order: &mut ListOrder,
        blocks: &BTreeMap<u64, ListOrderPatchValue<ListOrderBlock>>,
        locations: &BTreeMap<RowId, ListOrderPatchValue<ListOrderLocation>>,
        forward: bool,
    ) {
        for (id, change) in blocks {
            let value = if forward {
                &change.after
            } else {
                &change.before
            };
            match value {
                Some(value) => {
                    order.blocks.insert(*id, value.clone());
                }
                None => {
                    order.blocks.remove(id);
                }
            }
        }
        for (row, change) in locations {
            let value = if forward { change.after } else { change.before };
            match value {
                Some(value) => {
                    order.locations.insert(*row, value);
                }
                None => {
                    order.locations.remove(row);
                }
            }
        }
    }
}

struct PreparedListOrderMutation {
    patch: ListOrderPatch,
    maintenance: ListOrderMaintenance,
}

impl PreparedListOrderMutation {
    fn commit(self, order: &mut ListOrder) -> ListOrderPatch {
        self.patch.apply_forward(order);
        self.patch
    }
}

struct ListOrderDraft<'a> {
    base: &'a ListOrder,
    blocks: BTreeMap<u64, Option<ListOrderBlock>>,
    locations: BTreeMap<RowId, Option<ListOrderLocation>>,
    root: Option<u64>,
    first_block: Option<u64>,
    last_block: Option<u64>,
    next_block: u64,
    len: usize,
    maintenance: ListOrderMaintenance,
    fault: Option<ListOrderFaultPoint>,
}

impl<'a> ListOrderDraft<'a> {
    fn new(base: &'a ListOrder, fault: Option<ListOrderFaultPoint>) -> Self {
        Self {
            base,
            blocks: BTreeMap::new(),
            locations: BTreeMap::new(),
            root: base.root,
            first_block: base.first_block,
            last_block: base.last_block,
            next_block: base.next_block,
            len: base.len,
            maintenance: ListOrderMaintenance::default(),
            fault,
        }
    }

    fn checkpoint(&mut self, point: ListOrderFaultPoint) -> Result<(), Error> {
        if self.fault == Some(point) {
            self.fault = None;
            return Err(Error::Evaluation(format!(
                "injected list-order preparation failure at {point:?}"
            )));
        }
        Ok(())
    }

    fn block(&self, id: u64) -> Option<&ListOrderBlock> {
        match self.blocks.get(&id) {
            Some(value) => value.as_ref(),
            None => self.base.blocks.get(&id),
        }
    }

    fn block_mut(&mut self, id: u64) -> Result<&mut ListOrderBlock, Error> {
        if !self.blocks.contains_key(&id) {
            let value = self.base.blocks.get(&id).cloned().ok_or_else(|| {
                Error::InvalidPlan("list order draft block is missing".to_owned())
            })?;
            self.blocks.insert(id, Some(value));
        }
        self.blocks
            .get_mut(&id)
            .and_then(Option::as_mut)
            .ok_or_else(|| Error::InvalidPlan("list order draft block was removed".to_owned()))
    }

    fn location(&self, row: RowId) -> Option<ListOrderLocation> {
        match self.locations.get(&row) {
            Some(value) => *value,
            None => self.base.locations.get(&row).copied(),
        }
    }

    fn set_location(&mut self, row: RowId, value: Option<ListOrderLocation>) {
        self.locations.insert(row, value);
    }

    fn subtree_rows(&self, block: Option<u64>) -> usize {
        block
            .and_then(|block| self.block(block))
            .map(|block| block.subtree_rows)
            .unwrap_or(0)
    }

    fn height(&self, block: Option<u64>) -> u16 {
        block
            .and_then(|block| self.block(block))
            .map(|block| block.height)
            .unwrap_or(0)
    }

    fn balance(&self, block: u64) -> Result<i32, Error> {
        let state = self
            .block(block)
            .ok_or_else(|| Error::InvalidPlan("list order balance block is missing".to_owned()))?;
        Ok(i32::from(self.height(state.left)) - i32::from(self.height(state.right)))
    }

    fn refresh_block(&mut self, block: u64) -> Result<(), Error> {
        let (left, right, own_rows) = self
            .block(block)
            .map(|state| (state.left, state.right, state.rows.len()))
            .ok_or_else(|| Error::InvalidPlan("list order tree block is missing".to_owned()))?;
        let height = 1_u16
            .checked_add(self.height(left).max(self.height(right)))
            .ok_or_else(|| Error::Evaluation("list order tree height overflow".to_owned()))?;
        if height > ListOrder::MAX_TREE_HEIGHT {
            return Err(Error::Evaluation(
                "list order exceeds its deterministic tree-height bound".to_owned(),
            ));
        }
        let subtree_rows = own_rows
            .checked_add(self.subtree_rows(left))
            .and_then(|value| value.checked_add(self.subtree_rows(right)))
            .ok_or_else(|| Error::Evaluation("list order subtree size overflow".to_owned()))?;
        let state = self.block_mut(block)?;
        state.height = height;
        state.subtree_rows = subtree_rows;
        Ok(())
    }

    fn refresh_to_root(&mut self, mut block: u64) -> Result<usize, Error> {
        let mut visits = 0_usize;
        loop {
            visits = visits.saturating_add(1);
            self.refresh_block(block)?;
            let Some(parent) = self.block(block).and_then(|state| state.parent) else {
                break;
            };
            block = parent;
        }
        Ok(visits)
    }

    fn locate_index_with_visits(&self, mut index: usize) -> Option<(u64, usize, usize)> {
        if index >= self.len {
            return None;
        }
        let mut block = self.root;
        let mut visits = 0_usize;
        while let Some(block_id) = block {
            visits = visits.saturating_add(1);
            let state = self.block(block_id)?;
            let left_rows = self.subtree_rows(state.left);
            if index < left_rows {
                block = state.left;
            } else if index < left_rows.saturating_add(state.rows.len()) {
                return Some((block_id, index - left_rows, visits));
            } else {
                index = index
                    .saturating_sub(left_rows)
                    .saturating_sub(state.rows.len());
                block = state.right;
            }
        }
        None
    }

    fn get(&self, index: usize) -> Option<RowId> {
        let (block, offset, _) = self.locate_index_with_visits(index)?;
        self.block(block)?.rows.get(offset).copied()
    }

    fn position_with_visits(&self, row: RowId) -> Option<(usize, usize)> {
        let location = self.location(row)?;
        let state = self.block(location.block)?;
        if state.rows.get(location.offset) != Some(&row) {
            return None;
        }
        let mut position = self
            .subtree_rows(state.left)
            .saturating_add(location.offset);
        let mut current = location.block;
        let mut visits = 1_usize;
        while let Some(parent) = self.block(current)?.parent {
            visits = visits.saturating_add(1);
            let parent_state = self.block(parent)?;
            if parent_state.right == Some(current) {
                position = position
                    .saturating_add(self.subtree_rows(parent_state.left))
                    .saturating_add(parent_state.rows.len());
            } else if parent_state.left != Some(current) {
                return None;
            }
            current = parent;
        }
        (self.root == Some(current)).then_some((position, visits))
    }

    fn range_with_visits(&self, range: Range<usize>) -> (Vec<RowId>, usize, usize) {
        let end = range.end.min(self.len);
        let mut visits = 0_usize;
        let mut visit_max = 0_usize;
        let rows = (range.start.min(end)..end)
            .filter_map(|index| {
                let (block, offset, row_visits) = self.locate_index_with_visits(index)?;
                visits = visits.saturating_add(row_visits);
                visit_max = visit_max.max(row_visits);
                self.block(block)?.rows.get(offset).copied()
            })
            .collect();
        (rows, visits, visit_max)
    }

    fn next_block_id(&self) -> Result<(u64, u64), Error> {
        let block = self.next_block.max(1);
        let next = block.checked_add(1).ok_or_else(|| {
            Error::Evaluation("list order exhausted its block identity space".to_owned())
        })?;
        if self.block(block).is_some() {
            return Err(Error::InvalidPlan(
                "list order block allocator repeated an identity".to_owned(),
            ));
        }
        Ok((block, next))
    }

    fn push(&mut self, row: RowId) -> Result<(), Error> {
        if self.location(row).is_some() {
            return Err(Error::InvalidPlan(format!(
                "list order repeats row {}:{}:{}",
                row.list.0, row.key, row.generation
            )));
        }
        let block = match self.last_block {
            Some(block)
                if self
                    .block(block)
                    .is_some_and(|state| state.rows.len() < ListOrder::MAX_BLOCK_ROWS) =>
            {
                block
            }
            previous => self.create_block_after(previous, Vec::new())?,
        };
        let offset = self
            .block(block)
            .map(|state| state.rows.len())
            .ok_or_else(|| Error::InvalidPlan("list order block disappeared".to_owned()))?;
        self.block_mut(block)?.rows.push(row);
        self.set_location(row, Some(ListOrderLocation { block, offset }));
        self.len = self
            .len
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("list order row count overflow".to_owned()))?;
        self.maintenance.row_location_update_count =
            self.maintenance.row_location_update_count.saturating_add(1);
        let visits = self.refresh_to_root(block)?;
        self.maintenance.record_tree_visits(visits);
        Ok(())
    }

    fn insert(&mut self, index: usize, row: RowId) -> Result<(), Error> {
        if index >= self.len {
            return self.push(row);
        }
        if self.location(row).is_some() {
            return Err(Error::InvalidPlan(format!(
                "list order repeats row {}:{}:{}",
                row.list.0, row.key, row.generation
            )));
        }
        let (block, offset, locate_visits) =
            self.locate_index_with_visits(index).ok_or_else(|| {
                Error::InvalidPlan(format!("list order has no insertion position {index}"))
            })?;
        self.block_mut(block)?.rows.insert(offset, row);
        self.len = self
            .len
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("list order row count overflow".to_owned()))?;
        self.maintenance.record_tree_visits(locate_visits);
        let locations = self.refresh_locations(block, offset)?;
        self.maintenance.row_location_update_count = self
            .maintenance
            .row_location_update_count
            .saturating_add(locations);
        let visits = self.refresh_to_root(block)?;
        self.maintenance.record_tree_visits(visits);
        if self
            .block(block)
            .is_some_and(|state| state.rows.len() > ListOrder::MAX_BLOCK_ROWS)
        {
            self.split_block(block)?;
        }
        Ok(())
    }

    fn remove(&mut self, row: RowId) -> Result<Option<usize>, Error> {
        let Some(location) = self.location(row) else {
            return Ok(None);
        };
        let (position, position_visits) = self.position_with_visits(row).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "list order location for row {}:{}:{} is unreachable",
                row.list.0, row.key, row.generation
            ))
        })?;
        if self
            .block(location.block)
            .and_then(|state| state.rows.get(location.offset))
            != Some(&row)
        {
            return Err(Error::InvalidPlan(format!(
                "list order location for row {}:{}:{} is stale",
                row.list.0, row.key, row.generation
            )));
        }
        self.block_mut(location.block)?.rows.remove(location.offset);
        self.set_location(row, None);
        self.len = self
            .len
            .checked_sub(1)
            .ok_or_else(|| Error::InvalidPlan("list order row count underflow".to_owned()))?;
        self.maintenance.record_tree_visits(position_visits);
        let locations = self.refresh_locations(location.block, location.offset)?;
        self.maintenance.row_location_update_count = self
            .maintenance
            .row_location_update_count
            .saturating_add(locations);
        let visits = self.refresh_to_root(location.block)?;
        self.maintenance.record_tree_visits(visits);
        if self
            .block(location.block)
            .is_some_and(|block| block.rows.is_empty())
        {
            self.unlink_block(location.block)?;
        } else if self
            .block(location.block)
            .is_some_and(|block| block.rows.len() < ListOrder::MIN_BLOCK_ROWS)
        {
            self.merge_underfull_block(location.block)?;
        }
        Ok(Some(position))
    }

    fn create_block_after(
        &mut self,
        previous: Option<u64>,
        rows: Vec<RowId>,
    ) -> Result<u64, Error> {
        self.checkpoint(ListOrderFaultPoint::BlockAllocation)?;
        let next = match previous {
            Some(previous) => {
                self.block(previous)
                    .ok_or_else(|| {
                        Error::InvalidPlan("previous list order block is missing".to_owned())
                    })?
                    .next
            }
            None => self.first_block,
        };
        let (block, next_block) = self.next_block_id()?;
        self.blocks.insert(
            block,
            Some(ListOrderBlock {
                previous,
                next,
                height: 1,
                subtree_rows: rows.len(),
                rows,
                ..ListOrderBlock::default()
            }),
        );
        let visits = self.insert_tree_block_after(previous, block)?;
        self.maintenance.record_tree_visits(visits);
        if let Some(previous) = previous {
            self.block_mut(previous)?.next = Some(block);
        } else {
            self.first_block = Some(block);
        }
        if let Some(next) = next {
            self.block_mut(next)?.previous = Some(block);
        } else {
            self.last_block = Some(block);
        }
        self.next_block = next_block;
        Ok(block)
    }

    fn insert_tree_block_after(
        &mut self,
        previous: Option<u64>,
        block: u64,
    ) -> Result<usize, Error> {
        let mut visits = 1_usize;
        let parent = match (self.root, previous) {
            (None, None) => {
                self.root = Some(block);
                return Ok(visits);
            }
            (None, Some(_)) => {
                return Err(Error::InvalidPlan(
                    "list order tree has no root for a non-first block".to_owned(),
                ));
            }
            (Some(_), None) => {
                let first = self.first_block.ok_or_else(|| {
                    Error::InvalidPlan("list order tree has no first block".to_owned())
                })?;
                if self.block(first).and_then(|state| state.left).is_some() {
                    return Err(Error::InvalidPlan(
                        "first list order block unexpectedly has a left child".to_owned(),
                    ));
                }
                self.block_mut(first)?.left = Some(block);
                first
            }
            (Some(_), Some(previous)) => {
                if let Some(right) = self.block(previous).and_then(|state| state.right) {
                    let mut candidate = right;
                    visits = visits.saturating_add(1);
                    while let Some(left) = self.block(candidate).and_then(|state| state.left) {
                        candidate = left;
                        visits = visits.saturating_add(1);
                    }
                    self.block_mut(candidate)?.left = Some(block);
                    candidate
                } else {
                    self.block_mut(previous)?.right = Some(block);
                    previous
                }
            }
        };
        self.block_mut(block)?.parent = Some(parent);
        visits = visits.saturating_add(self.rebalance_from(Some(parent))?);
        Ok(visits)
    }

    fn rebalance_from(&mut self, mut current: Option<u64>) -> Result<usize, Error> {
        self.checkpoint(ListOrderFaultPoint::TreeRebalance)?;
        let mut visits = 0_usize;
        while let Some(block) = current {
            visits = visits.saturating_add(1);
            self.refresh_block(block)?;
            let balance = self.balance(block)?;
            let subtree_root = if balance > 1 {
                let left = self
                    .block(block)
                    .and_then(|state| state.left)
                    .ok_or_else(|| {
                        Error::InvalidPlan(
                            "left-heavy list order block has no left child".to_owned(),
                        )
                    })?;
                if self.balance(left)? < 0 {
                    self.rotate_left(left)?;
                    visits = visits.saturating_add(2);
                }
                visits = visits.saturating_add(2);
                self.rotate_right(block)?
            } else if balance < -1 {
                let right = self
                    .block(block)
                    .and_then(|state| state.right)
                    .ok_or_else(|| {
                        Error::InvalidPlan(
                            "right-heavy list order block has no right child".to_owned(),
                        )
                    })?;
                if self.balance(right)? > 0 {
                    self.rotate_right(right)?;
                    visits = visits.saturating_add(2);
                }
                visits = visits.saturating_add(2);
                self.rotate_left(block)?
            } else {
                block
            };
            current = self.block(subtree_root).and_then(|state| state.parent);
        }
        Ok(visits)
    }

    fn rotate_left(&mut self, pivot: u64) -> Result<u64, Error> {
        let child = self
            .block(pivot)
            .and_then(|state| state.right)
            .ok_or_else(|| Error::InvalidPlan("left rotation has no right child".to_owned()))?;
        let parent = self.block(pivot).and_then(|state| state.parent);
        let middle = self.block(child).and_then(|state| state.left);
        self.replace_tree_child(parent, pivot, Some(child))?;
        self.block_mut(pivot)?.right = middle;
        if let Some(middle) = middle {
            self.block_mut(middle)?.parent = Some(pivot);
        }
        self.block_mut(child)?.left = Some(pivot);
        self.block_mut(pivot)?.parent = Some(child);
        self.refresh_block(pivot)?;
        self.refresh_block(child)?;
        Ok(child)
    }

    fn rotate_right(&mut self, pivot: u64) -> Result<u64, Error> {
        let child = self
            .block(pivot)
            .and_then(|state| state.left)
            .ok_or_else(|| Error::InvalidPlan("right rotation has no left child".to_owned()))?;
        let parent = self.block(pivot).and_then(|state| state.parent);
        let middle = self.block(child).and_then(|state| state.right);
        self.replace_tree_child(parent, pivot, Some(child))?;
        self.block_mut(pivot)?.left = middle;
        if let Some(middle) = middle {
            self.block_mut(middle)?.parent = Some(pivot);
        }
        self.block_mut(child)?.right = Some(pivot);
        self.block_mut(pivot)?.parent = Some(child);
        self.refresh_block(pivot)?;
        self.refresh_block(child)?;
        Ok(child)
    }

    fn replace_tree_child(
        &mut self,
        parent: Option<u64>,
        old_child: u64,
        new_child: Option<u64>,
    ) -> Result<(), Error> {
        if let Some(parent) = parent {
            let state = self.block_mut(parent)?;
            if state.left == Some(old_child) {
                state.left = new_child;
            } else if state.right == Some(old_child) {
                state.right = new_child;
            } else {
                return Err(Error::InvalidPlan(
                    "tree parent does not reference replaced child".to_owned(),
                ));
            }
        } else if self.root == Some(old_child) {
            self.root = new_child;
        } else {
            return Err(Error::InvalidPlan(
                "list order tree root does not match replacement".to_owned(),
            ));
        }
        if let Some(new_child) = new_child {
            self.block_mut(new_child)?.parent = parent;
        }
        Ok(())
    }

    fn minimum_block(&self, mut block: u64) -> Result<(u64, usize), Error> {
        let mut visits = 1_usize;
        while let Some(left) = self.block(block).and_then(|state| state.left) {
            block = left;
            visits = visits.saturating_add(1);
        }
        Ok((block, visits))
    }

    fn detach_tree_block(&mut self, block: u64) -> Result<usize, Error> {
        let state = self
            .block(block)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan("detached list order block is missing".to_owned()))?;
        let mut visits = 1_usize;
        let mut rebalance = Vec::new();
        match (state.left, state.right) {
            (left, None) => {
                self.replace_tree_child(state.parent, block, left)?;
                rebalance.push(state.parent.or(left));
            }
            (None, right) => {
                self.replace_tree_child(state.parent, block, right)?;
                rebalance.push(state.parent.or(right));
            }
            (Some(left), Some(right)) => {
                let (successor, minimum_visits) = self.minimum_block(right)?;
                visits = visits.saturating_add(minimum_visits);
                let successor_parent = self.block(successor).and_then(|value| value.parent);
                if successor_parent != Some(block) {
                    let successor_right = self.block(successor).and_then(|value| value.right);
                    self.replace_tree_child(successor_parent, successor, successor_right)?;
                    self.block_mut(successor)?.right = Some(right);
                    self.block_mut(right)?.parent = Some(successor);
                    rebalance.push(successor_parent);
                }
                self.replace_tree_child(state.parent, block, Some(successor))?;
                self.block_mut(successor)?.left = Some(left);
                self.block_mut(left)?.parent = Some(successor);
                self.refresh_block(successor)?;
                rebalance.push(Some(successor));
            }
        }
        self.blocks.insert(block, None);
        for start in rebalance.into_iter().flatten().collect::<BTreeSet<_>>() {
            visits = visits.saturating_add(self.rebalance_from(Some(start))?);
        }
        Ok(visits)
    }

    fn refresh_locations(&mut self, block: u64, from: usize) -> Result<usize, Error> {
        let rows = self
            .block(block)
            .ok_or_else(|| Error::InvalidPlan("list order block is missing".to_owned()))?
            .rows
            .iter()
            .copied()
            .enumerate()
            .skip(from)
            .collect::<Vec<_>>();
        for (offset, row) in &rows {
            self.set_location(
                *row,
                Some(ListOrderLocation {
                    block,
                    offset: *offset,
                }),
            );
        }
        Ok(rows.len())
    }

    fn split_block(&mut self, block: u64) -> Result<(), Error> {
        self.checkpoint(ListOrderFaultPoint::BlockSplit)?;
        self.next_block_id()?;
        let split_at = self
            .block(block)
            .map(|state| state.rows.len() / 2)
            .ok_or_else(|| Error::InvalidPlan("split list order block is missing".to_owned()))?;
        let rows = self.block_mut(block)?.rows.split_off(split_at);
        let source_visits = self.refresh_to_root(block)?;
        self.maintenance.record_tree_visits(source_visits);
        let new_block = self.create_block_after(Some(block), rows)?;
        let locations = self.refresh_locations(new_block, 0)?;
        self.maintenance.row_location_update_count = self
            .maintenance
            .row_location_update_count
            .saturating_add(locations);
        self.maintenance.block_split_count = self.maintenance.block_split_count.saturating_add(1);
        Ok(())
    }

    fn merge_underfull_block(&mut self, block: u64) -> Result<(), Error> {
        let state = self.block(block).cloned().ok_or_else(|| {
            Error::InvalidPlan("underfull list order block is missing".to_owned())
        })?;
        if let Some(next) = state.next
            && state.rows.len().saturating_add(
                self.block(next)
                    .map(|value| value.rows.len())
                    .unwrap_or(usize::MAX),
            ) <= ListOrder::MAX_BLOCK_ROWS
        {
            return self.merge_blocks(block, next);
        }
        if let Some(previous) = state.previous
            && self
                .block(previous)
                .map(|value| value.rows.len())
                .unwrap_or(usize::MAX)
                .saturating_add(state.rows.len())
                <= ListOrder::MAX_BLOCK_ROWS
        {
            return self.merge_blocks(previous, block);
        }
        Ok(())
    }

    fn merge_blocks(&mut self, left: u64, right: u64) -> Result<(), Error> {
        if self.block(left).and_then(|block| block.next) != Some(right) {
            return Err(Error::InvalidPlan(
                "list order merge blocks are not adjacent".to_owned(),
            ));
        }
        let right_rows = self
            .block(right)
            .ok_or_else(|| Error::InvalidPlan("right list order block is missing".to_owned()))?
            .rows
            .clone();
        let start = self
            .block(left)
            .map(|value| value.rows.len())
            .ok_or_else(|| Error::InvalidPlan("left list order block is missing".to_owned()))?;
        self.unlink_block(right)?;
        self.block_mut(left)?.rows.extend(right_rows);
        let visits = self.refresh_to_root(left)?;
        self.maintenance.record_tree_visits(visits);
        let locations = self.refresh_locations(left, start)?;
        self.maintenance.row_location_update_count = self
            .maintenance
            .row_location_update_count
            .saturating_add(locations);
        self.maintenance.block_merge_count = self.maintenance.block_merge_count.saturating_add(1);
        Ok(())
    }

    fn unlink_block(&mut self, block: u64) -> Result<(), Error> {
        let (previous, next) = self
            .block(block)
            .map(|state| (state.previous, state.next))
            .ok_or_else(|| Error::InvalidPlan("unlinked list order block is missing".to_owned()))?;
        let visits = self.detach_tree_block(block)?;
        self.maintenance.record_tree_visits(visits);
        if let Some(previous) = previous {
            self.block_mut(previous)?.next = next;
        } else {
            self.first_block = next;
        }
        if let Some(next) = next {
            self.block_mut(next)?.previous = previous;
        } else {
            self.last_block = previous;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<PreparedListOrderMutation, Error> {
        self.checkpoint(ListOrderFaultPoint::Finalize)?;
        let after_version = self.base.version.checked_add(1).ok_or_else(|| {
            Error::Evaluation("list order mutation version is exhausted".to_owned())
        })?;
        let before = ListOrderMetadata::read(self.base);
        let after = ListOrderMetadata {
            root: self.root,
            first_block: self.first_block,
            last_block: self.last_block,
            next_block: self.next_block,
            len: self.len,
            version: after_version,
        };
        let blocks = self
            .blocks
            .into_iter()
            .map(|(id, value)| {
                (
                    id,
                    ListOrderPatchValue {
                        before: self.base.blocks.get(&id).cloned(),
                        after: value,
                    },
                )
            })
            .collect();
        let locations = self
            .locations
            .into_iter()
            .map(|(row, value)| {
                (
                    row,
                    ListOrderPatchValue {
                        before: self.base.locations.get(&row).copied(),
                        after: value,
                    },
                )
            })
            .collect();
        Ok(PreparedListOrderMutation {
            patch: ListOrderPatch {
                before,
                after,
                blocks,
                locations,
            },
            maintenance: self.maintenance,
        })
    }
}

struct ListOrderIter<'a> {
    order: &'a ListOrder,
    block: Option<u64>,
    offset: usize,
}

impl<'a> Iterator for ListOrderIter<'a> {
    type Item = &'a RowId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let block = self.block?;
            let state = self
                .order
                .blocks
                .get(&block)
                .expect("list order chain references an existing block");
            if let Some(row) = state.rows.get(self.offset) {
                self.offset += 1;
                return Some(row);
            }
            self.block = state.next;
            self.offset = 0;
        }
    }
}

#[derive(Clone, Debug, Default)]
struct SourceOrderMaintenance {
    storage: ListOrderMaintenance,
    changed_order_rows: BTreeSet<RowId>,
    relabeled_rows: BTreeSet<RowId>,
    relabel_operation_count: usize,
    relabel_window_max: usize,
}

impl SourceOrderMaintenance {
    fn work_units(&self) -> u64 {
        [
            self.storage.row_location_update_count,
            self.storage.block_split_count,
            self.storage.block_merge_count,
            self.storage.tree_visit_count,
            self.relabeled_rows.len(),
            self.relabel_operation_count,
        ]
        .into_iter()
        .fold(0_u64, |total, units| {
            total.saturating_add(units.try_into().unwrap_or(u64::MAX))
        })
    }
}

#[derive(Clone, Debug, Default)]
struct SourceOrderUndo {
    patch: Option<SourceOrderPatch>,
}

#[derive(Clone, Debug)]
struct SourceOrderPatch {
    order: ListOrderPatch,
    tokens: BTreeMap<RowId, ListOrderPatchValue<u128>>,
    before_next_order_token: u128,
    after_next_order_token: u128,
}

impl SourceOrderPatch {
    fn token_entries_match(&self, list: &ListState, after: bool) -> bool {
        self.tokens.iter().all(|(row, change)| {
            list.order_tokens.get(row)
                == if after {
                    change.after.as_ref()
                } else {
                    change.before.as_ref()
                }
        })
    }

    fn matches_before(&self, list: &ListState) -> bool {
        self.order.matches_before(&list.order)
            && list.next_order_token == self.before_next_order_token
            && self.token_entries_match(list, false)
    }

    fn matches_after(&self, list: &ListState) -> bool {
        self.order.matches_after(&list.order)
            && list.next_order_token == self.after_next_order_token
            && self.token_entries_match(list, true)
    }

    fn apply_forward(&self, list: &mut ListState) {
        assert!(
            self.matches_before(list),
            "prepared source-order patch lost its canonical base"
        );
        self.order.apply_forward(&mut list.order);
        Self::apply_tokens(&mut list.order_tokens, &self.tokens, true);
        list.next_order_token = self.after_next_order_token;
    }

    fn apply_reverse_idempotent(&self, list: &mut ListState) -> Result<(), Error> {
        if self.matches_before(list) {
            return Ok(());
        }
        if !self.matches_after(list) {
            return Err(Error::InvalidPlan(
                "source-order rollback no longer matches its prepared authority".to_owned(),
            ));
        }
        self.order.apply_reverse_idempotent(&mut list.order)?;
        Self::apply_tokens(&mut list.order_tokens, &self.tokens, false);
        list.next_order_token = self.before_next_order_token;
        Ok(())
    }

    fn apply_tokens(
        target: &mut BTreeMap<RowId, u128>,
        changes: &BTreeMap<RowId, ListOrderPatchValue<u128>>,
        forward: bool,
    ) {
        for (row, change) in changes {
            let value = if forward { change.after } else { change.before };
            match value {
                Some(value) => {
                    target.insert(*row, value);
                }
                None => {
                    target.remove(row);
                }
            }
        }
    }
}

struct PreparedSourceOrderMutation {
    patch: SourceOrderPatch,
    maintenance: SourceOrderMaintenance,
}

impl PreparedSourceOrderMutation {
    fn commit(self, list: &mut ListState) -> SourceOrderUndo {
        self.patch.apply_forward(list);
        SourceOrderUndo {
            patch: Some(self.patch),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ListState {
    rows: BTreeMap<RowId, Row>,
    order: ListOrder,
    order_tokens: BTreeMap<RowId, u128>,
    next_order_token: u128,
    revision: u64,
    owner_partitions: BTreeMap<Vec<OwnerInstanceRow>, OwnerPartition>,
    next_key: u64,
}

#[derive(Clone, Debug, Default)]
struct OwnerPartition {
    order: Vec<RowId>,
    by_materialization_origin: BTreeMap<Vec<OwnerInstanceRow>, RowId>,
}

impl ListState {
    const ORDER_TOKEN_STRIDE: u128 = 1_u128 << 64;
    const INITIAL_RELABEL_ROWS: usize = 16;
    const MAX_RELABEL_ROWS: usize = 256;

    #[cfg(test)]
    fn from_authority(
        rows: BTreeMap<RowId, Row>,
        order: Vec<RowId>,
        order_tokens: BTreeMap<RowId, u128>,
        next_order_token: u128,
        next_key: u64,
        revision: u64,
    ) -> Result<Self, Error> {
        let ordered = order.iter().copied().collect::<BTreeSet<_>>();
        if ordered.len() != order.len()
            || order_tokens.len() != order.len()
            || rows.len() != order.len()
            || rows.keys().copied().collect::<BTreeSet<_>>() != ordered
        {
            return Err(Error::InvalidPlan(
                "restored list authority rows, order, and source-order labels differ".to_owned(),
            ));
        }
        let mut previous = 0_u128;
        for row in &order {
            let token = order_tokens.get(row).copied().ok_or_else(|| {
                Error::InvalidPlan("restored list row has no source-order label".to_owned())
            })?;
            if token == 0 || token <= previous {
                return Err(Error::InvalidPlan(
                    "restored list source-order labels are not strictly increasing".to_owned(),
                ));
            }
            previous = token;
        }
        if next_order_token <= previous {
            return Err(Error::InvalidPlan(
                "restored list source-order allocator does not follow its rows".to_owned(),
            ));
        }
        let order = ListOrder::from_rows(order)?;
        Self::from_validated_authority(
            rows,
            order,
            order_tokens,
            next_order_token,
            next_key,
            revision,
        )
    }

    fn from_validated_authority(
        rows: BTreeMap<RowId, Row>,
        order: ListOrder,
        order_tokens: BTreeMap<RowId, u128>,
        next_order_token: u128,
        next_key: u64,
        revision: u64,
    ) -> Result<Self, Error> {
        if rows.len() != order.len() || order_tokens.len() != order.len() {
            return Err(Error::InvalidPlan(
                "validated list authority has inconsistent row counts".to_owned(),
            ));
        }
        Ok(Self {
            rows,
            order,
            order_tokens,
            next_order_token,
            revision,
            owner_partitions: BTreeMap::new(),
            next_key,
        })
    }

    #[cfg(test)]
    fn push_ordered(&mut self, row: RowId) -> Result<SourceOrderMaintenance, Error> {
        let prepared = self.prepare_push_ordered(row, None)?;
        let maintenance = prepared.maintenance.clone();
        prepared.commit(self);
        Ok(maintenance)
    }

    fn prepare_push_ordered(
        &self,
        row: RowId,
        fault: Option<ListOrderFaultPoint>,
    ) -> Result<PreparedSourceOrderMutation, Error> {
        if self.order.position(row).is_some() || self.order_tokens.contains_key(&row) {
            return Err(Error::InvalidPlan(format!(
                "list source order repeats row {}:{}:{}",
                row.list.0, row.key, row.generation
            )));
        }
        let token = self.next_order_token.max(Self::ORDER_TOKEN_STRIDE);
        let next_order_token = token.checked_add(Self::ORDER_TOKEN_STRIDE).ok_or_else(|| {
            Error::Evaluation("list source-order token space is exhausted".to_owned())
        })?;
        let order = self.order.prepare_push(row, fault)?;
        self.prepared_source_order(
            order,
            BTreeMap::from([(row, Some(token))]),
            next_order_token,
            SourceOrderMaintenance::default(),
        )
    }

    fn prepare_remove_ordered(
        &self,
        row: RowId,
        fault: Option<ListOrderFaultPoint>,
    ) -> Result<Option<PreparedSourceOrderMutation>, Error> {
        let Some(order) = self.order.prepare_remove(row, fault)? else {
            return Ok(None);
        };
        let prepared = self.prepared_source_order(
            order,
            BTreeMap::from([(row, None)]),
            self.next_order_token,
            SourceOrderMaintenance::default(),
        )?;
        Ok(Some(prepared))
    }

    fn order_token(&self, row: RowId) -> Option<u128> {
        self.order_tokens.get(&row).copied()
    }

    fn staged_order_token(
        &self,
        changes: &BTreeMap<RowId, Option<u128>>,
        row: RowId,
    ) -> Option<u128> {
        changes
            .get(&row)
            .copied()
            .flatten()
            .or_else(|| self.order_token(row))
    }

    fn order_token_for_prepared_insertion(
        &self,
        order: &ListOrderDraft<'_>,
        index: usize,
        changes: &mut BTreeMap<RowId, Option<u128>>,
        next_order_token: &mut u128,
        maintenance: &mut SourceOrderMaintenance,
    ) -> Result<u128, Error> {
        let lower = index
            .checked_sub(1)
            .and_then(|position| order.get(position))
            .and_then(|row| self.staged_order_token(changes, row))
            .unwrap_or(0);
        if index == order.len {
            let minimum = lower.checked_add(Self::ORDER_TOKEN_STRIDE).ok_or_else(|| {
                Error::Evaluation("list source-order token space is exhausted".to_owned())
            })?;
            let token = (*next_order_token).max(minimum);
            *next_order_token = token.checked_add(Self::ORDER_TOKEN_STRIDE).ok_or_else(|| {
                Error::Evaluation("list source-order token space is exhausted".to_owned())
            })?;
            return Ok(token);
        }
        let upper = order
            .get(index)
            .and_then(|row| self.staged_order_token(changes, row))
            .ok_or_else(|| Error::InvalidPlan("list order token is missing".to_owned()))?;
        let available = upper.checked_sub(lower).ok_or_else(|| {
            Error::InvalidPlan("list source-order labels are not increasing".to_owned())
        })?;
        if available > 1 {
            return Ok(lower + available / 2);
        }
        self.prepare_bounded_relabel(order, index, changes, *next_order_token, maintenance)
    }

    fn prepare_bounded_relabel(
        &self,
        order: &ListOrderDraft<'_>,
        index: usize,
        changes: &mut BTreeMap<RowId, Option<u128>>,
        next_order_token: u128,
        maintenance: &mut SourceOrderMaintenance,
    ) -> Result<u128, Error> {
        let len = order.len;
        let maximum = Self::MAX_RELABEL_ROWS.min(len);
        let mut window = Self::INITIAL_RELABEL_ROWS.min(maximum.max(1));
        loop {
            let mut start = index.saturating_sub(window / 2);
            let end = start.saturating_add(window).min(len);
            start = end.saturating_sub(window);
            if index < start || index > end {
                return Err(Error::InvalidPlan(
                    "source-order relabel window does not contain insertion".to_owned(),
                ));
            }
            let lower = start
                .checked_sub(1)
                .and_then(|position| order.get(position))
                .and_then(|row| self.staged_order_token(changes, row))
                .unwrap_or(0);
            let upper = if let Some(row) = order.get(end) {
                self.staged_order_token(changes, row)
                    .ok_or_else(|| Error::InvalidPlan("list order token is missing".to_owned()))?
            } else {
                next_order_token
            };
            let existing = end.saturating_sub(start);
            let divisor = u128::try_from(existing)
                .ok()
                .and_then(|value| value.checked_add(2))
                .ok_or_else(|| {
                    Error::Evaluation("source-order relabel size overflow".to_owned())
                })?;
            let available = upper.checked_sub(lower).ok_or_else(|| {
                Error::InvalidPlan("source-order relabel boundaries are reversed".to_owned())
            })?;
            let step = available / divisor;
            if step >= 1 {
                let (rows, visits, visit_max) = order.range_with_visits(start..end);
                maintenance.storage.tree_visit_count =
                    maintenance.storage.tree_visit_count.saturating_add(visits);
                maintenance.storage.tree_visit_max =
                    maintenance.storage.tree_visit_max.max(visit_max);
                for (offset, row) in rows.into_iter().enumerate() {
                    let old_position = start + offset;
                    let ordinal = if old_position < index {
                        offset as u128 + 1
                    } else {
                        offset as u128 + 2
                    };
                    let token = step
                        .checked_mul(ordinal)
                        .and_then(|value| lower.checked_add(value))
                        .ok_or_else(|| {
                            Error::Evaluation("source-order relabel arithmetic overflow".to_owned())
                        })?;
                    changes.insert(row, Some(token));
                }
                maintenance.relabel_operation_count =
                    maintenance.relabel_operation_count.saturating_add(1);
                maintenance.relabel_window_max = maintenance.relabel_window_max.max(existing);
                let insertion_ordinal = (index - start) as u128 + 1;
                return step
                    .checked_mul(insertion_ordinal)
                    .and_then(|value| lower.checked_add(value))
                    .ok_or_else(|| {
                        Error::Evaluation("source-order insertion label overflow".to_owned())
                    });
            }
            if window >= maximum {
                break;
            }
            window = window.saturating_mul(2).min(maximum);
        }
        Err(Error::Evaluation(format!(
            "source-order insertion has no label space inside the bounded {}-row maintenance window",
            Self::MAX_RELABEL_ROWS
        )))
    }

    fn prepare_reorder_rows(
        &self,
        insertion_index: usize,
        ordered_rows: &[RowId],
        fault: Option<ListOrderFaultPoint>,
    ) -> Result<Option<PreparedSourceOrderMutation>, Error> {
        let selected = ordered_rows.iter().copied().collect::<BTreeSet<_>>();
        if selected.len() != ordered_rows.len()
            || selected.iter().any(|row| !self.rows.contains_key(row))
        {
            return Err(Error::InvalidPlan(
                "materialized list partition order contains invalid rows".to_owned(),
            ));
        }
        let insertion_index = insertion_index.min(self.order.len().saturating_sub(selected.len()));
        if ordered_rows.iter().enumerate().all(|(offset, row)| {
            self.order.get(insertion_index.saturating_add(offset)) == Some(row)
        }) {
            return Ok(None);
        }
        let mut order = ListOrderDraft::new(&self.order, fault);
        let mut token_changes = BTreeMap::<RowId, Option<u128>>::new();
        let mut next_order_token = self.next_order_token;
        let mut maintenance = SourceOrderMaintenance::default();
        for row in ordered_rows {
            if order.remove(*row)?.is_none() {
                return Err(Error::InvalidPlan(format!(
                    "materialized row {}:{}:{} disappeared from source order",
                    row.list.0, row.key, row.generation
                )));
            }
            token_changes.insert(*row, None);
        }
        for (offset, row) in ordered_rows.iter().copied().enumerate() {
            let index = insertion_index.saturating_add(offset);
            let token = self.order_token_for_prepared_insertion(
                &order,
                index,
                &mut token_changes,
                &mut next_order_token,
                &mut maintenance,
            )?;
            token_changes.insert(row, Some(token));
            order.insert(index, row)?;
        }
        let prepared_order = order.finish()?;
        self.prepared_source_order(prepared_order, token_changes, next_order_token, maintenance)
            .map(Some)
    }

    #[cfg(test)]
    fn reorder_rows(
        &mut self,
        insertion_index: usize,
        ordered_rows: &[RowId],
    ) -> Result<(bool, SourceOrderMaintenance, SourceOrderUndo), Error> {
        let Some(prepared) = self.prepare_reorder_rows(insertion_index, ordered_rows, None)? else {
            return Ok((
                false,
                SourceOrderMaintenance::default(),
                SourceOrderUndo::default(),
            ));
        };
        let maintenance = prepared.maintenance.clone();
        let undo = prepared.commit(self);
        Ok((true, maintenance, undo))
    }

    fn prepared_source_order(
        &self,
        order: PreparedListOrderMutation,
        token_after: BTreeMap<RowId, Option<u128>>,
        after_next_order_token: u128,
        mut maintenance: SourceOrderMaintenance,
    ) -> Result<PreparedSourceOrderMutation, Error> {
        let tokens = token_after
            .into_iter()
            .map(|(row, after)| {
                let before = self.order_tokens.get(&row).copied();
                if before != after {
                    maintenance.changed_order_rows.insert(row);
                    if before.is_some() && after.is_some() {
                        maintenance.relabeled_rows.insert(row);
                    }
                }
                (row, ListOrderPatchValue { before, after })
            })
            .collect();
        maintenance.storage.merge(order.maintenance.clone());
        Ok(PreparedSourceOrderMutation {
            patch: SourceOrderPatch {
                order: order.patch,
                tokens,
                before_next_order_token: self.next_order_token,
                after_next_order_token,
            },
            maintenance,
        })
    }

    fn restore_source_order(&mut self, undo: &SourceOrderUndo) -> Result<(), Error> {
        if let Some(patch) = &undo.patch {
            patch.apply_reverse_idempotent(self)?;
        }
        Ok(())
    }

    fn rebuild_owner_partitions(&mut self) -> Result<(), Error> {
        self.owner_partitions.clear();
        for row_id in self.order.to_vec() {
            self.index_owner_partition_row(row_id)?;
        }
        Ok(())
    }

    fn index_owner_partition_row(&mut self, row_id: RowId) -> Result<(), Error> {
        let row = self.rows.get(&row_id).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "list {} order contains missing row {}:{}",
                row_id.list.0, row_id.key, row_id.generation
            ))
        })?;
        let Some((leaf, owner_prefix)) = row.owner_ancestors.split_last() else {
            return Err(Error::InvalidPlan(format!(
                "row {}:{}:{} has empty structural ownership",
                row_id.list.0, row_id.key, row_id.generation
            )));
        };
        let expected_leaf = OwnerInstanceRow {
            list: row_id.list,
            key: row_id.key,
            generation: row_id.generation,
        };
        if *leaf != expected_leaf {
            return Err(Error::InvalidPlan(format!(
                "row {}:{}:{} structural owner has a different leaf",
                row_id.list.0, row_id.key, row_id.generation
            )));
        }
        let partition = self
            .owner_partitions
            .entry(owner_prefix.to_vec())
            .or_default();
        partition.order.push(row_id);
        if let Some(origin) = &row.materialization_origin
            && partition
                .by_materialization_origin
                .insert(origin.clone(), row_id)
                .is_some()
        {
            return Err(Error::InvalidPlan(format!(
                "list {} owner partition repeats a materialization origin",
                row_id.list.0
            )));
        }
        Ok(())
    }

    fn remove_owner_partition_row(&mut self, row_id: RowId, row: &Row) {
        let Some((_, owner_prefix)) = row.owner_ancestors.split_last() else {
            return;
        };
        let mut remove_partition = false;
        if let Some(partition) = self.owner_partitions.get_mut(owner_prefix) {
            partition.order.retain(|candidate| *candidate != row_id);
            if let Some(origin) = &row.materialization_origin {
                partition.by_materialization_origin.remove(origin);
            }
            remove_partition = partition.order.is_empty();
        }
        if remove_partition {
            self.owner_partitions.remove(owner_prefix);
        }
    }
}

fn access_index_plan_id(id: PlanListIndexId) -> AccessIndexPlanId {
    AccessIndexPlanId::from_u128(id.0 as u128)
}

fn access_row_id(row: RowId) -> AccessRowId {
    AccessRowId::from_u128((u128::from(row.key) << 64) | u128::from(row.generation))
}

fn runtime_row_id(list: ListId, row: AccessRowId) -> RowId {
    let value = u128::from_be_bytes(*row.as_bytes());
    RowId {
        list,
        key: (value >> 64) as u64,
        generation: value as u64,
    }
}

fn source_order_token(token: u128) -> SourceOrderToken {
    SourceOrderToken::from_u128(token)
}

fn ordered_index_schema(plan: &PlanListIndex) -> Result<KeySchema, Error> {
    let components = plan
        .keys
        .iter()
        .map(|key| {
            let kind = match key.kind {
                PlanListIndexKeyKind::Number => AccessKeyKind::Number,
                PlanListIndexKeyKind::Text => AccessKeyKind::Text,
                PlanListIndexKeyKind::Bool => AccessKeyKind::Bool,
                PlanListIndexKeyKind::ClosedTag { type_id } => {
                    AccessKeyKind::ClosedTag(boon_list_access::TagTypeId::from_bytes(type_id))
                }
            };
            let direction = match key.direction {
                PlanOrderDirection::Ascending => AccessDirection::Asc,
                PlanOrderDirection::Descending => AccessDirection::Desc,
            };
            KeyComponent::new(kind, direction)
        })
        .collect::<Vec<_>>();
    KeySchema::new(components).map_err(|error| Error::InvalidPlan(error.to_string()))
}

fn structural_index_value(plan: &PlanListIndexKey, value: Value) -> Result<StructuralValue, Error> {
    match (plan.kind, value) {
        (PlanListIndexKeyKind::Number, Value::Number(value)) => {
            StructuralValue::number(value.get())
                .map_err(|error| Error::Evaluation(error.to_string()))
        }
        (PlanListIndexKeyKind::Text, Value::Text(value)) => Ok(StructuralValue::text(value)),
        (PlanListIndexKeyKind::Bool, Value::Bool(value)) => Ok(StructuralValue::Bool(value)),
        (PlanListIndexKeyKind::ClosedTag { type_id }, Value::Text(value)) => {
            let ordinal = plan
                .closed_tags
                .binary_search(&value)
                .map_err(|_| {
                    Error::Evaluation(format!(
                        "closed-tag ordered index evaluated unknown variant `{value}`"
                    ))
                })?
                .try_into()
                .map_err(|_| Error::InvalidPlan("closed-tag ordinal exceeds u32".to_owned()))?;
            Ok(StructuralValue::ClosedTag(AccessClosedTag::new(
                boon_list_access::TagTypeId::from_bytes(type_id),
                ordinal,
            )))
        }
        (kind, value) => Err(Error::Evaluation(format!(
            "typed ordered index key {kind:?} evaluated as {value:?}"
        ))),
    }
}

fn ordered_index_key_error(
    plan: &PlanListIndex,
    row: RowId,
    key_position: usize,
    key: &PlanListIndexKey,
    error: Error,
) -> Error {
    let expression = format!("{:?}", key.expression);
    let expression = expression.chars().take(512).collect::<String>();
    Error::Evaluation(format!(
        "ordered index {} on list {} row {}:{} key {key_position} expression {expression}: {error}",
        plan.id.0, row.list.0, row.key, row.generation
    ))
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Consumer {
    Root(FieldId),
    List(ListId),
    Row(RowId, FieldId),
    ProducerResult(RemoteCallSiteId),
    Effect(EffectConsumer),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct EffectConsumer {
    op: PlanOpId,
    row: Option<RowId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EffectActivation {
    invocation_id: EffectInvocationId,
    owner: OwnerInstanceId,
    source_event: Option<SourceEvent>,
}

type ListAccessSubscriptions = Vec<(EvaluatedListAccessSelection, BTreeSet<Consumer>)>;
type OrderedIndexCursorSnapshot = BTreeMap<RowId, Vec<AccessCursorKey>>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DynamicDependency {
    RootState(StateId),
    RootField(FieldId),
    RowField(RowId, FieldId),
    ListAccess(PlanListIndexId, EvaluatedListAccessSelection),
    List(ListId),
    DistributedImport(ImportId),
    DistributedCallResult(ImportId, DistributedCallInstanceId),
}

#[derive(Clone, Debug, Default)]
struct DynamicDependencies {
    by_root_state: BTreeMap<StateId, BTreeSet<Consumer>>,
    by_root_field: BTreeMap<FieldId, BTreeSet<Consumer>>,
    by_row_field: BTreeMap<(RowId, FieldId), BTreeSet<Consumer>>,
    by_list_access: BTreeMap<(PlanListIndexId, EvaluatedListAccessSelection), BTreeSet<Consumer>>,
    by_list: BTreeMap<ListId, BTreeSet<Consumer>>,
    by_distributed_import: BTreeMap<ImportId, BTreeSet<Consumer>>,
    by_distributed_call_result: BTreeMap<(ImportId, DistributedCallInstanceId), BTreeSet<Consumer>>,
    by_consumer: BTreeMap<Consumer, BTreeSet<DynamicDependency>>,
}

impl DynamicDependencies {
    fn clear(&mut self, consumer: Consumer) {
        let Some(dependencies) = self.by_consumer.remove(&consumer) else {
            return;
        };
        for dependency in dependencies {
            match dependency {
                DynamicDependency::RootState(state) => {
                    remove_consumer(&mut self.by_root_state, &state, consumer)
                }
                DynamicDependency::RootField(field) => {
                    remove_consumer(&mut self.by_root_field, &field, consumer)
                }
                DynamicDependency::RowField(row, field) => {
                    remove_consumer(&mut self.by_row_field, &(row, field), consumer)
                }
                DynamicDependency::ListAccess(index, selection) => {
                    remove_consumer(&mut self.by_list_access, &(index, selection), consumer)
                }
                DynamicDependency::List(list) => {
                    remove_consumer(&mut self.by_list, &list, consumer)
                }
                DynamicDependency::DistributedImport(import) => {
                    remove_consumer(&mut self.by_distributed_import, &import, consumer)
                }
                DynamicDependency::DistributedCallResult(import, instance) => remove_consumer(
                    &mut self.by_distributed_call_result,
                    &(import, instance),
                    consumer,
                ),
            }
        }
    }

    fn insert(&mut self, consumer: Consumer, dependency: DynamicDependency) {
        self.by_consumer
            .entry(consumer)
            .or_default()
            .insert(dependency.clone());
        match dependency {
            DynamicDependency::RootState(state) => {
                self.by_root_state
                    .entry(state)
                    .or_default()
                    .insert(consumer);
            }
            DynamicDependency::RootField(field) => {
                self.by_root_field
                    .entry(field)
                    .or_default()
                    .insert(consumer);
            }
            DynamicDependency::RowField(row, field) => {
                self.by_row_field
                    .entry((row, field))
                    .or_default()
                    .insert(consumer);
            }
            DynamicDependency::ListAccess(index, selection) => {
                self.by_list_access
                    .entry((index, selection))
                    .or_default()
                    .insert(consumer);
            }
            DynamicDependency::List(list) => {
                self.by_list.entry(list).or_default().insert(consumer);
            }
            DynamicDependency::DistributedImport(import) => {
                self.by_distributed_import
                    .entry(import)
                    .or_default()
                    .insert(consumer);
            }
            DynamicDependency::DistributedCallResult(import, instance) => {
                self.by_distributed_call_result
                    .entry((import, instance))
                    .or_default()
                    .insert(consumer);
            }
        }
    }
}

fn remove_consumer<K: Ord + Clone>(
    map: &mut BTreeMap<K, BTreeSet<Consumer>>,
    key: &K,
    consumer: Consumer,
) {
    let remove_key = map.get_mut(key).is_some_and(|consumers| {
        consumers.remove(&consumer);
        consumers.is_empty()
    });
    if remove_key {
        map.remove(key);
    }
}

fn take_map_where<K, V>(
    map: &mut BTreeMap<K, V>,
    mut predicate: impl FnMut(&K, &V) -> bool,
) -> BTreeMap<K, V>
where
    K: Clone + Ord,
{
    let keys = map
        .iter()
        .filter_map(|(key, value)| predicate(key, value).then_some(key.clone()))
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| map.remove(&key).map(|value| (key, value)))
        .collect()
}

fn take_set_where<T>(set: &mut BTreeSet<T>, mut predicate: impl FnMut(&T) -> bool) -> BTreeSet<T>
where
    T: Copy + Ord,
{
    let values = set
        .iter()
        .copied()
        .filter(|value| predicate(value))
        .collect::<Vec<_>>();
    for value in &values {
        set.remove(value);
    }
    values.into_iter().collect()
}

#[derive(Clone, Debug, Default)]
struct Dependencies {
    root_by_state: BTreeMap<StateId, BTreeSet<FieldId>>,
    root_by_field: BTreeMap<FieldId, BTreeSet<FieldId>>,
    root_by_list: BTreeMap<ListId, BTreeSet<FieldId>>,
    list_by_state: BTreeMap<StateId, BTreeSet<ListId>>,
    list_by_field: BTreeMap<FieldId, BTreeSet<ListId>>,
    list_by_list: BTreeMap<ListId, BTreeSet<ListId>>,
    row_by_field: BTreeMap<(ListId, FieldId), BTreeSet<FieldId>>,
    row_by_root_state: BTreeMap<StateId, BTreeSet<(ListId, FieldId)>>,
    row_by_root_field: BTreeMap<FieldId, BTreeSet<(ListId, FieldId)>>,
    row_by_list: BTreeMap<ListId, BTreeSet<(ListId, FieldId)>>,
}

#[derive(Clone, Debug)]
struct DurableListRuntimeMetadata {
    list_id: ListId,
    fields_by_leaf: BTreeMap<boon_plan::MemoryLeafId, FieldId>,
}

#[derive(Clone, Debug)]
struct Metadata {
    constants: BTreeMap<PlanConstantId, Value>,
    field_labels: BTreeMap<FieldId, String>,
    distributed_import_types: BTreeMap<ImportId, DataTypePlan>,
    recoverable_distributed_imports: BTreeSet<ImportId>,
    row_owned_call_results: BTreeMap<ImportId, RemoteCallSitePlan>,
    producer_function_instances: BTreeMap<RemoteCallSiteId, ProducerFunctionInstancePlan>,
    root_computations: BTreeMap<FieldId, Arc<PlanOp>>,
    list_computations: BTreeMap<ListId, Arc<PlanOp>>,
    row_computations: BTreeMap<FieldId, Arc<PlanOp>>,
    currentness_ops: BTreeMap<PlanOpId, Arc<PlanOp>>,
    derived_expression_count: usize,
    row_field_owner: BTreeMap<FieldId, ListId>,
    indexed_state_field: BTreeMap<StateId, FieldId>,
    indexed_state_owner: BTreeMap<StateId, ListId>,
    state_owners: BTreeMap<StateId, PlanOwner>,
    list_by_scope: BTreeMap<ScopeId, ListId>,
    list_fields_by_name: BTreeMap<(ListId, String), Vec<FieldId>>,
    list_authority_fields: BTreeMap<(ListId, String), FieldId>,
    deferred_authority_aliases: BTreeMap<(ListId, FieldId), FieldId>,
    deferred_value_aliases: BTreeMap<(ListId, FieldId), FieldId>,
    capture_fields: BTreeSet<FieldId>,
    list_fields_by_exact_name: BTreeMap<String, Vec<(ListId, FieldId)>>,
    row_field_names: BTreeMap<(ListId, FieldId), String>,
    semantic_list_identities: BTreeMap<ListId, ([u8; 32], [u8; 32])>,
    semantic_row_field_identities: BTreeMap<(ListId, FieldId), [u8; 32]>,
    list_indexes: BTreeMap<PlanListIndexId, PlanListIndex>,
    ordered_indexes_by_list: BTreeMap<ListId, BTreeSet<PlanListIndexId>>,
    ordered_indexes_by_row_field: BTreeMap<(ListId, FieldId), BTreeSet<PlanListIndexId>>,
    root_field_by_exact_name: BTreeMap<String, Vec<FieldId>>,
    root_field_by_name: BTreeMap<String, Vec<FieldId>>,
    root_state_by_exact_name: BTreeMap<String, Vec<StateId>>,
    root_state_by_name: BTreeMap<String, Vec<StateId>>,
    routes: BTreeMap<SourceId, SourceRoute>,
    updates_by_source: BTreeMap<SourceId, Vec<Arc<PlanOp>>>,
    updates_by_state: BTreeMap<StateId, Vec<Arc<PlanOp>>>,
    effect_updates_by_id: BTreeMap<PlanOpId, Arc<PlanOp>>,
    mutations_by_source: BTreeMap<SourceId, Vec<Arc<PlanOp>>>,
    mutations_by_state: BTreeMap<StateId, Vec<Arc<PlanOp>>>,
    source_derived_by_source: BTreeMap<SourceId, BTreeSet<FieldId>>,
    state_derived_by_state: BTreeMap<StateId, BTreeSet<FieldId>>,
    source_derived_lists_by_source: BTreeMap<SourceId, BTreeSet<ListId>>,
    state_derived_lists_by_state: BTreeMap<StateId, BTreeSet<ListId>>,
    durable_root_states: BTreeSet<StateId>,
    durable_row_fields: BTreeSet<(ListId, FieldId)>,
    durable_lists: BTreeSet<ListId>,
    durable_root_state_by_memory: BTreeMap<boon_plan::MemoryId, StateId>,
    durable_list_by_memory: BTreeMap<boon_plan::MemoryId, DurableListRuntimeMetadata>,
    session_info_root_fields: BTreeSet<FieldId>,
    session_info_row_fields: BTreeSet<FieldId>,
    published: BTreeSet<FieldId>,
    dependencies: Dependencies,
}

fn derived_expression_has_intrinsic(
    expression: &PlanDerivedExpression,
    arena: &PlanRowExpressionArena,
) -> Result<bool, Error> {
    let mut found = false;
    expression
        .visit_intrinsics(arena, &mut |_| found = true)
        .map_err(|error| Error::InvalidPlan(error.to_string()))?;
    Ok(found)
}

fn derived_expression_event_triggers(expression: &PlanDerivedExpression) -> Vec<ValueRef> {
    let mut pending = vec![expression];
    let mut triggers = Vec::new();
    while let Some(expression) = pending.pop() {
        match expression {
            PlanDerivedExpression::MaterializeList { expression, .. }
            | PlanDerivedExpression::BoolNotExpression { input: expression } => {
                pending.push(expression);
            }
            PlanDerivedExpression::BoolAnd { left, right } => {
                pending.push(right);
                pending.push(left);
            }
            PlanDerivedExpression::SourceEventTransform { arms, .. } => {
                for arm in arms {
                    if !triggers.contains(&arm.trigger) {
                        triggers.push(arm.trigger.clone());
                    }
                }
            }
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty { source_id, .. } => {
                let trigger = ValueRef::Source(*source_id);
                if !triggers.contains(&trigger) {
                    triggers.push(trigger);
                }
            }
            PlanDerivedExpression::BoolNot { .. }
            | PlanDerivedExpression::NumberCompareConst { .. }
            | PlanDerivedExpression::ValueCompare { .. }
            | PlanDerivedExpression::RowExpression { .. }
            | PlanDerivedExpression::MaterializedRowField { .. } => {}
        }
    }
    triggers
}

fn derived_expression_node_count(expression: &PlanDerivedExpression) -> usize {
    let mut pending = vec![expression];
    let mut count = 0usize;
    while let Some(expression) = pending.pop() {
        count = count.saturating_add(1);
        match expression {
            PlanDerivedExpression::MaterializeList { expression, .. }
            | PlanDerivedExpression::BoolNotExpression { input: expression } => {
                pending.push(expression);
            }
            PlanDerivedExpression::BoolAnd { left, right } => {
                pending.push(right);
                pending.push(left);
            }
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
            | PlanDerivedExpression::SourceEventTransform { .. }
            | PlanDerivedExpression::BoolNot { .. }
            | PlanDerivedExpression::NumberCompareConst { .. }
            | PlanDerivedExpression::ValueCompare { .. }
            | PlanDerivedExpression::RowExpression { .. }
            | PlanDerivedExpression::MaterializedRowField { .. } => {}
        }
    }
    count
}

fn typed_access_and_direct_list_inputs(
    op: &PlanOp,
    arena: &PlanRowExpressionArena,
    indexes: &BTreeMap<PlanListIndexId, PlanListIndex>,
) -> Result<(BTreeSet<ListId>, BTreeSet<ListId>), Error> {
    fn collect_row(
        expression: PlanRowExpressionId,
        arena: &PlanRowExpressionArena,
        indexes: &BTreeMap<PlanListIndexId, PlanListIndex>,
        access: &mut BTreeSet<ListId>,
        direct: &mut BTreeSet<ListId>,
    ) -> Result<(), Error> {
        let mut pending = vec![expression];
        let mut visited = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if !visited.insert(expression) {
                continue;
            }
            let node = arena
                .node(expression)
                .map_err(|error| Error::InvalidPlan(error.to_string()))?;
            let access_id = match node {
                PlanRowExpressionNode::ListAccess { access } => Some(access.index),
                PlanRowExpressionNode::ListPage { page } => Some(page.access.index),
                PlanRowExpressionNode::ContextualCollection {
                    indexed_access: Some(access),
                    ..
                } => Some(access.index),
                PlanRowExpressionNode::ListRef { list_id }
                | PlanRowExpressionNode::AuthorityListRef { list_id } => {
                    direct.insert(*list_id);
                    None
                }
                _ => None,
            };
            if let Some(index) = access_id {
                if let Some(index) = indexes.get(&index) {
                    access.insert(index.source_list);
                }
                continue;
            }
            node.visit_children(&mut |child| pending.push(child));
        }
        Ok(())
    }

    fn collect_derived(
        expression: &PlanDerivedExpression,
        arena: &PlanRowExpressionArena,
        indexes: &BTreeMap<PlanListIndexId, PlanListIndex>,
        access: &mut BTreeSet<ListId>,
        direct: &mut BTreeSet<ListId>,
    ) -> Result<(), Error> {
        let mut pending = vec![expression];
        while let Some(expression) = pending.pop() {
        match expression {
            PlanDerivedExpression::MaterializeList { expression, .. }
            | PlanDerivedExpression::BoolNotExpression { input: expression } => {
                    pending.push(expression);
            }
            PlanDerivedExpression::BoolAnd { left, right } => {
                    pending.push(right);
                    pending.push(left);
            }
            PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
                collect_row(*default, arena, indexes, access, direct)?;
                for arm in arms {
                    collect_row(arm.value, arena, indexes, access, direct)?;
                }
            }
            PlanDerivedExpression::RowExpression { expression }
            | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
                collect_row(*expression, arena, indexes, access, direct)?;
            }
            PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
            | PlanDerivedExpression::BoolNot { .. }
            | PlanDerivedExpression::NumberCompareConst { .. }
            | PlanDerivedExpression::ValueCompare { .. } => {}
        }
        }
        Ok(())
    }

    let mut access = BTreeSet::new();
    let mut direct = BTreeSet::new();
    if let PlanOpKind::DerivedValue {
        expression: Some(expression),
        ..
    } = &op.kind
    {
        collect_derived(expression, arena, indexes, &mut access, &mut direct)?;
    }
    Ok((access, direct))
}

fn distributed_import_contracts(
    plan: &MachinePlan,
) -> Result<BTreeMap<ImportId, DataTypePlan>, Error> {
    let Some(endpoint) = &plan.distributed_endpoint else {
        return Ok(BTreeMap::new());
    };
    if endpoint.endpoint.role != plan.program_role {
        return Err(Error::InvalidPlan(
            "distributed endpoint role does not match the machine role".to_owned(),
        ));
    }
    let contracts = endpoint
        .endpoint
        .value_imports
        .iter()
        .map(|import| (import.import_id, import.data_type.clone()))
        .chain(
            plan.producer_function_instances
                .iter()
                .flat_map(|instance| &instance.arguments)
                .map(|argument| (argument.import_id, argument.data_type.clone())),
        )
        .collect::<Vec<_>>();
    let mut by_id = BTreeMap::new();
    for (import_id, data_type) in contracts {
        if by_id.insert(import_id, data_type).is_some() {
            return Err(Error::InvalidPlan(
                "distributed import is declared more than once".to_owned(),
            ));
        }
    }
    Ok(by_id)
}

impl Metadata {
    fn new(plan: &MachinePlan) -> Result<Self, Error> {
        let constants = plan
            .constants
            .iter()
            .map(|constant| Ok((constant.id, constant_value(&constant.value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let distributed_import_types = distributed_import_contracts(plan)?;
        let recoverable_distributed_imports = plan
            .distributed_endpoint
            .as_ref()
            .into_iter()
            .flat_map(|endpoint| {
                endpoint
                    .endpoint
                    .value_imports
                    .iter()
                    .map(|import| import.import_id)
            })
            .collect();
        let row_owned_call_results = plan
            .distributed_endpoint
            .as_ref()
            .into_iter()
            .flat_map(|endpoint| &endpoint.endpoint.remote_call_sites)
            .filter(|call| call.mode == DistributedCallMode::Current)
            .filter_map(|call| {
                call.result
                    .current_import_id()
                    .map(|import_id| (import_id, call.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let producer_function_instances = plan
            .producer_function_instances
            .iter()
            .cloned()
            .map(|instance| (instance.call_site_id, instance))
            .collect::<BTreeMap<_, _>>();
        if producer_function_instances.len() != plan.producer_function_instances.len() {
            return Err(Error::InvalidPlan(
                "producer function call-site IDs are not unique".to_owned(),
            ));
        }
        let field_labels = debug_labels(&plan.debug_map.fields, "field:")
            .into_iter()
            .map(|(id, label)| (FieldId(id), label))
            .collect::<BTreeMap<_, _>>();
        let mut row_field_owner = BTreeMap::new();
        let mut list_fields_by_name = BTreeMap::<(ListId, String), Vec<FieldId>>::new();
        let mut list_authority_fields = BTreeMap::<(ListId, String), FieldId>::new();
        let mut capture_fields = BTreeSet::new();
        let mut list_fields_by_exact_name = BTreeMap::<String, Vec<(ListId, FieldId)>>::new();
        let mut row_field_names = BTreeMap::<(ListId, FieldId), String>::new();
        let mut semantic_list_identities = BTreeMap::new();
        let mut semantic_row_field_identities = BTreeMap::new();
        for slot in &plan.storage_layout.list_slots {
            let persistence = plan
                .persistence
                .lists
                .iter()
                .find(|memory| memory.runtime_slot == slot.id);
            if let Some(memory) = persistence {
                semantic_list_identities.insert(
                    slot.list_id,
                    (*memory.memory_id.as_bytes(), memory.type_fingerprint),
                );
            }
            for row_field in &slot.row_fields {
                let field = row_field.field_id;
                if let Some(previous) = row_field_owner.insert(field, slot.list_id)
                    && previous != slot.list_id
                {
                    return Err(Error::InvalidPlan(format!(
                        "field {} belongs to lists {} and {}",
                        field.0, previous.0, slot.list_id.0
                    )));
                }
                if row_field.role.is_value() {
                    row_field_names.insert((slot.list_id, field), row_field.name.clone());
                    list_fields_by_name
                        .entry((slot.list_id, row_field.name.clone()))
                        .or_default()
                        .push(field);
                }
                if row_field.role.is_authority() {
                    if list_authority_fields
                        .insert((slot.list_id, row_field.name.clone()), field)
                        .is_some()
                    {
                        return Err(Error::InvalidPlan(format!(
                            "list {} declares authority field `{}` more than once",
                            slot.list_id.0, row_field.name
                        )));
                    }
                    row_field_names
                        .entry((slot.list_id, field))
                        .or_insert_with(|| row_field.name.clone());
                }
                if row_field.role == boon_plan::PlanListRowFieldRole::Capture {
                    capture_fields.insert(field);
                }
                if let Some(leaf) = persistence.and_then(|memory| {
                    memory
                        .row_fields
                        .iter()
                        .find(|leaf| leaf.runtime_field_id == Some(field))
                }) {
                    semantic_row_field_identities
                        .insert((slot.list_id, field), *leaf.leaf_id.as_bytes());
                    list_fields_by_exact_name
                        .entry(leaf.semantic_path.clone())
                        .or_default()
                        .push((slot.list_id, field));
                }
            }
        }
        for fields in list_fields_by_name.values_mut() {
            fields.sort();
            fields.dedup();
        }
        for fields in list_fields_by_exact_name.values_mut() {
            fields.sort();
            fields.dedup();
        }
        let state_labels = debug_labels(&plan.debug_map.state_slots, "state:")
            .into_iter()
            .map(|(id, label)| (StateId(id), label))
            .collect::<BTreeMap<_, _>>();
        let mut root_state_by_exact_name = BTreeMap::<String, Vec<StateId>>::new();
        let mut root_state_by_name = BTreeMap::<String, Vec<StateId>>::new();
        for slot in plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| !slot.indexed)
        {
            if let Some(label) = state_labels.get(&slot.state_id) {
                root_state_by_exact_name
                    .entry(label.clone())
                    .or_default()
                    .push(slot.state_id);
                for name in debug_name_variants(label) {
                    root_state_by_name
                        .entry(name)
                        .or_default()
                        .push(slot.state_id);
                }
            }
        }
        for states in root_state_by_exact_name.values_mut() {
            states.sort();
            states.dedup();
        }
        for states in root_state_by_name.values_mut() {
            states.sort();
            states.dedup();
        }
        let list_indexes = plan
            .list_indexes
            .iter()
            .map(|index| (index.id, index.clone()))
            .collect::<BTreeMap<_, _>>();
        if list_indexes.len() != plan.list_indexes.len() {
            return Err(Error::InvalidPlan(
                "typed list index plan repeats an identity".to_owned(),
            ));
        }
        let mut ordered_indexes_by_list = BTreeMap::<ListId, BTreeSet<PlanListIndexId>>::new();
        let mut ordered_indexes_by_row_field =
            BTreeMap::<(ListId, FieldId), BTreeSet<PlanListIndexId>>::new();
        for index in list_indexes.values() {
            ordered_indexes_by_list
                .entry(index.source_list)
                .or_default()
                .insert(index.id);
            for key in &index.keys {
                plan.row_expressions
                    .visit_list_fields(key.expression, &mut |list, field| {
                        if list == index.source_list {
                            ordered_indexes_by_row_field
                                .entry((list, field))
                                .or_default()
                                .insert(index.id);
                        }
                    })
                    .map_err(|error| Error::InvalidPlan(error.to_string()))?;
            }
        }
        let mut list_by_scope = BTreeMap::new();
        for slot in &plan.storage_layout.list_slots {
            if let Some(scope) = slot.scope_id
                && let Some(previous) = list_by_scope.insert(scope, slot.list_id)
                && previous != slot.list_id
            {
                return Err(Error::InvalidPlan(format!(
                    "scope {} owns lists {} and {}",
                    scope.0, previous.0, slot.list_id.0
                )));
            }
        }
        let mut indexed_state_field = BTreeMap::new();
        let mut indexed_state_owner = BTreeMap::new();
        let state_owners = plan
            .storage_layout
            .scalar_slots
            .iter()
            .map(|slot| (slot.state_id, slot.owner.clone()))
            .collect::<BTreeMap<_, _>>();
        for slot in plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| slot.indexed)
        {
            let scope = slot.scope_id.ok_or_else(|| {
                Error::InvalidPlan(format!("indexed state {} has no scope", slot.state_id.0))
            })?;
            let list = *list_by_scope.get(&scope).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed state {} scope {} has no list",
                    slot.state_id.0, scope.0
                ))
            })?;
            let field = slot.indexed_field_id.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed state {} has no exact FieldId in list {}",
                    slot.state_id.0, list.0
                ))
            })?;
            if row_field_owner.get(&field) != Some(&list) {
                return Err(Error::InvalidPlan(format!(
                    "indexed state {} field {} does not belong to list {}",
                    slot.state_id.0, field.0, list.0
                )));
            }
            indexed_state_field.insert(slot.state_id, field);
            indexed_state_owner.insert(slot.state_id, list);
        }

        let mut root_computations = BTreeMap::new();
        let mut list_computations = BTreeMap::new();
        let mut row_computations = BTreeMap::new();
        let mut source_derived_by_source = BTreeMap::<SourceId, BTreeSet<FieldId>>::new();
        let mut state_derived_by_state = BTreeMap::<StateId, BTreeSet<FieldId>>::new();
        let mut source_derived_lists_by_source = BTreeMap::<SourceId, BTreeSet<ListId>>::new();
        let mut state_derived_lists_by_state = BTreeMap::<StateId, BTreeSet<ListId>>::new();
        let mut updates_by_source = BTreeMap::<SourceId, Vec<Arc<PlanOp>>>::new();
        let mut updates_by_state = BTreeMap::<StateId, Vec<Arc<PlanOp>>>::new();
        let mut effect_updates_by_id = BTreeMap::<PlanOpId, Arc<PlanOp>>::new();
        let mut session_info_root_fields = BTreeSet::new();
        let mut session_info_row_fields = BTreeSet::new();
        let mut mutations = Vec::new();
        for op in plan.regions.iter().flat_map(|region| &region.ops) {
            match &op.kind {
                PlanOpKind::DerivedValue { expression, .. } => match op.output {
                    Some(ValueRef::Field(field)) if op.indexed => {
                        row_computations.insert(field, Arc::new(op.clone()));
                        if let Some(expression) = expression
                            && derived_expression_has_intrinsic(expression, &plan.row_expressions)?
                        {
                            session_info_row_fields.insert(field);
                        }
                    }
                    Some(ValueRef::Field(field)) => {
                        root_computations.insert(field, Arc::new(op.clone()));
                        if let Some(expression) = expression
                            && derived_expression_has_intrinsic(expression, &plan.row_expressions)?
                        {
                            session_info_root_fields.insert(field);
                        }
                        if let Some(expression) = expression {
                            for trigger in derived_expression_event_triggers(expression) {
                                match trigger {
                                    ValueRef::Source(source) => {
                                        source_derived_by_source
                                            .entry(source)
                                            .or_default()
                                            .insert(field);
                                    }
                                    ValueRef::State(state) => {
                                        state_derived_by_state
                                            .entry(state)
                                            .or_default()
                                            .insert(field);
                                    }
                                    _ => {
                                        return Err(Error::InvalidPlan(format!(
                                            "source-event transform field {} has a non-event arm trigger",
                                            field.0,
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    Some(ValueRef::List(list)) if !op.indexed => {
                        list_computations.insert(list, Arc::new(op.clone()));
                        if let Some(expression) = expression {
                            for trigger in derived_expression_event_triggers(expression) {
                                match trigger {
                                    ValueRef::Source(source) => {
                                        source_derived_lists_by_source
                                            .entry(source)
                                            .or_default()
                                            .insert(list);
                                    }
                                    ValueRef::State(state) => {
                                        state_derived_lists_by_state
                                            .entry(state)
                                            .or_default()
                                            .insert(list);
                                    }
                                    _ => {
                                        return Err(Error::InvalidPlan(format!(
                                            "source-event transform list {} has a non-event arm trigger",
                                            list.0,
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(Error::InvalidPlan(format!(
                            "derived op {} has no valid field or list output",
                            op.id.0
                        )));
                    }
                },
                PlanOpKind::ListProjection { .. } => match op.output {
                    Some(ValueRef::Field(field)) => {
                        root_computations.insert(field, Arc::new(op.clone()));
                    }
                    Some(ValueRef::List(list)) => {
                        list_computations.insert(list, Arc::new(op.clone()));
                    }
                    _ => {
                        return Err(Error::InvalidPlan(format!(
                            "list projection op {} has no field or list output",
                            op.id.0
                        )));
                    }
                },
                PlanOpKind::StateUpdate { trigger, .. } => {
                    let op = Arc::new(op.clone());
                    if update_branch_has_effect(&op)
                        && effect_updates_by_id
                            .insert(op.id, Arc::clone(&op))
                            .is_some()
                    {
                        return Err(Error::InvalidPlan(format!(
                            "effect update op {} repeats its plan identity",
                            op.id.0
                        )));
                    }
                    match trigger {
                        ValueRef::Source(source) => updates_by_source
                            .entry(*source)
                            .or_default()
                            .push(Arc::clone(&op)),
                        ValueRef::State(state) => updates_by_state
                            .entry(*state)
                            .or_default()
                            .push(Arc::clone(&op)),
                        _ => {
                            return Err(Error::InvalidPlan(format!(
                                "update op {} has a non-event trigger",
                                op.id.0
                            )));
                        }
                    }
                }
                PlanOpKind::ListMutation { .. } => mutations.push(Arc::new(op.clone())),
                PlanOpKind::SourceRoute | PlanOpKind::DependencyEdge => {}
            }
        }
        for ops in updates_by_source.values_mut() {
            sort_update_ops_by_dependencies(ops);
        }
        for ops in updates_by_state.values_mut() {
            sort_update_ops_by_dependencies(ops);
        }
        mutations.sort_by_key(|op| op.id);
        let mut mutations_by_source = BTreeMap::<SourceId, Vec<Arc<PlanOp>>>::new();
        let mut mutations_by_state = BTreeMap::<StateId, Vec<Arc<PlanOp>>>::new();
        for mutation in &mutations {
            let trigger = match &mutation.kind {
                PlanOpKind::ListMutation {
                    mutation: PlanListMutation::Append(append),
                } => &append.trigger,
                PlanOpKind::ListMutation {
                    mutation: PlanListMutation::Remove(remove),
                } => &remove.trigger,
                _ => {
                    return Err(Error::InvalidPlan(format!(
                        "mutation op {} has no exact append or remove descriptor",
                        mutation.id.0
                    )));
                }
            };
            let operations = match trigger {
                ValueRef::Source(source) => mutations_by_source.entry(*source).or_default(),
                ValueRef::State(state) => mutations_by_state.entry(*state).or_default(),
                trigger => {
                    return Err(Error::InvalidPlan(format!(
                        "mutation op {} has non-event trigger {trigger:?}",
                        mutation.id.0
                    )));
                }
            };
            if !operations
                .iter()
                .any(|operation| operation.id == mutation.id)
            {
                operations.push(Arc::clone(mutation));
            }
        }
        for operations in mutations_by_source.values_mut() {
            operations.sort_by_key(|operation| operation.id);
        }
        for operations in mutations_by_state.values_mut() {
            operations.sort_by_key(|operation| operation.id);
        }

        let published: BTreeSet<FieldId> = match &plan.demand.root_derived_outputs {
            RootOutputDemand::All => root_computations.keys().copied().collect(),
            RootOutputDemand::Selected(fields) => fields.iter().copied().collect(),
        };

        let mut root_field_by_exact_name = BTreeMap::<String, Vec<FieldId>>::new();
        let mut root_field_by_name = BTreeMap::<String, Vec<FieldId>>::new();
        for field in root_computations.keys() {
            if let Some(label) = field_labels.get(field) {
                root_field_by_exact_name
                    .entry(label.clone())
                    .or_default()
                    .push(*field);
                for name in debug_name_variants(label) {
                    root_field_by_name.entry(name).or_default().push(*field);
                }
            }
        }
        for fields in root_field_by_exact_name.values_mut() {
            fields.sort();
            fields.dedup();
        }
        for fields in root_field_by_name.values_mut() {
            fields.sort();
            fields.dedup();
        }

        let routes = plan
            .source_routes
            .iter()
            .map(|route| (route.source_id, route.clone()))
            .collect::<BTreeMap<_, _>>();
        if routes.len() != plan.source_routes.len() {
            return Err(Error::InvalidPlan("duplicate source id".to_owned()));
        }
        let mut durable_root_state_by_memory = BTreeMap::new();
        for memory in plan
            .persistence
            .memory
            .iter()
            .filter(|memory| memory.kind == boon_plan::MemoryKind::Scalar)
        {
            let state = plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot && !slot.indexed)
                .map(|slot| slot.state_id)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistent scalar {} has no root runtime slot",
                        memory.memory_id
                    ))
                })?;
            if durable_root_state_by_memory
                .insert(memory.memory_id, state)
                .is_some()
            {
                return Err(Error::InvalidPlan(format!(
                    "persistent scalar memory {} is declared more than once",
                    memory.memory_id
                )));
            }
        }
        let durable_root_states = durable_root_state_by_memory.values().copied().collect();
        let mut durable_row_fields = BTreeSet::new();
        let mut durable_lists = BTreeSet::new();
        let mut durable_list_by_memory = BTreeMap::new();
        for memory in &plan.persistence.lists {
            let slot = plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistent list {} has no runtime slot",
                        memory.memory_id
                    ))
                })?;
            if !list_computations.contains_key(&slot.list_id) {
                durable_lists.insert(slot.list_id);
            }
            let mut fields_by_leaf = BTreeMap::new();
            for field in &memory.row_fields {
                let field_id = field.runtime_field_id.ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistent list {} has a row field without runtime identity",
                        memory.memory_id
                    ))
                })?;
                durable_row_fields.insert((slot.list_id, field_id));
                if fields_by_leaf.insert(field.leaf_id, field_id).is_some() {
                    return Err(Error::InvalidPlan(format!(
                        "persistent list {} repeats row leaf {}",
                        memory.memory_id, field.leaf_id
                    )));
                }
            }
            if durable_list_by_memory
                .insert(
                    memory.memory_id,
                    DurableListRuntimeMetadata {
                        list_id: slot.list_id,
                        fields_by_leaf,
                    },
                )
                .is_some()
            {
                return Err(Error::InvalidPlan(format!(
                    "persistent list memory {} is declared more than once",
                    memory.memory_id
                )));
            }
        }
        let indexed_state_fields = indexed_state_field
            .values()
            .copied()
            .collect::<BTreeSet<_>>();
        let deferred_authority_aliases = list_authority_fields
            .iter()
            .filter_map(|((list, name), authority)| {
                let candidates = list_fields_by_name
                    .get(&(*list, name.clone()))?
                    .iter()
                    .copied()
                    .filter(|field| {
                        field != authority
                            && (row_computations.contains_key(field)
                                || indexed_state_fields.contains(field))
                    })
                    .collect::<Vec<_>>();
                let [value] = candidates.as_slice() else {
                    return None;
                };
                Some(((*list, *authority), *value))
            })
            .collect::<BTreeMap<_, _>>();
        let deferred_value_aliases = deferred_authority_aliases
            .iter()
            .map(|((list, authority), value)| ((*list, *value), *authority))
            .collect::<BTreeMap<_, _>>();
        for ((list, authority), value) in &deferred_authority_aliases {
            if let Some(indexes) = ordered_indexes_by_row_field
                .get(&(*list, *authority))
                .cloned()
            {
                ordered_indexes_by_row_field
                    .entry((*list, *value))
                    .or_default()
                    .extend(indexes);
            }
            if let Some(indexes) = ordered_indexes_by_row_field.get(&(*list, *value)).cloned() {
                ordered_indexes_by_row_field
                    .entry((*list, *authority))
                    .or_default()
                    .extend(indexes);
            }
        }
        let mut currentness_ops = BTreeMap::new();
        for op in root_computations
            .values()
            .chain(list_computations.values())
            .chain(row_computations.values())
        {
            if currentness_ops.insert(op.id, Arc::clone(op)).is_some() {
                return Err(Error::InvalidPlan(format!(
                    "currentness op {} is registered more than once",
                    op.id.0
                )));
            }
        }
        let derived_expression_count = currentness_ops
            .values()
            .filter_map(|op| match &op.kind {
                PlanOpKind::DerivedValue {
                    expression: Some(expression),
                    ..
                } => Some(derived_expression_node_count(expression)),
                _ => None,
            })
            .fold(0usize, usize::saturating_add);
        let mut metadata = Self {
            constants,
            field_labels,
            distributed_import_types,
            recoverable_distributed_imports,
            row_owned_call_results,
            producer_function_instances,
            root_computations,
            list_computations,
            row_computations,
            currentness_ops,
            derived_expression_count,
            row_field_owner,
            indexed_state_field,
            indexed_state_owner,
            state_owners,
            list_by_scope,
            list_fields_by_name,
            list_authority_fields,
            deferred_authority_aliases,
            deferred_value_aliases,
            capture_fields,
            list_fields_by_exact_name,
            row_field_names,
            semantic_list_identities,
            semantic_row_field_identities,
            list_indexes,
            ordered_indexes_by_list,
            ordered_indexes_by_row_field,
            root_field_by_exact_name,
            root_field_by_name,
            root_state_by_exact_name,
            root_state_by_name,
            routes,
            updates_by_source,
            updates_by_state,
            effect_updates_by_id,
            mutations_by_source,
            mutations_by_state,
            source_derived_by_source,
            state_derived_by_state,
            source_derived_lists_by_source,
            state_derived_lists_by_state,
            durable_root_states,
            durable_row_fields,
            durable_lists,
            durable_root_state_by_memory,
            durable_list_by_memory,
            session_info_root_fields,
            session_info_row_fields,
            published,
            dependencies: Dependencies::default(),
        };
        metadata.dependencies = metadata.build_dependencies(&plan.row_expressions)?;
        Ok(metadata)
    }

    fn build_dependencies(&self, arena: &PlanRowExpressionArena) -> Result<Dependencies, Error> {
        let mut dependencies = Dependencies::default();
        for (output, op) in &self.root_computations {
            if source_event_transform_op(op) {
                continue;
            }
            let (typed_access_lists, direct_list_inputs) =
                typed_access_and_direct_list_inputs(op, arena, &self.list_indexes)?;
            for input in &op.inputs {
                match input {
                    ValueRef::State(state) if !self.indexed_state_owner.contains_key(state) => {
                        dependencies
                            .root_by_state
                            .entry(*state)
                            .or_default()
                            .insert(*output);
                    }
                    ValueRef::Field(field) if !self.row_field_owner.contains_key(field) => {
                        dependencies
                            .root_by_field
                            .entry(*field)
                            .or_default()
                            .insert(*output);
                    }
                    ValueRef::List(list)
                        if !typed_access_lists.contains(list)
                            || direct_list_inputs.contains(list) =>
                    {
                        dependencies
                            .root_by_list
                            .entry(*list)
                            .or_default()
                            .insert(*output);
                    }
                    _ => {}
                }
            }
        }
        for (output, op) in &self.list_computations {
            let (typed_access_lists, direct_list_inputs) =
                typed_access_and_direct_list_inputs(op, arena, &self.list_indexes)?;
            for input in &op.inputs {
                match input {
                    ValueRef::State(state) if !self.indexed_state_owner.contains_key(state) => {
                        dependencies
                            .list_by_state
                            .entry(*state)
                            .or_default()
                            .insert(*output);
                    }
                    ValueRef::Field(field) if !self.row_field_owner.contains_key(field) => {
                        dependencies
                            .list_by_field
                            .entry(*field)
                            .or_default()
                            .insert(*output);
                    }
                    ValueRef::List(list)
                        if list != output
                            && (!typed_access_lists.contains(list)
                                || direct_list_inputs.contains(list)) =>
                    {
                        dependencies
                            .list_by_list
                            .entry(*list)
                            .or_default()
                            .insert(*output);
                    }
                    _ => {}
                }
            }
        }
        for (output, op) in &self.row_computations {
            let Some(owner) = self.row_field_owner.get(output).copied() else {
                continue;
            };
            for input in &op.inputs {
                match input {
                    ValueRef::State(state) => {
                        if self.indexed_state_owner.get(state) == Some(&owner) {
                            if let Some(field) = self.indexed_state_field.get(state) {
                                dependencies
                                    .row_by_field
                                    .entry((owner, *field))
                                    .or_default()
                                    .insert(*output);
                            }
                        } else if !self.indexed_state_owner.contains_key(state) {
                            dependencies
                                .row_by_root_state
                                .entry(*state)
                                .or_default()
                                .insert((owner, *output));
                        }
                    }
                    ValueRef::Field(field) => {
                        if self.row_field_owner.get(field) == Some(&owner) {
                            dependencies
                                .row_by_field
                                .entry((owner, *field))
                                .or_default()
                                .insert(*output);
                        } else if !self.row_field_owner.contains_key(field) {
                            dependencies
                                .row_by_root_field
                                .entry(*field)
                                .or_default()
                                .insert((owner, *output));
                        }
                    }
                    // List/get and List/find install precise runtime dependencies. A
                    // List/ref installs a broad runtime dependency when evaluated.
                    ValueRef::List(_) => {}
                    _ => {}
                }
            }
        }
        Ok(dependencies)
    }

    fn list_authority_field(&self, list: ListId, name: &str) -> Result<FieldId, Error> {
        self.list_authority_fields
            .get(&(list, name.to_owned()))
            .copied()
            .ok_or_else(|| {
                Error::InvalidPlan(format!("list {} has no authority field `{name}`", list.0))
            })
    }

    fn exact_list_field(&self, name: &str) -> Result<Option<(ListId, FieldId)>, Error> {
        let Some(fields) = self.list_fields_by_exact_name.get(name) else {
            return Ok(None);
        };
        match fields.as_slice() {
            [field] => Ok(Some(*field)),
            fields => Err(Error::InvalidPlan(format!(
                "list field semantic path `{name}` names multiple owners {fields:?}"
            ))),
        }
    }
}

fn output_list_field(
    fields: &[OutputListFieldRef],
    list: ListId,
    name: &str,
) -> Result<FieldId, Error> {
    fields
        .iter()
        .find(|field| field.list_id == list && field.name == name)
        .map(|field| field.field_id)
        .ok_or_else(|| {
            Error::InvalidPlan(format!(
                "host value row field `{name}` on ListId {} has no compiler-owned FieldId",
                list.0
            ))
        })
}

fn debug_name_variants(label: &str) -> Vec<String> {
    let parts = label.split('.').collect::<Vec<_>>();
    (0..parts.len())
        .map(|start| parts[start..].join("."))
        .collect()
}

fn unique_root_name<T: Copy + std::fmt::Debug>(
    names: &BTreeMap<String, Vec<T>>,
    name: &str,
    kind: &str,
) -> Result<Option<T>, Error> {
    match names.get(name).map(Vec::as_slice) {
        Some([value]) => Ok(Some(*value)),
        Some(values) => Err(Error::InvalidPlan(format!(
            "root {kind} name `{name}` is ambiguous across {values:?}"
        ))),
        None => Ok(None),
    }
}

fn debug_labels(entries: &[boon_plan::DebugEntry], prefix: &str) -> BTreeMap<usize, String> {
    entries
        .iter()
        .filter_map(|entry| {
            entry
                .id
                .strip_prefix(prefix)?
                .parse::<usize>()
                .ok()
                .map(|id| (id, entry.label.clone()))
        })
        .collect()
}

fn debug_label<'a>(
    entries: &'a [boon_plan::DebugEntry],
    prefix: &str,
    id: usize,
) -> Option<&'a str> {
    let id = format!("{prefix}{id}");
    entries
        .iter()
        .find_map(|entry| (entry.id == id).then_some(entry.label.as_str()))
}

fn local_name(label: &str) -> &str {
    label.rsplit('.').next().unwrap_or(label)
}

fn constant_value(value: &PlanConstantValue) -> Result<Value, Error> {
    match value {
        PlanConstantValue::Text { value } | PlanConstantValue::Enum { value } => {
            Ok(Value::Text(value.clone()))
        }
        PlanConstantValue::Data { value } => Ok(runtime_value_from_data(value)),
        PlanConstantValue::Number { value } => Ok(Value::Number(*value)),
        PlanConstantValue::Bool { value } => Ok(Value::Bool(*value)),
        PlanConstantValue::Bytes {
            byte_len,
            inline_bytes,
            ..
        } => {
            let bytes = inline_bytes.clone().ok_or_else(|| {
                Error::InvalidPlan("BYTES constant has no inline payload".to_owned())
            })?;
            if bytes.len() as u64 != *byte_len {
                return Err(Error::InvalidPlan(format!(
                    "BYTES constant length {} does not match byte_len {byte_len}",
                    bytes.len()
                )));
            }
            Ok(Value::Bytes(Bytes::from(bytes)))
        }
    }
}

fn runtime_value_from_data(value: &boon_data::Value) -> Value {
    match value {
        boon_data::Value::Null => Value::Null,
        boon_data::Value::Bool(value) => Value::Bool(*value),
        boon_data::Value::Number(value) => Value::Number(*value),
        boon_data::Value::Text(value) => Value::Text(value.clone()),
        boon_data::Value::Bytes(value) => Value::Bytes(value.clone()),
        boon_data::Value::List(values) => {
            Value::List(values.iter().map(runtime_value_from_data).collect())
        }
        boon_data::Value::Record(fields) => Value::Record(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), runtime_value_from_data(value)))
                .collect(),
        ),
        boon_data::Value::Variant { tag, fields } => {
            if fields.is_empty() {
                Value::Text(tag.clone())
            } else {
                Value::Record(
                    std::iter::once(("$tag".to_owned(), Value::Text(tag.clone())))
                        .chain(
                            fields.iter().map(|(name, value)| {
                                (name.clone(), runtime_value_from_data(value))
                            }),
                        )
                        .collect(),
                )
            }
        }
        boon_data::Value::Error { code, fields } => {
            if fields.is_empty() {
                Value::Error { code: code.clone() }
            } else {
                Value::Record(
                    std::iter::once(("error".to_owned(), Value::Text(code.clone())))
                        .chain(
                            fields.iter().map(|(name, value)| {
                                (name.clone(), runtime_value_from_data(value))
                            }),
                        )
                        .collect(),
                )
            }
        }
    }
}

fn runtime_value_to_data(value: &Value) -> Result<boon_data::Value, Error> {
    Ok(match value {
        Value::Null => boon_data::Value::Null,
        Value::Bool(value) => boon_data::Value::Bool(*value),
        Value::Number(value) => boon_data::Value::Number(*value),
        Value::Text(value) => boon_data::Value::Text(value.clone()),
        Value::Bytes(value) => boon_data::Value::Bytes(value.clone()),
        Value::List(values) => boon_data::Value::List(
            values
                .iter()
                .map(runtime_value_to_data)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Record(fields) => boon_data::Value::Record(
            fields
                .iter()
                .map(|(name, value)| Ok((name.clone(), runtime_value_to_data(value)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()?,
        ),
        Value::Error { code } => boon_data::Value::Error {
            code: code.clone(),
            fields: BTreeMap::new(),
        },
        Value::MappedRow { .. } | Value::Row { .. } => {
            return Err(Error::Evaluation(
                "ordinary data boundaries cannot contain runtime row handles".to_owned(),
            ));
        }
        Value::HostBound { .. } => {
            return Err(Error::Evaluation(
                "host-bound values cannot cross an ordinary data boundary".to_owned(),
            ));
        }
    })
}

fn stable_list_fields(
    list: &boon_plan::ListMemoryPlan,
) -> Result<BTreeMap<FieldId, boon_plan::MemoryLeafId>, Error> {
    let mut fields = BTreeMap::new();
    for leaf in &list.row_fields {
        let field = leaf.runtime_field_id.ok_or_else(|| {
            Error::InvalidPlan(format!(
                "persistence list {} leaf {} has no runtime FieldId",
                list.memory_id, leaf.leaf_id
            ))
        })?;
        if fields.insert(field, leaf.leaf_id).is_some() {
            return Err(Error::InvalidPlan(format!(
                "persistence list {} repeats runtime field {}",
                list.memory_id, field.0
            )));
        }
    }
    Ok(fields)
}

fn stored_list(
    plan: &MachinePlan,
    memory: &boon_plan::ListMemoryPlan,
    authority: &ListAuthority,
    touched_only: bool,
) -> Result<boon_persistence::StoredList, Error> {
    let mut rows = authority
        .rows
        .iter()
        .filter(|row| !touched_only || !row.touched_fields.is_empty())
        .map(|row| stored_row(plan, memory, row, touched_only))
        .collect::<Result<Vec<_>, _>>()?;
    if !authority.touched {
        for row in &mut rows {
            row.source_order_token = 0;
        }
    }
    Ok(boon_persistence::StoredList {
        touched: authority.touched,
        revision: authority.revision,
        next_key: if authority.touched {
            authority.next_key
        } else {
            0
        },
        next_order_token: if authority.touched {
            authority.next_order_token
        } else {
            0
        },
        rows,
    })
}

fn stored_row(
    plan: &MachinePlan,
    memory: &boon_plan::ListMemoryPlan,
    row: &RowAuthority,
    touched_only: bool,
) -> Result<boon_persistence::StoredRow, Error> {
    let stable_fields = stable_list_fields(memory)?;
    let fields = row
        .fields
        .iter()
        .filter(|(field, _)| !touched_only || row.touched_fields.contains(field))
        .filter_map(|(field, value)| {
            stable_fields
                .get(field)
                .copied()
                .map(|stable| stored_value(value).map(|value| (stable, value)))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let touched_fields = row
        .touched_fields
        .iter()
        .filter_map(|field| stable_fields.get(field).copied())
        .collect::<BTreeSet<_>>();
    Ok(boon_persistence::StoredRow {
        key: row.id.key,
        generation: row.id.generation,
        source_order_token: row.source_order_token,
        owner: durable_owner_for_rows(plan, &row.owner_ancestors)?,
        materialization_origin: row
            .materialization_origin
            .as_ref()
            .map(|origin| durable_owner_for_rows(plan, origin))
            .transpose()?,
        fields,
        touched_fields,
    })
}

fn durable_owner_for_rows(
    plan: &MachinePlan,
    ancestors: &[OwnerInstanceRow],
) -> Result<boon_persistence::DurableOwner, Error> {
    let ancestors = ancestors
        .iter()
        .map(|row| {
            let memory = plan
                .persistence
                .lists
                .iter()
                .find(|memory| {
                    plan.storage_layout
                        .list_slots
                        .iter()
                        .any(|slot| slot.id == memory.runtime_slot && slot.list_id == row.list)
                })
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "list {} has no stable persistence identity",
                        row.list.0
                    ))
                })?;
            Ok(boon_persistence::DurableRowId {
                list_memory_id: memory.memory_id,
                row_key: row.key,
                row_generation: row.generation,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(boon_persistence::DurableOwner { ancestors })
}

pub(crate) fn stored_value(value: &Value) -> Result<boon_persistence::StoredValue, Error> {
    match value {
        Value::Null => Ok(boon_persistence::StoredValue::Null),
        Value::Bool(value) => Ok(boon_persistence::StoredValue::Bool(*value)),
        Value::Number(value) => Ok(boon_persistence::StoredValue::Number(*value)),
        Value::Text(value) => Ok(boon_persistence::StoredValue::Text(value.clone())),
        Value::Bytes(value) => Ok(boon_persistence::StoredValue::Bytes(value.clone())),
        Value::List(values) => values
            .iter()
            .map(stored_value)
            .collect::<Result<Vec<_>, _>>()
            .map(boon_persistence::StoredValue::List),
        Value::Record(fields) => {
            let mut stored = fields
                .iter()
                .filter(|(name, _)| name.as_str() != "$tag")
                .map(|(name, value)| Ok((name.clone(), stored_value(value)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()?;
            match fields.get("$tag") {
                Some(Value::Text(tag)) => Ok(boon_persistence::StoredValue::Variant {
                    tag: tag.clone(),
                    fields: std::mem::take(&mut stored),
                }),
                Some(_) => Err(Error::Evaluation(
                    "tagged runtime record has a non-text `$tag` field".to_owned(),
                )),
                None => Ok(boon_persistence::StoredValue::Record(stored)),
            }
        }
        Value::Error { code } => Ok(boon_persistence::StoredValue::Error {
            code: code.clone(),
            fields: BTreeMap::new(),
        }),
        Value::MappedRow { .. } | Value::Row { .. } => Err(Error::Evaluation(
            "row handles and derived mapped rows are not durable authority".to_owned(),
        )),
        Value::HostBound { .. } => Err(Error::Evaluation(
            "host-bound values are transient and cannot be persisted".to_owned(),
        )),
    }
}

fn durable_owner_value(owner: &boon_persistence::DurableOwner) -> boon_persistence::StoredValue {
    boon_persistence::StoredValue::List(
        owner
            .ancestors
            .iter()
            .map(|row| {
                boon_persistence::StoredValue::Record(BTreeMap::from([
                    (
                        "generation".to_owned(),
                        boon_persistence::StoredValue::Text(row.row_generation.to_string()),
                    ),
                    (
                        "key".to_owned(),
                        boon_persistence::StoredValue::Text(row.row_key.to_string()),
                    ),
                    (
                        "memory".to_owned(),
                        boon_persistence::StoredValue::Bytes(
                            row.list_memory_id.as_bytes().to_vec().into(),
                        ),
                    ),
                ]))
            })
            .collect(),
    )
}

fn stored_value_for_data_type(
    value: &Value,
    data_type: &boon_plan::DataTypePlan,
) -> Result<boon_persistence::StoredValue, Error> {
    if let (Value::Text(tag), boon_plan::DataTypePlan::Variant { variants }) = (value, data_type) {
        let variant = variants
            .iter()
            .find(|variant| variant.tag == *tag)
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "variant tag `{tag}` is not declared by the durable value schema"
                ))
            })?;
        if !variant.fields.is_empty() {
            return Err(Error::Evaluation(format!(
                "structured variant `{tag}` requires named fields"
            )));
        }
        return Ok(boon_persistence::StoredValue::Variant {
            tag: tag.clone(),
            fields: BTreeMap::new(),
        });
    }
    stored_value(value)
}

fn validate_value_for_data_type(
    value: &Value,
    data_type: &boon_plan::DataTypePlan,
    path: &str,
) -> Result<(), Error> {
    use boon_plan::DataTypePlan;

    if let Value::HostBound { visible, .. } = value {
        return validate_value_for_data_type(visible, data_type, path);
    }

    match (value, data_type) {
        (Value::Null, DataTypePlan::Null)
        | (Value::Bool(_), DataTypePlan::Bool)
        | (Value::Number(_), DataTypePlan::Number)
        | (Value::Text(_), DataTypePlan::Text) => Ok(()),
        (Value::Bytes(value), DataTypePlan::Bytes { fixed_len })
            if fixed_len
                .is_none_or(|expected| u64::try_from(value.len()).ok() == Some(expected)) =>
        {
            Ok(())
        }
        (Value::List(values), DataTypePlan::List { item }) => {
            for (index, value) in values.iter().enumerate() {
                validate_value_for_data_type(value, item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        (Value::Record(values), DataTypePlan::Record { fields, open }) => {
            validate_record_for_data_type(values, fields, *open, path)
        }
        (Value::Text(tag), DataTypePlan::Variant { variants }) => {
            let variant = variants
                .iter()
                .find(|variant| variant.tag == *tag)
                .ok_or_else(|| {
                    Error::Evaluation(format!("{path} has undeclared variant `{tag}`"))
                })?;
            if variant.fields.is_empty() {
                Ok(())
            } else {
                Err(Error::Evaluation(format!(
                    "{path} variant `{tag}` requires named fields"
                )))
            }
        }
        (Value::Record(values), DataTypePlan::Variant { variants }) => {
            let Some(Value::Text(tag)) = values.get("$tag") else {
                return Err(Error::Evaluation(format!(
                    "{path} structured variant has no text `$tag`"
                )));
            };
            let variant = variants
                .iter()
                .find(|variant| variant.tag == *tag)
                .ok_or_else(|| {
                    Error::Evaluation(format!("{path} has undeclared variant `{tag}`"))
                })?;
            let fields = values
                .iter()
                .filter(|(name, _)| name.as_str() != "$tag")
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            validate_record_for_data_type(&fields, &variant.fields, variant.open, path)
        }
        (Value::Error { .. }, DataTypePlan::Error { fields, .. }) if fields.is_empty() => Ok(()),
        (_, DataTypePlan::Unknown) => Err(Error::InvalidPlan(format!(
            "{path} has an unknown data type"
        ))),
        _ => Err(Error::Evaluation(format!(
            "{path} has runtime kind {} but its declared data type is {data_type:?}",
            runtime_value_kind(value)
        ))),
    }
}

fn runtime_value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "Null",
        Value::Bool(_) => "Bool",
        Value::Number(_) => "Number",
        Value::Text(_) => "Text",
        Value::Bytes(_) => "Bytes",
        Value::List(_) => "List",
        Value::Record(_) => "Record",
        Value::MappedRow { .. } => "MappedRow",
        Value::Row { .. } => "Row",
        Value::Error { .. } => "Error",
        Value::HostBound { visible, .. } => runtime_value_kind(visible),
    }
}

fn validate_distributed_boundary_value(
    value: &Value,
    data_type: &DataTypePlan,
    path: &str,
) -> Result<(), Error> {
    if value.contains_host_binding() {
        return Err(Error::Evaluation(format!(
            "{path} contains a process-local host binding"
        )));
    }
    // Cross-role reads preserve Boon's normal value type while allowing a
    // generated transport/currentness error to flow through existing error
    // propagation. Applications do not serialize or inspect transport frames.
    if matches!(value, Value::Error { .. }) {
        return Ok(());
    }
    validate_value_for_data_type(value, data_type, path)
}

fn validate_record_for_data_type(
    values: &BTreeMap<String, Value>,
    fields: &[boon_plan::DataTypeFieldPlan],
    open: bool,
    path: &str,
) -> Result<(), Error> {
    for field in fields {
        let value = values.get(&field.name).ok_or_else(|| {
            Error::Evaluation(format!("{path} is missing field `{}`", field.name))
        })?;
        validate_value_for_data_type(value, &field.data_type, &format!("{path}.{}", field.name))?;
    }
    if !open
        && let Some(name) = values
            .keys()
            .find(|name| !fields.iter().any(|field| field.name == **name))
    {
        return Err(Error::Evaluation(format!(
            "{path} contains undeclared field `{name}`"
        )));
    }
    Ok(())
}

pub(crate) fn runtime_value(value: boon_persistence::StoredValue) -> Result<Value, Error> {
    match value {
        boon_persistence::StoredValue::Null => Ok(Value::Null),
        boon_persistence::StoredValue::Bool(value) => Ok(Value::Bool(value)),
        boon_persistence::StoredValue::Number(value) => Ok(Value::Number(value)),
        boon_persistence::StoredValue::Text(value) => Ok(Value::Text(value)),
        boon_persistence::StoredValue::Bytes(value) => Ok(Value::Bytes(value)),
        boon_persistence::StoredValue::List(values) => values
            .into_iter()
            .map(runtime_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        boon_persistence::StoredValue::Record(fields) => fields
            .into_iter()
            .map(|(name, value)| Ok((name, runtime_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        boon_persistence::StoredValue::Variant { tag, fields } if fields.is_empty() => {
            Ok(Value::Text(tag))
        }
        boon_persistence::StoredValue::Variant { tag, mut fields } => {
            if fields
                .insert("$tag".to_owned(), boon_persistence::StoredValue::Text(tag))
                .is_some()
            {
                return Err(Error::Evaluation(
                    "stored variant contains reserved `$tag` field".to_owned(),
                ));
            }
            runtime_value(boon_persistence::StoredValue::Record(fields))
        }
        boon_persistence::StoredValue::Error { code, fields } => {
            if fields.is_empty() {
                Ok(Value::Error { code })
            } else {
                Err(Error::Evaluation(
                    "runtime cannot restore structured error authority".to_owned(),
                ))
            }
        }
    }
}

#[derive(Clone)]
enum AuthorityUndo {
    RootState {
        state: StateId,
        value: Option<Value>,
        touched: bool,
    },
    RowField {
        row: RowId,
        field: FieldId,
        value: Option<Value>,
        touched_field: bool,
        touched_list: bool,
    },
    AppendRow {
        row: RowId,
        previous_next_key: u64,
        source_order: SourceOrderUndo,
        touched_list: bool,
    },
    RemoveRow {
        row: RowId,
        value: Row,
        source_order: SourceOrderUndo,
        previous_next_key: u64,
        touched_list: bool,
        touched_fields: BTreeSet<FieldId>,
    },
    ReorderRows {
        list: ListId,
        undo: SourceOrderUndo,
        owner_partition: Option<(Vec<OwnerInstanceRow>, Vec<RowId>)>,
    },
}

#[derive(Clone)]
struct DistributedContextUndo {
    session_context: SessionContext,
    imports: BTreeMap<ImportId, (Option<Value>, Option<u64>)>,
    row_owned_call_results:
        BTreeMap<(ImportId, DistributedCallInstanceId), Option<DistributedCurrentCallResult>>,
}

#[derive(Clone, Copy)]
enum DistributedContextInstall {
    Patch,
    Replace,
}

#[derive(Clone, Copy)]
enum DistributedContextTurn {
    Authority,
    Execution,
}

#[derive(Clone, Default)]
struct Work {
    emit: bool,
    deltas: Vec<Delta>,
    authority_deltas: Vec<AuthorityDelta>,
    outbox_changes: Vec<boon_persistence::DurableOutboxChange>,
    transient_effects: Vec<TransientEffectInvocation>,
    cancelled_transient_effects: Vec<TransientEffectCallId>,
    transient_effect_credit_grants: Vec<TransientEffectCreditGrant>,
    distributed_invocations: Vec<DistributedInvocation>,
    committed_transient_effects: Vec<TransientEffectCallId>,
    completed_transient_effects: Vec<(TransientEffectCallId, PendingTransientEffect)>,
    updated_transient_effects: Vec<(TransientEffectCallId, PendingTransientEffect)>,
    metrics: TurnMetrics,
    dirty_states: HashSet<StateId>,
    active_trigger: Option<ActiveTrigger>,
    active_state_routes: HashSet<(StateId, OwnerInstanceId)>,
    dirty_consumers: HashSet<Consumer>,
    pending_effect_reconciliations: BTreeSet<EffectConsumer>,
    effect_reconciliation_sequence: Option<u64>,
    changed_rows: HashSet<RowId>,
    suppress_row_deltas: HashSet<RowId>,
    recomputed_targets: HashSet<ValueTarget>,
    pending_list_mutations: Vec<PendingListMutation>,
    authority_undo: Vec<AuthorityUndo>,
    undo_root_states: HashSet<StateId>,
    undo_row_fields: HashSet<(RowId, FieldId)>,
    list_revision_undo: BTreeMap<ListId, u64>,
    distributed_context_undo: Option<DistributedContextUndo>,
    effect_activation_undo: BTreeMap<EffectConsumer, Option<EffectActivation>>,
    detached_producer_leases: BTreeMap<ProducerLeaseKey, ProducerLeaseState>,
    active_value_list_authorities: Vec<PlanValueListAuthority>,
    pending_settle: bool,
    previous_last_sequence: Option<u64>,
    previous_turn_sequence: u64,
    work_limit: Option<u64>,
    work_units: u64,
    enforce_work_limit: bool,
}

impl Work {
    fn with_limit(work_limit: Option<u64>) -> Self {
        Self {
            work_limit,
            enforce_work_limit: true,
            ..Self::default()
        }
    }

    fn begin_turn(&mut self, last_sequence: Option<u64>, turn_sequence: u64) {
        self.emit = true;
        self.deltas.clear();
        self.authority_deltas.clear();
        self.outbox_changes.clear();
        self.transient_effects.clear();
        self.cancelled_transient_effects.clear();
        self.transient_effect_credit_grants.clear();
        self.distributed_invocations.clear();
        self.committed_transient_effects.clear();
        self.completed_transient_effects.clear();
        self.updated_transient_effects.clear();
        self.metrics = TurnMetrics::default();
        self.dirty_states.clear();
        self.active_trigger = None;
        self.active_state_routes.clear();
        self.dirty_consumers.clear();
        self.pending_effect_reconciliations.clear();
        self.effect_reconciliation_sequence = None;
        self.changed_rows.clear();
        self.suppress_row_deltas.clear();
        self.recomputed_targets.clear();
        self.pending_list_mutations.clear();
        self.distributed_invocations.clear();
        self.authority_undo.clear();
        self.undo_root_states.clear();
        self.undo_row_fields.clear();
        self.list_revision_undo.clear();
        self.distributed_context_undo = None;
        self.effect_activation_undo.clear();
        self.detached_producer_leases.clear();
        self.active_value_list_authorities.clear();
        self.pending_settle = false;
        self.previous_last_sequence = last_sequence;
        self.previous_turn_sequence = turn_sequence;
        self.work_units = 0;
        self.enforce_work_limit = true;
    }

    fn consume(&mut self, units: u64) -> Result<(), Error> {
        let attempted = self.work_units.saturating_add(units);
        if self.enforce_work_limit
            && self
                .work_limit
                .is_some_and(|work_limit| attempted > work_limit)
        {
            return Err(Error::WorkBudgetExceeded {
                limit: self.work_limit.unwrap_or_default(),
                attempted,
            });
        }
        self.work_units = attempted;
        Ok(())
    }

    fn allow_rollback(&mut self) {
        // Authority rollback must complete after a bounded evaluation aborts.
        // Static plan and storage limits still bound this recovery path.
        self.enforce_work_limit = false;
    }

    fn settle(&mut self) {
        self.authority_undo.clear();
        self.undo_root_states.clear();
        self.undo_row_fields.clear();
        self.list_revision_undo.clear();
        self.committed_transient_effects.clear();
        self.completed_transient_effects.clear();
        self.updated_transient_effects.clear();
        self.pending_list_mutations.clear();
        self.distributed_context_undo = None;
        self.effect_activation_undo.clear();
        self.detached_producer_leases.clear();
        self.active_value_list_authorities.clear();
        self.pending_settle = false;
    }

    fn finish_metrics(&mut self) {
        self.metrics.dirty_state_count = self.dirty_states.len();
        self.metrics.dirty_field_count = self.dirty_consumers.len();
        self.metrics.changed_row_count = self.changed_rows.len();
        self.metrics.work_unit_count = self.work_units;
        self.metrics.recomputed_targets.clear();
        self.metrics
            .recomputed_targets
            .extend(self.recomputed_targets.iter().copied());
        self.metrics.recomputed_targets.sort_unstable();
    }
}

fn record_ordered_index_fanout(work: &mut Work, fanout: usize) {
    work.metrics.ordered_index_affected_fanout_max = work
        .metrics
        .ordered_index_affected_fanout_max
        .max(fanout.try_into().unwrap_or(u64::MAX));
}

#[derive(Default)]
struct PreparedOrderedIndexDirty {
    rows: BTreeMap<PlanListIndexId, BTreeSet<RowId>>,
    fanout: usize,
}

impl PreparedOrderedIndexDirty {
    fn charge(&self, work: &mut Work) -> Result<(), Error> {
        work.consume(self.fanout.try_into().unwrap_or(u64::MAX))
    }
}

fn record_source_order_maintenance(work: &mut Work, maintenance: &SourceOrderMaintenance) {
    let location_updates = maintenance
        .storage
        .row_location_update_count
        .try_into()
        .unwrap_or(u64::MAX);
    let relabeled_rows = maintenance
        .relabeled_rows
        .len()
        .try_into()
        .unwrap_or(u64::MAX);
    work.metrics.source_order_location_update_count = work
        .metrics
        .source_order_location_update_count
        .saturating_add(location_updates);
    work.metrics.source_order_location_update_max = work
        .metrics
        .source_order_location_update_max
        .max(location_updates);
    work.metrics.source_order_block_split_count =
        work.metrics.source_order_block_split_count.saturating_add(
            maintenance
                .storage
                .block_split_count
                .try_into()
                .unwrap_or(u64::MAX),
        );
    work.metrics.source_order_block_merge_count =
        work.metrics.source_order_block_merge_count.saturating_add(
            maintenance
                .storage
                .block_merge_count
                .try_into()
                .unwrap_or(u64::MAX),
        );
    work.metrics.source_order_tree_visit_count =
        work.metrics.source_order_tree_visit_count.saturating_add(
            maintenance
                .storage
                .tree_visit_count
                .try_into()
                .unwrap_or(u64::MAX),
        );
    work.metrics.source_order_tree_visit_max = work.metrics.source_order_tree_visit_max.max(
        maintenance
            .storage
            .tree_visit_max
            .try_into()
            .unwrap_or(u64::MAX),
    );
    work.metrics.source_order_relabel_operation_count = work
        .metrics
        .source_order_relabel_operation_count
        .saturating_add(
            maintenance
                .relabel_operation_count
                .try_into()
                .unwrap_or(u64::MAX),
        );
    work.metrics.source_order_relabel_row_count = work
        .metrics
        .source_order_relabel_row_count
        .saturating_add(relabeled_rows);
    work.metrics.source_order_relabel_window_max =
        work.metrics.source_order_relabel_window_max.max(
            maintenance
                .relabel_window_max
                .try_into()
                .unwrap_or(u64::MAX),
        );
}

fn charge_source_order_maintenance(
    work: &mut Work,
    maintenance: &SourceOrderMaintenance,
) -> Result<(), Error> {
    work.consume(maintenance.work_units())
}

#[derive(Clone, Debug)]
struct PendingTransientEffect {
    invocation_id: EffectInvocationId,
    effect_id: boon_plan::EffectId,
    owner: OwnerInstanceId,
    target: Option<RowId>,
    execution_scope: u64,
    delivery: boon_plan::EffectDeliveryCardinality,
    semantic: TransientEffectSemanticValidator,
    next_result_sequence: u64,
    available_credits: u32,
}

fn authority_delta_is_producer_local(
    delta: &AuthorityDelta,
    ownership: &boon_plan::ProducerFunctionOwnershipPlan,
) -> bool {
    match delta {
        AuthorityDelta::SetRoot { state, .. } => ownership.states.contains(state),
        AuthorityDelta::SetRowField { row, field, .. } => {
            ownership.lists.contains(&row.list) || ownership.fields.contains(field)
        }
        AuthorityDelta::ReplaceList { list_id, .. } => ownership.lists.contains(list_id),
        AuthorityDelta::InsertRow { row, .. } => ownership.lists.contains(&row.id.list),
        AuthorityDelta::RemoveRow { row, .. } => ownership.lists.contains(&row.list),
    }
}

fn instantiate_plan_owner(
    plan: &PlanOwner,
    trigger: &ActiveTrigger,
) -> Result<OwnerInstanceId, Error> {
    if plan.static_owner.is_root() {
        if !plan.ancestors.is_empty() {
            return Err(Error::InvalidPlan(
                "root static owner declares repeated ancestors".to_owned(),
            ));
        }
        return Ok(OwnerInstanceId::root());
    }
    if plan.ancestors.is_empty() {
        return OwnerInstanceId::new(plan.static_owner, Vec::new())
            .map_err(|detail| Error::InvalidPlan(detail.to_owned()));
    }
    if trigger.owner_plan.ancestors.len() < plan.ancestors.len()
        || trigger.owner.ancestors.len() < plan.ancestors.len()
    {
        return Err(Error::InvalidPlan(format!(
            "owner {} requires {} repeated ancestors, trigger owner {} provides {}",
            plan.static_owner.0,
            plan.ancestors.len(),
            trigger.owner.static_owner.0,
            trigger.owner.ancestors.len()
        )));
    }
    let mut ancestors = Vec::with_capacity(plan.ancestors.len());
    for (index, expected) in plan.ancestors.iter().enumerate() {
        let trigger_shape = &trigger.owner_plan.ancestors[index];
        let row = trigger.owner.ancestors[index];
        if trigger_shape != expected || row.list != expected.list {
            return Err(Error::InvalidPlan(format!(
                "owner {} ancestor depth {index} does not match trigger owner {}",
                plan.static_owner.0, trigger.owner.static_owner.0
            )));
        }
        ancestors.push(row);
    }
    OwnerInstanceId::new(plan.static_owner, ancestors)
        .map_err(|detail| Error::InvalidPlan(detail.to_owned()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum EvalValue {
    Value(Value),
    Row(RowId),
    List(Vec<EvalValue>),
    Record(BTreeMap<String, EvalValue>),
    MappedRow {
        id: RowId,
        fields: BTreeMap<String, EvalValue>,
        captures: BTreeMap<FieldId, EvalValue>,
    },
    OrderedList {
        items: Vec<OrderedEvalItem>,
        directions: Vec<EvalOrderDirection>,
    },
}

enum EvalMaterializationCollection {
    List {
        remaining: std::vec::IntoIter<EvalValue>,
        output: Vec<Value>,
    },
    Record {
        remaining: std::collections::btree_map::IntoIter<String, EvalValue>,
        output: BTreeMap<String, Value>,
        mapped_row: Option<RowId>,
    },
    OrderedList {
        remaining: std::vec::IntoIter<OrderedEvalItem>,
        output: Vec<Value>,
    },
}

enum EvalMaterializationSlot {
    Item,
    Field(String),
}

enum EvalMaterializationTask {
    Evaluate(EvalValue),
    Continue(EvalMaterializationCollection),
    Append {
        collection: EvalMaterializationCollection,
        slot: EvalMaterializationSlot,
    },
}

enum PageValueCollection {
    List {
        remaining: std::vec::IntoIter<Value>,
        output: Vec<Value>,
    },
    Record {
        remaining: std::collections::btree_map::IntoIter<String, Value>,
        output: BTreeMap<String, Value>,
    },
}

enum PageValueTask {
    Evaluate(Value),
    Continue(PageValueCollection),
    Append {
        collection: PageValueCollection,
        slot: EvalMaterializationSlot,
    },
}

const MAX_VALUE_MATERIALIZATION_CONTINUATIONS: usize = 8_192;

fn ensure_value_continuation_capacity<T>(
    tasks: &[T],
    additional: usize,
    context: &str,
) -> Result<(), Error> {
    if tasks.len().saturating_add(additional) > MAX_VALUE_MATERIALIZATION_CONTINUATIONS {
        return Err(Error::Evaluation(format!(
            "{context} exceeded its checked continuation bound of {}",
            MAX_VALUE_MATERIALIZATION_CONTINUATIONS
        )));
    }
    Ok(())
}

enum PageTraversalFailure {
    WorkLimit,
    Runtime(Error),
    Access(AccessError),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EvaluatedListAccessSelection {
    OrderedStart,
    KeyPrefix {
        values: Vec<StructuralValue>,
    },
    TextPrefix {
        leading: Vec<StructuralValue>,
        prefix: String,
    },
    ComponentRange {
        leading: Vec<StructuralValue>,
        lower: Option<(StructuralValue, bool)>,
        upper: Option<(StructuralValue, bool)>,
    },
    Union {
        branches: Vec<EvaluatedListAccessSelection>,
    },
    Intersection {
        branches: Vec<EvaluatedListAccessSelection>,
    },
}

#[derive(Clone, Copy)]
enum ListAccessSelectionBranchKind {
    Union,
    Intersection,
}

enum ListAccessSelectionTask<'a> {
    Evaluate(&'a PlanListAccessSelection),
    FinishBranches {
        kind: ListAccessSelectionBranchKind,
        value_base: usize,
        branch_count: usize,
    },
}

const MAX_LIST_ACCESS_SELECTION_CONTINUATIONS: usize = 8_192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvalOrderDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OrderedEvalItem {
    value: EvalValue,
    keys: Vec<Value>,
    source_order: usize,
}

type PlanLocalBindings = BTreeMap<(PlanStaticOwnerId, PlanLocalId), EvalValue>;

#[derive(Clone, Copy)]
struct ExpressionContext<'a> {
    row: Option<RowId>,
    event: Option<&'a SourceEvent>,
    output: Option<FieldId>,
    consumer: Option<Consumer>,
}

enum ExpressionEntry<'a> {
    Row {
        expression: PlanRowExpressionId,
        context: ExpressionContext<'a>,
    },
    CurrentnessOp {
        op: PlanOpId,
        row: Option<RowId>,
        event: Option<&'a SourceEvent>,
    },
    ValueRef {
        value_ref: ValueRef,
        context: ExpressionContext<'a>,
    },
}

struct ExpressionBindingUndo {
    key: (PlanStaticOwnerId, PlanLocalId),
    previous: Option<EvalValue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpressionCurrentnessTarget {
    Root(FieldId),
    Row(RowId, FieldId),
    List(ListId),
}

enum ExpressionValueRef<'plan> {
    Arena(PlanRowExpressionId),
    List(ListId),
    Derived(&'plan ValueRef),
}

#[derive(Clone, Copy)]
enum ExpressionProjectionTarget {
    Value,
    List(ListId),
}

const MAX_EVALUATED_BUILTIN_ARGS: usize = 8;

struct EvaluatedBuiltinArgs {
    entries: [Option<(&'static str, EvalValue)>; MAX_EVALUATED_BUILTIN_ARGS],
}

impl EvaluatedBuiltinArgs {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
        }
    }

    fn insert(&mut self, name: &'static str, value: EvalValue) -> Result<(), Error> {
        if self
            .entries
            .iter()
            .flatten()
            .any(|(candidate, _)| *candidate == name)
        {
            return Err(Error::InvalidPlan(format!(
                "evaluated builtin argument `{name}` is repeated"
            )));
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "builtin evaluation exceeds its {}-argument bound",
                    MAX_EVALUATED_BUILTIN_ARGS
                ))
            })?;
        *entry = Some((name, value));
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Option<EvalValue> {
        let entry = self.entries.iter_mut().find(|entry| {
            entry
                .as_ref()
                .is_some_and(|(candidate, _)| *candidate == name)
        })?;
        entry.take().map(|(_, value)| value)
    }

    fn first_name(&self) -> Option<&'static str> {
        self.entries.iter().flatten().next().map(|(name, _)| *name)
    }
}

struct ContextualCollectionState {
    expression: PlanRowExpressionId,
    owner: PlanStaticOwnerId,
    operation: PlanContextualOperationKind,
    local: (PlanStaticOwnerId, PlanLocalId),
    body: PlanRowExpressionId,
    remaining: std::vec::IntoIter<OrderedEvalItem>,
    directions: Option<Vec<EvalOrderDirection>>,
    output: Vec<OrderedEvalItem>,
}

struct ContextualMapCaptureState {
    collection: Box<ContextualCollectionState>,
    item: OrderedEvalItem,
    origin: Option<RowId>,
    mapped: EvalValue,
    next_capture: usize,
    evaluated_captures: BTreeMap<FieldId, EvalValue>,
}

struct ContextualOrderState {
    local: (PlanStaticOwnerId, PlanLocalId),
    key: PlanRowExpressionId,
    remaining: std::vec::IntoIter<OrderedEvalItem>,
    ordered: Vec<OrderedEvalItem>,
    directions: Vec<EvalOrderDirection>,
}

struct IndexedContextualState {
    operation: PlanContextualOperationKind,
    local: (PlanStaticOwnerId, PlanLocalId),
    body: PlanRowExpressionId,
    remaining: std::vec::IntoIter<RowId>,
    retained: Vec<EvalValue>,
}

struct RowOwnedCallState<'event, 'plan> {
    import_id: ImportId,
    call: &'plan RemoteCallSitePlan,
    call_instance_id: DistributedCallInstanceId,
    consumer: Consumer,
    context: ExpressionContext<'event>,
    next_argument: usize,
    arguments: DistributedCurrentCallArguments,
}

enum ExpressionTask<'event, 'plan> {
    Evaluate {
        expression: PlanRowExpressionId,
        context: ExpressionContext<'event>,
    },
    Apply {
        expression: PlanRowExpressionId,
        value_base: usize,
        context: ExpressionContext<'event>,
    },
    EvaluateDerived {
        expression: &'plan PlanDerivedExpression,
        context: ExpressionContext<'event>,
    },
    MaterializeListAfterValue {
        target_list: ListId,
        authority_source_list: Option<ListId>,
        fields: &'plan BTreeMap<String, FieldId>,
        row_field_copies: &'plan [PlanMaterializedRowFieldCopy],
        owner_prefix: Vec<OwnerInstanceRow>,
        authority_depth: usize,
        event: Option<&'event SourceEvent>,
    },
    DerivedSourceKeyAfterValue {
        skip_empty: bool,
    },
    DerivedBoolNotAfterValue,
    DerivedNumberCompareAfterValue {
        op: PlanInfixOp,
        right: FiniteReal,
    },
    DerivedValueCompareAfterLeft {
        op: PlanInfixOp,
        right: &'plan ValueRef,
        context: ExpressionContext<'event>,
    },
    DerivedValueCompareAfterRight {
        op: PlanInfixOp,
        left: EvalValue,
    },
    DerivedBoolAndAfterLeft {
        right: &'plan PlanDerivedExpression,
        context: ExpressionContext<'event>,
    },
    DerivedBoolAndAfterRight,
    DerivedBoolNotExpressionAfterValue,
    BeginRowOwnedCall {
        import_id: ImportId,
        context: ExpressionContext<'event>,
    },
    RowOwnedCallNext {
        state: RowOwnedCallState<'event, 'plan>,
    },
    RowOwnedCallAfterArgument {
        state: RowOwnedCallState<'event, 'plan>,
    },
    ValueRef {
        value_ref: ExpressionValueRef<'plan>,
        context: ExpressionContext<'event>,
    },
    ListValue {
        list: ListId,
        context: ExpressionContext<'event>,
    },
    StateValue {
        state: StateId,
        context: ExpressionContext<'event>,
    },
    StateProjectionAfterValue {
        expression: PlanRowExpressionId,
        state: StateId,
    },
    DerivedStateProjectionAfterValue {
        field_path: &'plan [String],
        state: StateId,
    },
    EnsureRoot {
        field: FieldId,
        event: Option<&'event SourceEvent>,
    },
    FinishRoot {
        field: FieldId,
    },
    EnsureRow {
        row: RowId,
        field: FieldId,
        event: Option<&'event SourceEvent>,
    },
    FinishRow {
        row: RowId,
        field: FieldId,
    },
    EnsureList {
        list: ListId,
        event: Option<&'event SourceEvent>,
    },
    FinishList {
        list: ListId,
        op: PlanOpId,
    },
    EvaluateCurrentnessOp {
        op: PlanOpId,
        row: Option<RowId>,
        event: Option<&'event SourceEvent>,
    },
    ProjectionAfterSource {
        op: PlanOpId,
        size: usize,
        target: ExpressionProjectionTarget,
    },
    ListGetFieldAfterIndex {
        list: ListId,
        field: FieldId,
        context: ExpressionContext<'event>,
    },
    ListRowFieldAfterRow {
        list: ListId,
        field: FieldId,
        context: ExpressionContext<'event>,
    },
    SelectAfterInput {
        expression: PlanRowExpressionId,
        context: ExpressionContext<'event>,
    },
    BuiltinAfterOperands {
        expression: PlanRowExpressionId,
        value_base: usize,
    },
    BuiltinBoolAfterLeft {
        function: PlanRowBuiltin,
        right: PlanRowExpressionId,
        context: ExpressionContext<'event>,
    },
    BuiltinBoolAfterRight,
    ContextualCollectionAfterSource {
        expression: PlanRowExpressionId,
        owner: PlanStaticOwnerId,
        operation: PlanContextualOperationKind,
        row_local: PlanLocalId,
        body: PlanRowExpressionId,
        context: ExpressionContext<'event>,
    },
    ContextualCollectionNext {
        state: Box<ContextualCollectionState>,
        context: ExpressionContext<'event>,
    },
    ContextualCollectionAfterPredicate {
        state: Box<ContextualCollectionState>,
        item: OrderedEvalItem,
        context: ExpressionContext<'event>,
    },
    ContextualCollectionAfterMapBody {
        state: Box<ContextualCollectionState>,
        item: OrderedEvalItem,
        origin: Option<RowId>,
        context: ExpressionContext<'event>,
    },
    ContextualMapCaptureNext {
        state: Box<ContextualMapCaptureState>,
        context: ExpressionContext<'event>,
    },
    ContextualMapCaptureAfterValue {
        state: Box<ContextualMapCaptureState>,
        field: FieldId,
        context: ExpressionContext<'event>,
    },
    ContextualOrderAfterSource {
        operation: PlanOrderOperationKind,
        owner: PlanStaticOwnerId,
        row_local: PlanLocalId,
        key: PlanRowExpressionId,
        direction: PlanRowExpressionId,
        context: ExpressionContext<'event>,
    },
    ContextualOrderAfterDirection {
        operation: PlanOrderOperationKind,
        owner: PlanStaticOwnerId,
        row_local: PlanLocalId,
        key: PlanRowExpressionId,
        items: Vec<OrderedEvalItem>,
        directions: Option<Vec<EvalOrderDirection>>,
        context: ExpressionContext<'event>,
    },
    ContextualOrderNext {
        state: Box<ContextualOrderState>,
        context: ExpressionContext<'event>,
    },
    ContextualOrderAfterKey {
        state: Box<ContextualOrderState>,
        item: OrderedEvalItem,
        context: ExpressionContext<'event>,
    },
    IndexedContextualNext {
        state: Box<IndexedContextualState>,
        context: ExpressionContext<'event>,
    },
    IndexedContextualAfterPredicate {
        state: Box<IndexedContextualState>,
        candidate: RowId,
        context: ExpressionContext<'event>,
    },
    RestoreBinding {
        undo: usize,
    },
}

struct ExpressionWorkStack<'event, 'plan> {
    tasks: Vec<ExpressionTask<'event, 'plan>>,
    values: Vec<EvalValue>,
    limit: usize,
}

impl<'event, 'plan> ExpressionWorkStack<'event, 'plan> {
    fn new(limit: usize) -> Self {
        Self {
            tasks: Vec::new(),
            values: Vec::new(),
            limit,
        }
    }

    fn push_task(&mut self, task: ExpressionTask<'event, 'plan>) -> Result<(), Error> {
        if self.tasks.len() >= self.limit {
            return Err(Error::InvalidPlan(format!(
                "expression continuation stack exceeded its plan-derived bound of {}",
                self.limit
            )));
        }
        self.tasks.push(task);
        Ok(())
    }

    fn push_value(&mut self, value: EvalValue) -> Result<(), Error> {
        if self.values.len() >= self.limit {
            return Err(Error::InvalidPlan(format!(
                "expression value stack exceeded its plan-derived bound of {}",
                self.limit
            )));
        }
        self.values.push(value);
        Ok(())
    }

    fn pop_value(&mut self) -> Result<EvalValue, Error> {
        self.values.pop().ok_or_else(|| {
            Error::InvalidPlan("expression continuation produced no operand value".to_owned())
        })
    }
}

fn restore_expression_binding(bindings: &mut PlanLocalBindings, undo: ExpressionBindingUndo) {
    match undo.previous {
        Some(previous) => {
            bindings.insert(undo.key, previous);
        }
        None => {
            bindings.remove(&undo.key);
        }
    }
}

fn schedule_bound_expression<'event, 'plan>(
    stack: &mut ExpressionWorkStack<'event, 'plan>,
    undos: &mut Vec<ExpressionBindingUndo>,
    bindings: &mut PlanLocalBindings,
    binding: (EvalValue, (PlanStaticOwnerId, PlanLocalId)),
    expression: PlanRowExpressionId,
    context: ExpressionContext<'event>,
) -> Result<(), Error> {
    let (value, key) = binding;
    if undos.len() >= stack.limit {
        return Err(Error::InvalidPlan(format!(
            "expression binding stack exceeded its plan-derived bound of {}",
            stack.limit
        )));
    }
    let previous = bindings.insert(key, value);
    let undo = undos.len();
    undos.push(ExpressionBindingUndo { key, previous });
    stack.push_task(ExpressionTask::RestoreBinding { undo })?;
    stack.push_task(ExpressionTask::Evaluate {
        expression,
        context,
    })
}

fn schedule_isolated_expression<'event, 'plan>(
    stack: &mut ExpressionWorkStack<'event, 'plan>,
    undos: &mut Vec<ExpressionBindingUndo>,
    bindings: &mut PlanLocalBindings,
    binding: Option<(EvalValue, (PlanStaticOwnerId, PlanLocalId))>,
    expression: PlanRowExpressionId,
    context: ExpressionContext<'event>,
) -> Result<(), Error> {
    let required = bindings
        .len()
        .saturating_add(usize::from(binding.is_some()));
    if undos.len().saturating_add(required) > stack.limit {
        return Err(Error::InvalidPlan(format!(
            "expression binding stack exceeded its plan-derived bound of {}",
            stack.limit
        )));
    }

    let keys = bindings.keys().copied().collect::<Vec<_>>();
    let restore_start = undos.len();
    for key in keys {
        let previous = bindings.remove(&key);
        undos.push(ExpressionBindingUndo { key, previous });
    }
    for undo in restore_start..undos.len() {
        stack.push_task(ExpressionTask::RestoreBinding { undo })?;
    }

    if let Some(binding) = binding {
        schedule_bound_expression(stack, undos, bindings, binding, expression, context)
    } else {
        stack.push_task(ExpressionTask::Evaluate {
            expression,
            context,
        })
    }
}

fn finish_expression_currentness(
    targets: &mut Vec<ExpressionCurrentnessTarget>,
    expected: ExpressionCurrentnessTarget,
) -> Result<(), Error> {
    let actual = targets.pop();
    if actual != Some(expected) {
        return Err(Error::InvalidPlan(format!(
            "expression currentness continuation closed {expected:?} after {actual:?}"
        )));
    }
    Ok(())
}

fn push_expression_operand<'event, 'plan>(
    stack: &mut ExpressionWorkStack<'event, 'plan>,
    expression: PlanRowExpressionId,
    context: ExpressionContext<'event>,
) -> Result<(), Error> {
    stack.push_task(ExpressionTask::Evaluate {
        expression,
        context,
    })
}

fn schedule_apply_operands<'event, 'plan>(
    stack: &mut ExpressionWorkStack<'event, 'plan>,
    node: &PlanRowExpressionNode,
    context: ExpressionContext<'event>,
) -> Result<(), Error> {
    let mut push = |expression| push_expression_operand(stack, expression, context);
    match node {
        PlanRowExpressionNode::TextTrim { input }
        | PlanRowExpressionNode::TextIsEmpty { input }
        | PlanRowExpressionNode::TextLength { input }
        | PlanRowExpressionNode::TextToNumber { input }
        | PlanRowExpressionNode::BytesToHex { input }
        | PlanRowExpressionNode::BytesToBase64 { input }
        | PlanRowExpressionNode::BytesFromHex { input }
        | PlanRowExpressionNode::BytesFromBase64 { input }
        | PlanRowExpressionNode::BytesIsEmpty { input }
        | PlanRowExpressionNode::BytesLength { input }
        | PlanRowExpressionNode::ListSum { input }
        | PlanRowExpressionNode::ObjectField { object: input, .. } => push(*input)?,
        PlanRowExpressionNode::TextStartsWith { input, prefix }
        | PlanRowExpressionNode::BytesStartsWith { input, prefix }
        | PlanRowExpressionNode::BytesConcat {
            left: input,
            right: prefix,
        }
        | PlanRowExpressionNode::BytesEqual {
            left: input,
            right: prefix,
        }
        | PlanRowExpressionNode::NumberInfix {
            left: input,
            right: prefix,
            ..
        } => {
            push(*prefix)?;
            push(*input)?;
        }
        PlanRowExpressionNode::BytesEndsWith { input, suffix }
        | PlanRowExpressionNode::BytesFind {
            input,
            needle: suffix,
        }
        | PlanRowExpressionNode::BytesGet {
            input,
            index: suffix,
        }
        | PlanRowExpressionNode::BytesTake {
            input,
            byte_count: suffix,
        }
        | PlanRowExpressionNode::BytesDrop {
            input,
            byte_count: suffix,
        } => {
            push(*suffix)?;
            push(*input)?;
        }
        PlanRowExpressionNode::TextSubstring {
            input,
            start,
            length,
        }
        | PlanRowExpressionNode::BytesSlice {
            input,
            offset: start,
            byte_count: length,
        }
        | PlanRowExpressionNode::BytesSet {
            input,
            index: start,
            value: length,
        } => {
            push(*length)?;
            push(*start)?;
            push(*input)?;
        }
        PlanRowExpressionNode::TextToBytes { input, encoding }
        | PlanRowExpressionNode::BytesToText { input, encoding } => {
            // Encoding is evaluated and validated before the input.
            push(*input)?;
            if let Some(encoding) = encoding {
                push(*encoding)?;
            }
        }
        PlanRowExpressionNode::BytesZeros { byte_count } => push(*byte_count)?,
        PlanRowExpressionNode::BytesReadUnsigned {
            input,
            offset,
            byte_count,
            endian,
        }
        | PlanRowExpressionNode::BytesReadSigned {
            input,
            offset,
            byte_count,
            endian,
        } => {
            push(*endian)?;
            push(*byte_count)?;
            push(*offset)?;
            push(*input)?;
        }
        PlanRowExpressionNode::BytesWriteUnsigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpressionNode::BytesWriteSigned {
            input,
            offset,
            byte_count,
            endian,
            value,
        } => {
            push(*value)?;
            push(*endian)?;
            push(*byte_count)?;
            push(*offset)?;
            push(*input)?;
        }
        PlanRowExpressionNode::TextConcat { parts }
        | PlanRowExpressionNode::ListLiteral { items: parts } => {
            for part in parts.iter().rev() {
                push(*part)?;
            }
        }
        PlanRowExpressionNode::ListRange { from, to } => {
            push(*to)?;
            push(*from)?;
        }
        PlanRowExpressionNode::Object { fields }
        | PlanRowExpressionNode::TaggedObject { fields, .. } => {
            for field in fields.iter().rev() {
                push(field.value)?;
            }
        }
        PlanRowExpressionNode::Intrinsic { .. }
        | PlanRowExpressionNode::Field { .. }
        | PlanRowExpressionNode::Constant { .. }
        | PlanRowExpressionNode::ListGetField { .. }
        | PlanRowExpressionNode::ListRef { .. }
        | PlanRowExpressionNode::AuthorityListRef { .. }
        | PlanRowExpressionNode::ContextualCollection { .. }
        | PlanRowExpressionNode::ContextualOrder { .. }
        | PlanRowExpressionNode::ListAccess { .. }
        | PlanRowExpressionNode::ListPage { .. }
        | PlanRowExpressionNode::BoundedListPage { .. }
        | PlanRowExpressionNode::Local { .. }
        | PlanRowExpressionNode::LocalRow { .. }
        | PlanRowExpressionNode::EventRow { .. }
        | PlanRowExpressionNode::ListRowField { .. }
        | PlanRowExpressionNode::BuiltinCall { .. }
        | PlanRowExpressionNode::Select { .. } => {
            return Err(Error::InvalidPlan(
                "non-apply row expression reached the apply operand scheduler".to_owned(),
            ));
        }
    }
    Ok(())
}

fn schedule_builtin_operands<'event, 'plan>(
    stack: &mut ExpressionWorkStack<'event, 'plan>,
    function: PlanRowBuiltin,
    input: Option<PlanRowExpressionId>,
    args: &[PlanRowCallArg],
    context: ExpressionContext<'event>,
) -> Result<(), Error> {
    function
        .validate_call(input, args)
        .map_err(|error| Error::InvalidPlan(error.to_string()))?;

    if function == PlanRowBuiltin::ListTake {
        let count = args
            .iter()
            .find(|argument| argument.name == "count")
            .ok_or_else(|| {
                Error::InvalidPlan("List/take has no compiled count operand".to_owned())
            })?;
        let input = input
            .ok_or_else(|| Error::InvalidPlan("List/take has no compiled input".to_owned()))?;
        push_expression_operand(stack, input, context)?;
        push_expression_operand(stack, count.value, context)?;
        return Ok(());
    }

    for parameter in function.signature().parameters.iter().rev() {
        if parameter.receiver {
            let input = input.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "{} has no compiled input",
                    function.function_name()
                ))
            })?;
            push_expression_operand(stack, input, context)?;
        } else if let Some(argument) = args.iter().find(|argument| argument.name == parameter.name)
        {
            push_expression_operand(stack, argument.value, context)?;
        }
    }
    Ok(())
}

trait IntoExpressionId {
    fn into_expression_id(self) -> PlanRowExpressionId;
}

impl IntoExpressionId for PlanRowExpressionId {
    fn into_expression_id(self) -> PlanRowExpressionId {
        self
    }
}

impl IntoExpressionId for &PlanRowExpressionId {
    fn into_expression_id(self) -> PlanRowExpressionId {
        *self
    }
}

type DistributedCurrentCallArguments = BTreeMap<DistributedArgumentId, Value>;
#[derive(Clone, Eq, PartialEq)]
struct DistributedCurrentCallResult {
    arguments: DistributedCurrentCallArguments,
    content_revision: u64,
    value: Value,
}

type DistributedCurrentCallDemands = BTreeMap<
    Consumer,
    BTreeMap<(RemoteCallSiteId, DistributedCallInstanceId), DistributedCurrentCallArguments>,
>;

#[derive(Clone, Debug)]
enum PendingListMutation {
    Append {
        site: usize,
        ordinal: u32,
        sequence: u64,
        owner: OwnerInstanceId,
        list: ListId,
        fields: BTreeMap<FieldId, Value>,
    },
    Remove {
        site: usize,
        ordinal: u32,
        sequence: u64,
        owner: OwnerInstanceId,
        rows: Vec<RowId>,
    },
}

impl PendingListMutation {
    fn ordinal(&self) -> u32 {
        match self {
            Self::Append { ordinal, .. } | Self::Remove { ordinal, .. } => *ordinal,
        }
    }

    fn site_owner(&self) -> (usize, &OwnerInstanceId) {
        match self {
            Self::Append { site, owner, .. } | Self::Remove { site, owner, .. } => (*site, owner),
        }
    }

    fn sequence(&self) -> u64 {
        match self {
            Self::Append { sequence, .. } | Self::Remove { sequence, .. } => *sequence,
        }
    }
}

#[derive(Clone)]
pub struct MachineInstance {
    plan: Arc<MachinePlan>,
    options: SessionOptions,
    metadata: Arc<Metadata>,
    distributed_imports: BTreeMap<ImportId, Value>,
    distributed_import_revisions: BTreeMap<ImportId, u64>,
    row_owned_call_results:
        BTreeMap<(ImportId, DistributedCallInstanceId), DistributedCurrentCallResult>,
    distributed_current_call_demands: DistributedCurrentCallDemands,
    root_states: BTreeMap<StateId, Value>,
    root_fields: BTreeMap<FieldId, DerivedCell>,
    derived_lists: BTreeMap<ListId, DerivedListCell>,
    lists: BTreeMap<ListId, ListState>,
    ordered_indexes: BTreeMap<PlanListIndexId, OrderedIndex>,
    dirty_ordered_indexes: BTreeSet<PlanListIndexId>,
    dirty_ordered_index_rows: BTreeMap<PlanListIndexId, BTreeSet<RowId>>,
    evaluating_ordered_indexes: BTreeSet<PlanListIndexId>,
    dynamic_dependencies: DynamicDependencies,
    list_access_flush_in_progress: bool,
    last_sequence: Option<u64>,
    turn_sequence: u64,
    launch_epoch: u64,
    cursor_sealing_key: CursorSealingKey,
    cursor_ephemeral_launch_epoch: Option<u64>,
    transient_effect_scope: u64,
    next_transient_effect_sequence: u64,
    pending_transient_effects: BTreeMap<TransientEffectCallId, PendingTransientEffect>,
    effect_activations: BTreeMap<EffectConsumer, EffectActivation>,
    root_source_bindings: BTreeMap<SourceId, u64>,
    next_binding_id: u64,
    touched_root_states: BTreeSet<StateId>,
    touched_lists: BTreeSet<ListId>,
    touched_row_fields: BTreeSet<(RowId, FieldId)>,
    machine_origin: MachineOrigin,
    producer_leases: BTreeMap<ProducerLeaseKey, ProducerLeaseState>,
    active_producer_lease: Option<ActiveProducerLease>,
    global_dependency_revision: u64,
    turn_work: Work,
    startup_metrics: TurnMetrics,
}

impl CursorIdentityResolver for MachineInstance {
    fn semantic_row_id(&self, row: RowId) -> Option<CursorSemanticRowId> {
        let (memory_id, type_fingerprint) = self
            .metadata
            .semantic_list_identities
            .get(&row.list)
            .copied()?;
        Some(CursorSemanticRowId::new(
            memory_id,
            type_fingerprint,
            row.key,
            row.generation,
        ))
    }

    fn semantic_row_field_id(&self, row: RowId, field: FieldId) -> Option<[u8; 32]> {
        self.metadata
            .semantic_row_field_identities
            .get(&(row.list, field))
            .copied()
    }
}

#[derive(Default)]
struct PreparedOrderedIndexImage {
    indexes: BTreeMap<PlanListIndexId, OrderedIndex>,
    logical_row_count: u64,
    entry_count: u64,
    expanded_key_count: u64,
    encoded_key_bytes: u64,
    structural_key_bytes: u64,
    payload_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MachineOrigin {
    slot: u32,
    generation: u64,
}

impl MachineOrigin {
    pub const LOCAL: Self = Self {
        slot: u32::MAX,
        generation: u64::MAX,
    };

    pub fn new(slot: u32, generation: u64) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidEvent(
                "machine origin generation must be positive".to_owned(),
            ));
        }
        Ok(Self { slot, generation })
    }

    pub const fn slot(self) -> u32 {
        self.slot
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProducerLeaseKey {
    origin: MachineOrigin,
    call_site_id: RemoteCallSiteId,
    call_owner: PlanStaticOwnerId,
    call_instance_id: DistributedCallInstanceId,
}

#[derive(Clone, Default)]
struct ProducerLeaseState {
    initialized: bool,
    seen_global_revision: u64,
    distributed_imports: BTreeMap<ImportId, Value>,
    distributed_import_revisions: BTreeMap<ImportId, u64>,
    root_states: BTreeMap<StateId, Value>,
    root_fields: BTreeMap<FieldId, DerivedCell>,
    derived_lists: BTreeMap<ListId, DerivedListCell>,
    lists: BTreeMap<ListId, ListState>,
    ordered_indexes: BTreeMap<PlanListIndexId, OrderedIndex>,
    dirty_ordered_indexes: BTreeSet<PlanListIndexId>,
    dirty_ordered_index_rows: BTreeMap<PlanListIndexId, BTreeSet<RowId>>,
    dynamic_dependencies: DynamicDependencies,
    distributed_current_call_demands: DistributedCurrentCallDemands,
    row_owned_call_results:
        BTreeMap<(ImportId, DistributedCallInstanceId), DistributedCurrentCallResult>,
    producer_result: Option<Value>,
    root_source_bindings: BTreeMap<SourceId, u64>,
    touched_root_states: BTreeSet<StateId>,
    touched_lists: BTreeSet<ListId>,
    touched_row_fields: BTreeSet<(RowId, FieldId)>,
    pending_transient_effects: BTreeMap<TransientEffectCallId, PendingTransientEffect>,
    effect_activations: BTreeMap<EffectConsumer, EffectActivation>,
}

#[derive(Clone)]
struct ActiveProducerLease {
    key: ProducerLeaseKey,
    call_site_id: RemoteCallSiteId,
    ownership: boon_plan::ProducerFunctionOwnershipPlan,
    existed: bool,
    initialized: bool,
    saved_argument_imports: BTreeMap<ImportId, Option<Value>>,
    saved_argument_revisions: BTreeMap<ImportId, Option<u64>>,
    saved_dynamic_dependencies: DynamicDependencies,
    saved_distributed_current_call_demands: DistributedCurrentCallDemands,
    saved_effect_activations: BTreeMap<EffectConsumer, EffectActivation>,
    saved_row_owned_call_results:
        BTreeMap<(ImportId, DistributedCallInstanceId), DistributedCurrentCallResult>,
    producer_result: Option<Value>,
}

#[derive(Clone)]
pub struct MachineTemplate {
    plan: Arc<MachinePlan>,
    metadata: Arc<Metadata>,
}

impl MachineTemplate {
    pub fn new(plan: MachinePlan) -> Result<Self, Error> {
        Self::new_shared(Arc::new(plan))
    }

    pub fn new_shared(plan: Arc<MachinePlan>) -> Result<Self, Error> {
        if plan.version.major != boon_plan::PLAN_MAJOR_VERSION {
            return Err(Error::InvalidPlan(format!(
                "plan major version {} is not supported",
                plan.version.major
            )));
        }
        let verification = verify_plan(&plan)
            .map_err(|error| Error::InvalidPlan(format!("plan verification failed: {error}")))?;
        if verification.status != "pass" {
            let failures = verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .map(|check| format!("{}: {}", check.id, check.detail))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(Error::InvalidPlan(format!(
                "plan verification rejected runtime readiness: {failures}"
            )));
        }
        let metadata = Arc::new(Metadata::new(&plan)?);
        Ok(Self { plan, metadata })
    }

    pub fn shared_plan(&self) -> Arc<MachinePlan> {
        Arc::clone(&self.plan)
    }

    pub fn instantiate(&self, options: SessionOptions) -> Result<MachineInstanceBuilder, Error> {
        MachineInstanceBuilder::from_template(self.clone(), options)
    }
}

pub struct MachineInstanceBuilder {
    session: MachineInstance,
    authority: Option<AuthoritySnapshot>,
    durable_restore: Option<boon_persistence::RestoreImage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineBuildPhase {
    Bootstrap,
    TranslateDurableRestore,
    StorageRows,
    IndexedDefaults,
    RestoreAuthority,
    RuntimeState,
    RootDefaults,
    OrderedIndexes,
    PublishedCurrentness,
    Failed,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineBuildProgress {
    pub phase: MachineBuildPhase,
    pub completed_steps: u64,
}

pub enum MachineBuildPoll {
    Pending(MachineBuildProgress),
    Ready(MachineInstance),
}

#[derive(Default)]
struct StorageBuildCursor {
    slot: usize,
    row: u64,
}

#[derive(Default)]
struct IndexedDefaultsCursor {
    slot: usize,
    row: usize,
}

#[derive(Default)]
struct RootDefaultsCursor {
    slot: usize,
}

#[derive(Default)]
struct ListRowsCursor {
    slot: usize,
    row: usize,
}

enum AuthorityListRestoreState {
    SparseRows {
        rows: std::vec::IntoIter<RowAuthority>,
        seen: BTreeSet<RowId>,
    },
    ReplacementRows {
        rows: std::vec::IntoIter<RowAuthority>,
        next_key: u64,
        next_order_token: u128,
        built_rows: BTreeMap<RowId, Row>,
        order: ListOrder,
        order_tokens: BTreeMap<RowId, u128>,
        seen: BTreeSet<RowId>,
        previous_order_token: u128,
        minimum_next_key: u64,
    },
    InitializeReplacementRows {
        row: usize,
    },
    Complete,
}

struct AuthorityListRestoreBuild {
    list_id: ListId,
    revision: u64,
    allowed_fields: BTreeSet<FieldId>,
    state: AuthorityListRestoreState,
}

enum AuthorityRestorePhase {
    Begin,
    States,
    Lists,
    ValidateRows(ListRowsCursor),
    Complete,
}

struct AuthorityRestoreBuild {
    through_turn_sequence: u64,
    states: std::collections::btree_map::IntoIter<StateId, ScalarAuthority>,
    lists: std::collections::btree_map::IntoIter<ListId, ListAuthority>,
    current_list: Option<AuthorityListRestoreBuild>,
    phase: AuthorityRestorePhase,
}

enum DurableRestorePhase {
    Scalars,
    Lists,
    Complete,
}

struct DurableListTranslateBuild {
    memory_id: boon_plan::MemoryId,
    list_id: ListId,
    touched: bool,
    revision: u64,
    next_key: u64,
    next_order_token: u128,
    rows: std::vec::IntoIter<boon_persistence::StoredRow>,
    translated_rows: Vec<RowAuthority>,
}

struct DurableRestoreBuild {
    through_turn_sequence: u64,
    scalars:
        std::collections::btree_map::IntoIter<boon_plan::MemoryId, boon_persistence::StoredScalar>,
    lists: std::collections::btree_map::IntoIter<boon_plan::MemoryId, boon_persistence::StoredList>,
    current_list: Option<DurableListTranslateBuild>,
    states: BTreeMap<StateId, ScalarAuthority>,
    translated_lists: BTreeMap<ListId, ListAuthority>,
    phase: DurableRestorePhase,
}

enum RuntimeStateBuildPhase {
    ClearOwnerPartitions {
        list: usize,
    },
    RebuildOwnerPartitions(ListRowsCursor),
    ClearDynamicDependencies,
    ResetCaches,
    RootSourceBindings {
        sources: Vec<SourceId>,
        next: usize,
        clearing: bool,
    },
    Rows(ListRowsCursor),
    RootFields {
        fields: Vec<FieldId>,
        next: usize,
    },
    DerivedLists {
        lists: Vec<ListId>,
        next: usize,
    },
    Complete,
}

struct RuntimeStateBuild {
    phase: RuntimeStateBuildPhase,
}

enum PublishedCurrentnessPhase {
    CollectDirty { next: usize },
    EvaluateDirty { next: usize },
}

struct PublishedCurrentnessBuild {
    fields: Vec<FieldId>,
    dirty: Vec<FieldId>,
    phase: PublishedCurrentnessPhase,
    completed_passes: usize,
}

struct OrderedIndexCandidateBuild {
    plan: PlanListIndex,
    next_row: usize,
    index: Option<OrderedIndex>,
    integrity: Option<OrderedIndexIntegrityTask>,
}

struct OrderedIndexImageBuild {
    plans: Vec<PlanListIndex>,
    next_plan: usize,
    current: Option<OrderedIndexCandidateBuild>,
    prepared: PreparedOrderedIndexImage,
}

enum MachineBuildState {
    Bootstrap,
    TranslateDurableRestore(DurableRestoreBuild),
    StorageRows(StorageBuildCursor),
    IndexedDefaults(IndexedDefaultsCursor),
    RestoreAuthority(AuthorityRestoreBuild),
    RuntimeState(RuntimeStateBuild),
    RootDefaults(RootDefaultsCursor),
    OrderedIndexes(OrderedIndexImageBuild),
    PublishedCurrentness(PublishedCurrentnessBuild),
    Failed,
    Complete,
}

pub struct MachineBuildTask {
    session: Option<MachineInstance>,
    authority: Option<AuthoritySnapshot>,
    durable_restore: Option<boon_persistence::RestoreImage>,
    work: Work,
    state: MachineBuildState,
    completed_steps: u64,
}

fn next_session_launch_epoch() -> Result<u64, Error> {
    NEXT_SESSION_LAUNCH_EPOCH
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(1)
        })
        .map_err(|_| Error::Evaluation("session launch epoch exhausted".to_owned()))
}

fn session_cursor_sealing_key(options: &SessionOptions) -> Result<CursorSealingKey, Error> {
    if let Some(key) = &options.cursor_sealing_key {
        return Ok(key.clone());
    }
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| Error::Evaluation(format!("cursor key generation failed: {error}")))?;
    Ok(CursorSealingKey::from_bytes(bytes))
}

fn storage_initializer_row_count(slot: &ListStorageSlot) -> Result<u64, Error> {
    let explicit = u64::try_from(slot.initial_rows.len()).map_err(|_| {
        Error::InvalidPlan(format!(
            "list {} initial row count does not fit the runtime key space",
            slot.list_id.0
        ))
    })?;
    let range = match (slot.initializer_kind, slot.range) {
        (ListInitializerKind::Range, Some(range)) if range.from <= range.to => {
            let count = i128::from(range.to) - i128::from(range.from) + 1;
            u64::try_from(count).map_err(|_| {
                Error::InvalidPlan(format!(
                    "list {} range row count does not fit the runtime key space",
                    slot.list_id.0
                ))
            })?
        }
        (ListInitializerKind::Range, None) => {
            return Err(Error::InvalidPlan(format!(
                "list {} range has no bounds",
                slot.list_id.0
            )));
        }
        _ => 0,
    };
    explicit.checked_add(range).ok_or_else(|| {
        Error::InvalidPlan(format!(
            "list {} initializer row count overflowed",
            slot.list_id.0
        ))
    })
}

fn record_ordered_index_access_error(work: &mut Work, error: &AccessError) {
    if matches!(error, AccessError::ResourceLimitExceeded { .. }) {
        work.metrics.ordered_index_resource_limit_failure_count = work
            .metrics
            .ordered_index_resource_limit_failure_count
            .saturating_add(1);
    }
}

fn record_ordered_index_integrity(
    work: &mut Work,
    report: boon_list_access::IntegrityReport,
    expanded: bool,
) {
    work.metrics.ordered_index_rebuild_logical_row_count = work
        .metrics
        .ordered_index_rebuild_logical_row_count
        .saturating_add(report.logical_rows);
    work.metrics.ordered_index_rebuild_entry_count = work
        .metrics
        .ordered_index_rebuild_entry_count
        .saturating_add(report.index_entries);
    if expanded {
        work.metrics.ordered_index_rebuild_expanded_key_count = work
            .metrics
            .ordered_index_rebuild_expanded_key_count
            .saturating_add(report.index_entries);
    }
    work.metrics.ordered_index_rebuild_encoded_key_bytes = work
        .metrics
        .ordered_index_rebuild_encoded_key_bytes
        .saturating_add(report.encoded_key_bytes);
    work.metrics.ordered_index_rebuild_structural_key_bytes = work
        .metrics
        .ordered_index_rebuild_structural_key_bytes
        .saturating_add(report.structural_key_bytes);
    work.metrics.ordered_index_rebuild_payload_bytes = work
        .metrics
        .ordered_index_rebuild_payload_bytes
        .saturating_add(report.payload_bytes());
}

impl AuthorityRestoreBuild {
    fn new(authority: AuthoritySnapshot) -> Self {
        Self {
            through_turn_sequence: authority.through_turn_sequence,
            states: authority.states.into_iter(),
            lists: authority.lists.into_iter(),
            current_list: None,
            phase: AuthorityRestorePhase::Begin,
        }
    }
}

impl DurableRestoreBuild {
    fn new(image: boon_persistence::RestoreImage) -> Self {
        Self {
            through_turn_sequence: image.through_turn_sequence,
            scalars: image.scalars.into_iter(),
            lists: image.lists.into_iter(),
            current_list: None,
            states: BTreeMap::new(),
            translated_lists: BTreeMap::new(),
            phase: DurableRestorePhase::Scalars,
        }
    }

    fn finish(&mut self) -> AuthoritySnapshot {
        AuthoritySnapshot {
            through_turn_sequence: self.through_turn_sequence,
            states: std::mem::take(&mut self.states),
            lists: std::mem::take(&mut self.translated_lists),
        }
    }
}

impl RuntimeStateBuild {
    fn new() -> Self {
        Self {
            phase: RuntimeStateBuildPhase::ClearOwnerPartitions { list: 0 },
        }
    }
}

impl PublishedCurrentnessBuild {
    fn new(session: &MachineInstance) -> Self {
        Self {
            fields: session.metadata.published.iter().copied().collect(),
            dirty: Vec::new(),
            phase: PublishedCurrentnessPhase::CollectDirty { next: 0 },
            completed_passes: 0,
        }
    }
}

impl MachineInstanceBuilder {
    pub fn new(plan: MachinePlan, options: SessionOptions) -> Result<Self, Error> {
        Self::new_shared(Arc::new(plan), options)
    }

    pub fn new_shared(plan: Arc<MachinePlan>, options: SessionOptions) -> Result<Self, Error> {
        MachineTemplate::new_shared(plan)?.instantiate(options)
    }

    fn from_template(template: MachineTemplate, options: SessionOptions) -> Result<Self, Error> {
        validate_session_options(&options)?;
        let MachineTemplate { plan, metadata } = template;
        let distributed_imports = metadata
            .distributed_import_types
            .keys()
            .copied()
            .map(|import_id| {
                (
                    import_id,
                    Value::Error {
                        code: "remote_not_current".to_owned(),
                    },
                )
            })
            .collect();
        let turn_work = Work::with_limit(options.max_work_units_per_transaction);
        let launch_epoch = next_session_launch_epoch()?;
        let cursor_ephemeral_launch_epoch =
            options.cursor_sealing_key.is_none().then_some(launch_epoch);
        let cursor_sealing_key = session_cursor_sealing_key(&options)?;
        Ok(Self {
            session: MachineInstance {
                plan,
                options,
                metadata,
                distributed_imports,
                distributed_import_revisions: BTreeMap::new(),
                row_owned_call_results: BTreeMap::new(),
                distributed_current_call_demands: BTreeMap::new(),
                root_states: BTreeMap::new(),
                root_fields: BTreeMap::new(),
                derived_lists: BTreeMap::new(),
                lists: BTreeMap::new(),
                ordered_indexes: BTreeMap::new(),
                dirty_ordered_indexes: BTreeSet::new(),
                dirty_ordered_index_rows: BTreeMap::new(),
                evaluating_ordered_indexes: BTreeSet::new(),
                dynamic_dependencies: DynamicDependencies::default(),
                list_access_flush_in_progress: false,
                last_sequence: None,
                turn_sequence: 0,
                launch_epoch,
                cursor_sealing_key,
                cursor_ephemeral_launch_epoch,
                transient_effect_scope: 0,
                next_transient_effect_sequence: 1,
                pending_transient_effects: BTreeMap::new(),
                effect_activations: BTreeMap::new(),
                root_source_bindings: BTreeMap::new(),
                next_binding_id: 1,
                touched_root_states: BTreeSet::new(),
                touched_lists: BTreeSet::new(),
                touched_row_fields: BTreeSet::new(),
                machine_origin: MachineOrigin::LOCAL,
                producer_leases: BTreeMap::new(),
                active_producer_lease: None,
                global_dependency_revision: 1,
                turn_work,
                startup_metrics: TurnMetrics::default(),
            },
            authority: None,
            durable_restore: None,
        })
    }

    pub fn restore(mut self, authority: AuthoritySnapshot) -> Self {
        self.authority = Some(authority);
        self.durable_restore = None;
        self
    }

    pub fn restore_durable(mut self, image: boon_persistence::RestoreImage) -> Result<Self, Error> {
        self.session.validate_durable_restore_header(&image)?;
        self.authority = None;
        self.durable_restore = Some(image);
        Ok(self)
    }

    pub fn restore_recovery(mut self, image: MachineRecoveryImage) -> Result<Self, Error> {
        validate_session_context(&image.session_context)?;
        if image.last_source_sequence == Some(0) {
            return Err(Error::InvalidPlan(
                "machine recovery source sequence must be positive".to_owned(),
            ));
        }
        let declared_imports = self
            .session
            .metadata
            .recoverable_distributed_imports
            .clone();
        let recovered_imports = image
            .distributed_imports
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if declared_imports != recovered_imports {
            return Err(Error::InvalidPlan(
                "machine recovery distributed imports do not match the compiled endpoint"
                    .to_owned(),
            ));
        }

        let mut distributed_imports = self.session.distributed_imports.clone();
        let mut distributed_import_revisions = self.session.distributed_import_revisions.clone();
        for (import_id, recovered) in image.distributed_imports {
            if recovered.revision == Some(0) {
                return Err(Error::InvalidPlan(
                    "machine recovery import revision must be positive".to_owned(),
                ));
            }
            let value = runtime_value_from_data(&recovered.value);
            if recovered.revision.is_none()
                && !matches!(&value, Value::Error { code } if code == "remote_not_current")
            {
                return Err(Error::InvalidPlan(
                    "machine recovery import without a revision must be remote_not_current"
                        .to_owned(),
                ));
            }
            if recovered.revision.is_some() {
                let data_type = self
                    .session
                    .metadata
                    .distributed_import_types
                    .get(&import_id)
                    .expect("recovered import set was matched against compiled imports");
                validate_distributed_boundary_value(
                    &value,
                    data_type,
                    "machine recovery distributed import",
                )?;
            }
            distributed_imports.insert(import_id, value);
            if let Some(revision) = recovered.revision {
                distributed_import_revisions.insert(import_id, revision);
            }
        }

        self.session
            .validate_durable_restore_header(&image.authority)?;
        self.authority = None;
        self.durable_restore = Some(image.authority);
        self.session.last_sequence = image.last_source_sequence;
        self.session.options.session_context = image.session_context;
        self.session.distributed_imports = distributed_imports;
        self.session.distributed_import_revisions = distributed_import_revisions;
        Ok(self)
    }

    pub fn into_build_task(self) -> MachineBuildTask {
        let work = self.session.fresh_work();
        MachineBuildTask {
            session: Some(self.session),
            authority: self.authority,
            durable_restore: self.durable_restore,
            work,
            state: MachineBuildState::Bootstrap,
            completed_steps: 0,
        }
    }

    pub fn build(self) -> Result<MachineInstance, Error> {
        let mut task = self.into_build_task();
        loop {
            match task.poll(usize::MAX)? {
                MachineBuildPoll::Pending(_) => {}
                MachineBuildPoll::Ready(session) => return Ok(session),
            }
        }
    }
}

impl MachineBuildTask {
    pub fn progress(&self) -> MachineBuildProgress {
        MachineBuildProgress {
            phase: self.phase(),
            completed_steps: self.completed_steps,
        }
    }

    pub fn poll(&mut self, max_steps: usize) -> Result<MachineBuildPoll, Error> {
        if max_steps == 0 {
            return Err(Error::InvalidPlan(
                "machine build poll requires a positive step budget".to_owned(),
            ));
        }
        if matches!(self.state, MachineBuildState::Failed) {
            return Err(Error::InvalidPlan(
                "failed machine build task was polled again".to_owned(),
            ));
        }
        if self.session.is_none() {
            return Err(Error::InvalidPlan(
                "completed machine build task was polled again".to_owned(),
            ));
        }
        let result = self.poll_inner(max_steps);
        if result.is_err() {
            self.state = MachineBuildState::Failed;
            self.session.take();
        }
        result
    }

    fn poll_inner(&mut self, max_steps: usize) -> Result<MachineBuildPoll, Error> {
        let mut remaining = max_steps;
        loop {
            let state = std::mem::replace(&mut self.state, MachineBuildState::Complete);
            match state {
                MachineBuildState::Bootstrap => {
                    self.session_mut().initialize_root_source_bindings()?;
                    self.session_mut().initialize_storage_prelude()?;
                    self.finish_step(&mut remaining);
                    self.state = if let Some(image) = self.durable_restore.take() {
                        MachineBuildState::TranslateDurableRestore(DurableRestoreBuild::new(image))
                    } else {
                        MachineBuildState::StorageRows(StorageBuildCursor::default())
                    };
                }
                MachineBuildState::TranslateDurableRestore(mut build) => {
                    let mut finished = false;
                    while remaining != 0 {
                        let complete = self.poll_durable_restore_step(&mut build)?;
                        self.finish_step(&mut remaining);
                        if complete {
                            self.authority = Some(build.finish());
                            self.state =
                                MachineBuildState::StorageRows(StorageBuildCursor::default());
                            finished = true;
                            break;
                        }
                    }
                    if !finished && matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::TranslateDurableRestore(build);
                    }
                }
                MachineBuildState::StorageRows(mut cursor) => {
                    while remaining != 0 {
                        let Some((slot, row)) = self.next_storage_row(&mut cursor)? else {
                            self.state = if let Some(authority) = self.authority.take() {
                                MachineBuildState::RestoreAuthority(AuthorityRestoreBuild::new(
                                    authority,
                                ))
                            } else {
                                MachineBuildState::IndexedDefaults(IndexedDefaultsCursor::default())
                            };
                            break;
                        };
                        let mut work = std::mem::take(&mut self.work);
                        let result = self
                            .session_mut()
                            .initialize_storage_row(&slot, row, &mut work);
                        self.work = work;
                        result?;
                        self.finish_step(&mut remaining);
                    }
                    if matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::StorageRows(cursor);
                    }
                }
                MachineBuildState::IndexedDefaults(mut cursor) => {
                    while remaining != 0 {
                        let Some(row) = self.next_indexed_default_row(&mut cursor) else {
                            self.state =
                                MachineBuildState::RootDefaults(RootDefaultsCursor::default());
                            break;
                        };
                        let mut work = std::mem::take(&mut self.work);
                        let result = self
                            .session_mut()
                            .initialize_missing_indexed_states(row, &mut work);
                        self.work = work;
                        result?;
                        self.finish_step(&mut remaining);
                    }
                    if matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::IndexedDefaults(cursor);
                    }
                }
                MachineBuildState::RestoreAuthority(mut build) => {
                    while remaining != 0 {
                        let complete = self.poll_authority_restore_step(&mut build)?;
                        self.finish_step(&mut remaining);
                        if complete {
                            self.state = MachineBuildState::RuntimeState(RuntimeStateBuild::new());
                            break;
                        }
                    }
                    if matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::RestoreAuthority(build);
                    }
                }
                MachineBuildState::RuntimeState(mut build) => {
                    while remaining != 0 {
                        let complete = self.poll_runtime_state_step(&mut build)?;
                        self.finish_step(&mut remaining);
                        if complete {
                            self.state = MachineBuildState::IndexedDefaults(
                                IndexedDefaultsCursor::default(),
                            );
                            break;
                        }
                    }
                    if matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::RuntimeState(build);
                    }
                }
                MachineBuildState::RootDefaults(mut cursor) => {
                    while remaining != 0 {
                        let Some(slot) = self.next_root_default_slot(&mut cursor) else {
                            let ordered = self.session_mut().begin_ordered_index_image_build();
                            self.state = MachineBuildState::OrderedIndexes(ordered);
                            break;
                        };
                        let mut work = std::mem::take(&mut self.work);
                        let result = self
                            .session_mut()
                            .initialize_root_field_default(&slot, &mut work);
                        self.work = work;
                        result?;
                        self.finish_step(&mut remaining);
                    }
                    if matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::RootDefaults(cursor);
                    }
                }
                MachineBuildState::OrderedIndexes(mut build) => {
                    while remaining != 0 {
                        if build.current.is_none() {
                            let Some(plan) = build.plans.get(build.next_plan).cloned() else {
                                let mut work = std::mem::take(&mut self.work);
                                let prepared = std::mem::take(&mut build.prepared);
                                self.session_mut()
                                    .publish_ordered_index_image(prepared, &mut work);
                                self.work = work;
                                let currentness =
                                    PublishedCurrentnessBuild::new(self.session_mut());
                                self.state = MachineBuildState::PublishedCurrentness(currentness);
                                break;
                            };
                            let mut work = std::mem::take(&mut self.work);
                            let candidate = self
                                .session_mut()
                                .begin_ordered_index_candidate(plan, &mut work)?;
                            self.work = work;
                            build.current = Some(candidate);
                        }

                        let candidate = build.current.as_mut().expect("candidate started");
                        let row = self
                            .session_mut()
                            .lists
                            .get(&candidate.plan.source_list)
                            .and_then(|list| list.order.get(candidate.next_row))
                            .copied();
                        if let Some(row) = row {
                            let mut work = std::mem::take(&mut self.work);
                            let result = self
                                .session_mut()
                                .insert_ordered_index_candidate_row(candidate, row, &mut work);
                            self.work = work;
                            result?;
                            candidate.next_row += 1;
                            self.finish_step(&mut remaining);
                            continue;
                        }

                        if candidate.integrity.is_none() {
                            let index = candidate
                                .index
                                .take()
                                .expect("candidate index exists before integrity validation");
                            candidate.integrity = Some(index.into_integrity_task());
                        }

                        let integrity = candidate
                            .integrity
                            .as_mut()
                            .expect("candidate integrity task started");
                        let completed_before = integrity.progress().completed_steps;
                        let poll = match integrity.poll(remaining) {
                            Ok(poll) => poll,
                            Err(error) => {
                                record_ordered_index_access_error(&mut self.work, &error);
                                return Err(Error::Evaluation(error.to_string()));
                            }
                        };
                        let completed_after = integrity.progress().completed_steps;
                        let completed = completed_after.saturating_sub(completed_before);
                        self.finish_steps(&mut remaining, completed)?;

                        let OrderedIndexIntegrityPoll::Ready(result) = poll else {
                            continue;
                        };
                        let (index, report) = result.into_parts();
                        let plan_id = candidate.plan.id;
                        let expanded = candidate.plan.keys.iter().any(|key| {
                            matches!(
                                key.multiplicity,
                                PlanListIndexKeyMultiplicity::ListItems { .. }
                            )
                        });
                        record_ordered_index_integrity(&mut self.work, report, expanded);
                        build.current.take().expect("completed candidate");
                        let mut work = std::mem::take(&mut self.work);
                        let result = self.session_mut().extend_prepared_ordered_index_image(
                            &mut build.prepared,
                            plan_id,
                            index,
                            report,
                            &mut work,
                        );
                        self.work = work;
                        result?;
                        build.next_plan += 1;
                    }
                    if matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::OrderedIndexes(build);
                    }
                }
                MachineBuildState::PublishedCurrentness(mut build) => {
                    let mut finished = false;
                    while remaining != 0 {
                        let complete = self.poll_published_currentness_step(&mut build)?;
                        self.finish_step(&mut remaining);
                        if complete {
                            self.state = MachineBuildState::Complete;
                            finished = true;
                            break;
                        }
                    }
                    if !finished && matches!(self.state, MachineBuildState::Complete) {
                        self.state = MachineBuildState::PublishedCurrentness(build);
                    }
                }
                MachineBuildState::Failed => {
                    return Err(Error::InvalidPlan(
                        "failed machine build task resumed internally".to_owned(),
                    ));
                }
                MachineBuildState::Complete => {
                    self.work.finish_metrics();
                    let mut session = self.session.take().expect("active machine build");
                    session.startup_metrics = self.work.metrics.clone();
                    session.strip_unleased_producer_resources();
                    return Ok(MachineBuildPoll::Ready(session));
                }
            }
            if remaining == 0 {
                return Ok(MachineBuildPoll::Pending(self.progress()));
            }
        }
    }

    fn session_mut(&mut self) -> &mut MachineInstance {
        self.session.as_mut().expect("active machine build")
    }

    fn finish_step(&mut self, remaining: &mut usize) {
        *remaining -= 1;
        self.completed_steps = self.completed_steps.saturating_add(1);
    }

    fn finish_steps(&mut self, remaining: &mut usize, completed: u64) -> Result<(), Error> {
        let completed = usize::try_from(completed).map_err(|_| {
            Error::Evaluation("machine build progress does not fit this target".to_owned())
        })?;
        if completed > *remaining {
            return Err(Error::Evaluation(
                "machine build task exceeded its poll step budget".to_owned(),
            ));
        }
        *remaining -= completed;
        self.completed_steps = self.completed_steps.saturating_add(completed as u64);
        Ok(())
    }

    fn phase(&self) -> MachineBuildPhase {
        match self.state {
            MachineBuildState::Bootstrap => MachineBuildPhase::Bootstrap,
            MachineBuildState::TranslateDurableRestore(_) => {
                MachineBuildPhase::TranslateDurableRestore
            }
            MachineBuildState::StorageRows(_) => MachineBuildPhase::StorageRows,
            MachineBuildState::IndexedDefaults(_) => MachineBuildPhase::IndexedDefaults,
            MachineBuildState::RestoreAuthority(_) => MachineBuildPhase::RestoreAuthority,
            MachineBuildState::RuntimeState(_) => MachineBuildPhase::RuntimeState,
            MachineBuildState::RootDefaults(_) => MachineBuildPhase::RootDefaults,
            MachineBuildState::OrderedIndexes(_) => MachineBuildPhase::OrderedIndexes,
            MachineBuildState::PublishedCurrentness(_) => MachineBuildPhase::PublishedCurrentness,
            MachineBuildState::Failed => MachineBuildPhase::Failed,
            MachineBuildState::Complete => MachineBuildPhase::Complete,
        }
    }

    fn next_storage_row(
        &mut self,
        cursor: &mut StorageBuildCursor,
    ) -> Result<Option<(ListStorageSlot, u64)>, Error> {
        loop {
            let Some(slot) = self
                .session_mut()
                .plan
                .storage_layout
                .list_slots
                .get(cursor.slot)
                .cloned()
            else {
                return Ok(None);
            };
            self.session_mut().lists.entry(slot.list_id).or_default();
            let replaced_by_restore = self
                .authority
                .as_ref()
                .and_then(|authority| authority.lists.get(&slot.list_id))
                .is_some_and(|list| list.touched);
            let row_count = if replaced_by_restore {
                0
            } else {
                storage_initializer_row_count(&slot)?
            };
            if cursor.row < row_count {
                let row = cursor.row;
                cursor.row += 1;
                return Ok(Some((slot, row)));
            }
            cursor.slot += 1;
            cursor.row = 0;
        }
    }

    fn next_indexed_default_row(&mut self, cursor: &mut IndexedDefaultsCursor) -> Option<RowId> {
        loop {
            let slot = self
                .session_mut()
                .plan
                .storage_layout
                .list_slots
                .get(cursor.slot)
                .cloned()?;
            let row = self
                .session_mut()
                .lists
                .get(&slot.list_id)
                .and_then(|list| list.order.get(cursor.row))
                .copied();
            if let Some(row) = row {
                cursor.row += 1;
                return Some(row);
            }
            cursor.slot += 1;
            cursor.row = 0;
        }
    }

    fn next_root_default_slot(
        &mut self,
        cursor: &mut RootDefaultsCursor,
    ) -> Option<ScalarStorageSlot> {
        loop {
            let slot = self
                .session_mut()
                .plan
                .storage_layout
                .scalar_slots
                .get(cursor.slot)
                .cloned()?;
            cursor.slot += 1;
            if !slot.indexed && matches!(slot.initializer, ScalarInitializerPlan::Expression { .. })
            {
                return Some(slot);
            }
        }
    }

    fn next_list_row(&mut self, cursor: &mut ListRowsCursor) -> Option<RowId> {
        loop {
            let list = self
                .session_mut()
                .plan
                .storage_layout
                .list_slots
                .get(cursor.slot)
                .map(|slot| slot.list_id)?;
            let row = self
                .session_mut()
                .lists
                .get(&list)
                .and_then(|state| state.order.get(cursor.row))
                .copied();
            if let Some(row) = row {
                cursor.row += 1;
                return Some(row);
            }
            cursor.slot += 1;
            cursor.row = 0;
        }
    }

    fn poll_durable_restore_step(
        &mut self,
        build: &mut DurableRestoreBuild,
    ) -> Result<bool, Error> {
        match build.phase {
            DurableRestorePhase::Scalars => {
                let Some((memory_id, scalar)) = build.scalars.next() else {
                    build.phase = DurableRestorePhase::Lists;
                    return Ok(false);
                };
                let state = self
                    .session_mut()
                    .metadata
                    .durable_root_state_by_memory
                    .get(&memory_id)
                    .copied()
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "restore image contains unknown scalar memory {memory_id}"
                        ))
                    })?;
                if !scalar.touched {
                    return Err(Error::InvalidPlan(format!(
                        "sparse restore scalar {memory_id} is present but not touched"
                    )));
                }
                self.work.consume(1)?;
                if build
                    .states
                    .insert(
                        state,
                        ScalarAuthority {
                            touched: true,
                            value: runtime_value(scalar.value)?,
                        },
                    )
                    .is_some()
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore image maps more than one scalar memory to state {}",
                        state.0
                    )));
                }
            }
            DurableRestorePhase::Lists => {
                if let Some(current) = build.current_list.as_mut() {
                    if let Some(row) = current.rows.next() {
                        self.work.consume(1)?;
                        let translated = self.translate_durable_row(current, row)?;
                        current.translated_rows.push(translated);
                    } else {
                        let current = build
                            .current_list
                            .take()
                            .expect("completed durable list translation remains active");
                        let authority = ListAuthority {
                            touched: current.touched,
                            revision: current.revision,
                            next_key: current.next_key,
                            next_order_token: current.next_order_token,
                            rows: current.translated_rows,
                        };
                        if build
                            .translated_lists
                            .insert(current.list_id, authority)
                            .is_some()
                        {
                            return Err(Error::InvalidPlan(format!(
                                "restore image maps more than one list memory to list {}",
                                current.list_id.0
                            )));
                        }
                    }
                } else if let Some((memory_id, list)) = build.lists.next() {
                    build.current_list =
                        Some(self.begin_durable_list_translation(memory_id, list)?);
                } else {
                    build.phase = DurableRestorePhase::Complete;
                }
            }
            DurableRestorePhase::Complete => return Ok(true),
        }
        Ok(false)
    }

    fn begin_durable_list_translation(
        &mut self,
        memory_id: boon_plan::MemoryId,
        list: boon_persistence::StoredList,
    ) -> Result<DurableListTranslateBuild, Error> {
        let metadata = self
            .session_mut()
            .metadata
            .durable_list_by_memory
            .get(&memory_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "restore image contains unknown list memory {memory_id}"
                ))
            })?;
        if !list.touched && list.next_key != 0 {
            return Err(Error::InvalidPlan(format!(
                "sparse row overrides for list {memory_id} must not replace its allocator"
            )));
        }
        Ok(DurableListTranslateBuild {
            memory_id,
            list_id: metadata.list_id,
            touched: list.touched,
            revision: list.revision,
            next_key: list.next_key,
            next_order_token: list.next_order_token,
            rows: list.rows.into_iter(),
            translated_rows: Vec::new(),
        })
    }

    fn translate_durable_row(
        &mut self,
        build: &DurableListTranslateBuild,
        row: boon_persistence::StoredRow,
    ) -> Result<RowAuthority, Error> {
        let metadata = self
            .session_mut()
            .metadata
            .durable_list_by_memory
            .get(&build.memory_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "restore image contains unknown list memory {}",
                    build.memory_id
                ))
            })?;
        let owner_ancestors = self.session_mut().runtime_owner_rows(&row.owner)?;
        let materialization_origin = row
            .materialization_origin
            .as_ref()
            .map(|owner| self.session_mut().runtime_owner_rows(owner))
            .transpose()?;
        if !build.touched
            && (row.touched_fields.is_empty()
                || row
                    .fields
                    .keys()
                    .any(|field| !row.touched_fields.contains(field)))
        {
            return Err(Error::InvalidPlan(format!(
                "sparse restore list {} contains non-override row data",
                build.memory_id
            )));
        }
        let fields = row
            .fields
            .into_iter()
            .map(|(stable, value)| {
                let field = metadata
                    .fields_by_leaf
                    .get(&stable)
                    .copied()
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "restore list {} contains unknown row leaf {stable}",
                            build.memory_id
                        ))
                    })?;
                Ok((field, runtime_value(value)?))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let touched_fields = row
            .touched_fields
            .into_iter()
            .map(|stable| {
                metadata
                    .fields_by_leaf
                    .get(&stable)
                    .copied()
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "restore list {} touches unknown row leaf {stable}",
                            build.memory_id
                        ))
                    })
            })
            .collect::<Result<BTreeSet<_>, Error>>()?;
        let id = RowId {
            list: build.list_id,
            key: row.key,
            generation: row.generation,
        };
        let expected_leaf = OwnerInstanceRow {
            list: id.list,
            key: id.key,
            generation: id.generation,
        };
        if owner_ancestors.last() != Some(&expected_leaf) {
            return Err(Error::InvalidPlan(format!(
                "restore row {}:{} structural owner has a different leaf",
                row.key, row.generation
            )));
        }
        Ok(RowAuthority {
            id,
            source_order_token: if build.touched {
                row.source_order_token
            } else {
                0
            },
            owner_ancestors,
            materialization_origin,
            fields,
            touched_fields,
        })
    }

    fn poll_authority_restore_step(
        &mut self,
        build: &mut AuthorityRestoreBuild,
    ) -> Result<bool, Error> {
        match &mut build.phase {
            AuthorityRestorePhase::Begin => {
                let session = self.session_mut();
                session.turn_sequence = build.through_turn_sequence;
                session.touched_root_states.clear();
                build.phase = AuthorityRestorePhase::States;
            }
            AuthorityRestorePhase::States => {
                let Some((state, scalar)) = build.states.next() else {
                    build.phase = AuthorityRestorePhase::Lists;
                    return Ok(false);
                };
                let session = self.session_mut();
                if !session
                    .plan
                    .storage_layout
                    .scalar_slots
                    .iter()
                    .any(|slot| !slot.indexed && slot.state_id == state)
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore image contains unknown root state {}",
                        state.0
                    )));
                }
                if scalar.touched {
                    session.root_states.insert(state, scalar.value);
                    session.touched_root_states.insert(state);
                }
            }
            AuthorityRestorePhase::Lists => {
                if let Some(current) = build.current_list.as_mut() {
                    if self.poll_authority_list_restore_step(current)? {
                        build.current_list = None;
                    }
                } else if let Some((list, authority)) = build.lists.next() {
                    build.current_list = Some(self.begin_authority_list_restore(list, authority)?);
                } else {
                    build.phase = AuthorityRestorePhase::ValidateRows(ListRowsCursor::default());
                }
            }
            AuthorityRestorePhase::ValidateRows(cursor) => {
                if let Some(row) = self.next_list_row(cursor) {
                    self.session_mut().validate_row_ownership_for_row(row)?;
                } else {
                    build.phase = AuthorityRestorePhase::Complete;
                }
            }
            AuthorityRestorePhase::Complete => return Ok(true),
        }
        Ok(false)
    }

    fn begin_authority_list_restore(
        &mut self,
        list_id: ListId,
        authority: ListAuthority,
    ) -> Result<AuthorityListRestoreBuild, Error> {
        if !self
            .session_mut()
            .plan
            .storage_layout
            .list_slots
            .iter()
            .any(|slot| slot.list_id == list_id)
        {
            return Err(Error::InvalidPlan(format!(
                "restore image contains unknown list {}",
                list_id.0
            )));
        }
        let allowed_fields = self.session_mut().authority_fields_for_list(list_id);
        let ListAuthority {
            touched,
            revision,
            next_key,
            next_order_token,
            rows,
        } = authority;
        let state = if touched {
            AuthorityListRestoreState::ReplacementRows {
                built_rows: BTreeMap::new(),
                order: ListOrder::default(),
                order_tokens: BTreeMap::new(),
                rows: rows.into_iter(),
                next_key,
                next_order_token,
                seen: BTreeSet::new(),
                previous_order_token: 0,
                minimum_next_key: 1,
            }
        } else {
            if next_key != 0 {
                return Err(Error::InvalidPlan(format!(
                    "restore row overrides for list {} replace allocator state",
                    list_id.0
                )));
            }
            AuthorityListRestoreState::SparseRows {
                rows: rows.into_iter(),
                seen: BTreeSet::new(),
            }
        };
        Ok(AuthorityListRestoreBuild {
            list_id,
            revision,
            allowed_fields,
            state,
        })
    }

    fn poll_authority_list_restore_step(
        &mut self,
        build: &mut AuthorityListRestoreBuild,
    ) -> Result<bool, Error> {
        match &mut build.state {
            AuthorityListRestoreState::SparseRows { rows, seen } => {
                let Some(restored_row) = rows.next() else {
                    self.session_mut()
                        .lists
                        .get_mut(&build.list_id)
                        .expect("restored sparse list exists in initialized defaults")
                        .revision = build.revision;
                    build.state = AuthorityListRestoreState::Complete;
                    return Ok(false);
                };
                self.work.consume(1)?;
                if restored_row.id.list != build.list_id || !seen.insert(restored_row.id) {
                    return Err(Error::InvalidPlan(format!(
                        "restore image contains invalid or repeated row {}:{}:{}",
                        restored_row.id.list.0, restored_row.id.key, restored_row.id.generation
                    )));
                }
                if restored_row.touched_fields.is_empty()
                    || restored_row.fields.keys().any(|field| {
                        !build.allowed_fields.contains(field)
                            || !restored_row.touched_fields.contains(field)
                    })
                    || restored_row
                        .touched_fields
                        .iter()
                        .any(|field| !restored_row.fields.contains_key(field))
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore row override {}:{} contains non-authoritative or untouched data",
                        restored_row.id.key, restored_row.id.generation
                    )));
                }
                let session = self.session_mut();
                let row = session
                    .lists
                    .get(&build.list_id)
                    .and_then(|list| list.rows.get(&restored_row.id))
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "restore row override {}:{} does not exist in current defaults",
                            restored_row.id.key, restored_row.id.generation
                        ))
                    })?;
                if row.owner_ancestors != restored_row.owner_ancestors {
                    return Err(Error::InvalidPlan(format!(
                        "restore row override {}:{} changed structural owner",
                        restored_row.id.key, restored_row.id.generation
                    )));
                }
                if row.materialization_origin != restored_row.materialization_origin {
                    return Err(Error::InvalidPlan(format!(
                        "restore row override {}:{} changed materialization origin",
                        restored_row.id.key, restored_row.id.generation
                    )));
                }
                let restored_fields = restored_row.fields;
                let field_ids = restored_fields.keys().copied().collect::<Vec<_>>();
                {
                    let row = session
                        .lists
                        .get_mut(&build.list_id)
                        .and_then(|list| list.rows.get_mut(&restored_row.id))
                        .expect("validated sparse restore row remains present");
                    row.fields.extend(restored_fields);
                }
                for field in field_ids {
                    session.touched_row_fields.insert((restored_row.id, field));
                    session.suspend_row_default(restored_row.id, field)?;
                }
            }
            AuthorityListRestoreState::ReplacementRows {
                rows,
                next_key,
                next_order_token,
                built_rows,
                order,
                order_tokens,
                seen,
                previous_order_token,
                minimum_next_key,
            } => {
                let Some(restored_row) = rows.next() else {
                    if *next_key < *minimum_next_key {
                        return Err(Error::InvalidPlan(format!(
                            "restore list {} next key {} is below required {}",
                            build.list_id.0, *next_key, *minimum_next_key
                        )));
                    }
                    if *next_order_token <= *previous_order_token {
                        return Err(Error::InvalidPlan(format!(
                            "restore list {} next source-order token does not follow its rows",
                            build.list_id.0
                        )));
                    }
                    self.session_mut().lists.insert(
                        build.list_id,
                        ListState::from_validated_authority(
                            std::mem::take(built_rows),
                            std::mem::take(order),
                            std::mem::take(order_tokens),
                            *next_order_token,
                            *next_key,
                            build.revision,
                        )?,
                    );
                    self.session_mut().touched_lists.insert(build.list_id);
                    build.state = AuthorityListRestoreState::InitializeReplacementRows { row: 0 };
                    return Ok(false);
                };
                self.work.consume(1)?;
                if restored_row.id.list != build.list_id {
                    return Err(Error::InvalidPlan(format!(
                        "restore row {}:{} belongs to list {}, expected {}",
                        restored_row.id.key,
                        restored_row.id.generation,
                        restored_row.id.list.0,
                        build.list_id.0
                    )));
                }
                if !seen.insert(restored_row.id) {
                    return Err(Error::InvalidPlan(format!(
                        "restore image repeats row {}:{}:{}",
                        build.list_id.0, restored_row.id.key, restored_row.id.generation
                    )));
                }
                if restored_row.source_order_token == 0
                    || restored_row.source_order_token <= *previous_order_token
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore list {} has non-increasing source-order token for row {}:{}",
                        build.list_id.0, restored_row.id.key, restored_row.id.generation
                    )));
                }
                *previous_order_token = restored_row.source_order_token;
                if let Some(field) = restored_row
                    .fields
                    .keys()
                    .find(|field| !build.allowed_fields.contains(field))
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore row {}:{} contains non-authoritative field {}",
                        build.list_id.0, restored_row.id.key, field.0
                    )));
                }
                if restored_row
                    .touched_fields
                    .iter()
                    .any(|field| !restored_row.fields.contains_key(field))
                {
                    return Err(Error::InvalidPlan(format!(
                        "restore row {}:{} touches a field without a value",
                        build.list_id.0, restored_row.id.key
                    )));
                }
                let row_id = restored_row.id;
                let mut row = Row {
                    owner_ancestors: restored_row.owner_ancestors,
                    materialization_origin: restored_row.materialization_origin,
                    fields: restored_row.fields,
                    ..Row::default()
                };
                let session = self.session_mut();
                for field in session.metadata.row_computations.keys() {
                    if session.metadata.row_field_owner.get(field) == Some(&build.list_id) {
                        row.derived.insert(*field, Currentness::Dirty);
                    }
                }
                session.touched_row_fields.extend(
                    restored_row
                        .touched_fields
                        .into_iter()
                        .map(|field| (row_id, field)),
                );
                let prepared_order = order.prepare_push(row_id, None)?;
                let order_maintenance = prepared_order.maintenance.clone();
                let source_maintenance = SourceOrderMaintenance {
                    storage: order_maintenance.clone(),
                    ..SourceOrderMaintenance::default()
                };
                charge_source_order_maintenance(&mut self.work, &source_maintenance)?;
                prepared_order.commit(order);
                record_source_order_maintenance(&mut self.work, &source_maintenance);
                order_tokens.insert(row_id, restored_row.source_order_token);
                built_rows.insert(row_id, row);
                let required_next_key = row_id.key.checked_add(1).ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "restore list {} exhausts its row-key allocator",
                        build.list_id.0
                    ))
                })?;
                *minimum_next_key = (*minimum_next_key).max(required_next_key);
            }
            AuthorityListRestoreState::InitializeReplacementRows { row } => {
                let row_id = self
                    .session_mut()
                    .lists
                    .get(&build.list_id)
                    .and_then(|list| list.order.get(*row))
                    .copied();
                let Some(row_id) = row_id else {
                    build.state = AuthorityListRestoreState::Complete;
                    return Ok(false);
                };
                let mut work = std::mem::take(&mut self.work);
                let result = self
                    .session_mut()
                    .initialize_missing_indexed_states(row_id, &mut work);
                self.work = work;
                result?;
                *row += 1;
            }
            AuthorityListRestoreState::Complete => return Ok(true),
        }
        Ok(false)
    }

    fn poll_runtime_state_step(&mut self, build: &mut RuntimeStateBuild) -> Result<bool, Error> {
        match &mut build.phase {
            RuntimeStateBuildPhase::ClearOwnerPartitions { list } => {
                let Some(list_id) = self
                    .session_mut()
                    .plan
                    .storage_layout
                    .list_slots
                    .get(*list)
                    .map(|slot| slot.list_id)
                else {
                    build.phase =
                        RuntimeStateBuildPhase::RebuildOwnerPartitions(ListRowsCursor::default());
                    return Ok(false);
                };
                let state = self
                    .session_mut()
                    .lists
                    .get_mut(&list_id)
                    .expect("storage list exists during runtime-state rebuild");
                if state.owner_partitions.pop_first().is_none() {
                    *list += 1;
                }
            }
            RuntimeStateBuildPhase::RebuildOwnerPartitions(cursor) => {
                if let Some(row) = self.next_list_row(cursor) {
                    self.session_mut()
                        .lists
                        .get_mut(&row.list)
                        .expect("row list exists during owner rebuild")
                        .index_owner_partition_row(row)?;
                } else {
                    build.phase = RuntimeStateBuildPhase::ClearDynamicDependencies;
                }
            }
            RuntimeStateBuildPhase::ClearDynamicDependencies => {
                let consumer = self
                    .session_mut()
                    .dynamic_dependencies
                    .by_consumer
                    .keys()
                    .next()
                    .copied();
                if let Some(consumer) = consumer {
                    self.session_mut().clear_consumer_dependencies(consumer);
                } else {
                    build.phase = RuntimeStateBuildPhase::ResetCaches;
                }
            }
            RuntimeStateBuildPhase::ResetCaches => {
                let session = self.session_mut();
                session.ordered_indexes.clear();
                session.dirty_ordered_indexes =
                    session.metadata.list_indexes.keys().copied().collect();
                session.dirty_ordered_index_rows.clear();
                session.distributed_current_call_demands.clear();
                session.next_binding_id = 1;
                let sources = session
                    .metadata
                    .routes
                    .values()
                    .filter(|route| !route.scoped)
                    .map(|route| route.source_id)
                    .collect();
                build.phase = RuntimeStateBuildPhase::RootSourceBindings {
                    sources,
                    next: 0,
                    clearing: true,
                };
            }
            RuntimeStateBuildPhase::RootSourceBindings {
                sources,
                next,
                clearing,
            } => {
                if *clearing {
                    if self
                        .session_mut()
                        .root_source_bindings
                        .pop_first()
                        .is_none()
                    {
                        *clearing = false;
                    }
                } else if let Some(source) = sources.get(*next).copied() {
                    let session = self.session_mut();
                    let binding_epoch = session.next_binding_id;
                    session.next_binding_id =
                        session.next_binding_id.checked_add(1).ok_or_else(|| {
                            Error::Evaluation("source binding epoch overflow".to_owned())
                        })?;
                    session.root_source_bindings.insert(source, binding_epoch);
                    *next += 1;
                } else {
                    build.phase = RuntimeStateBuildPhase::Rows(ListRowsCursor::default());
                }
            }
            RuntimeStateBuildPhase::Rows(cursor) => {
                if let Some(row) = self.next_list_row(cursor) {
                    self.work.consume(1)?;
                    if let Some(state) = self
                        .session_mut()
                        .lists
                        .get_mut(&row.list)
                        .and_then(|list| list.rows.get_mut(&row))
                    {
                        state.bindings.clear();
                        for currentness in state.derived.values_mut() {
                            *currentness = Currentness::Dirty;
                        }
                    }
                    let scope = self
                        .session_mut()
                        .plan
                        .storage_layout
                        .list_slots
                        .iter()
                        .find(|slot| slot.list_id == row.list)
                        .and_then(|slot| slot.scope_id);
                    self.session_mut().bind_row_sources(row, scope)?;
                } else {
                    let fields = self.session_mut().root_fields.keys().copied().collect();
                    build.phase = RuntimeStateBuildPhase::RootFields { fields, next: 0 };
                }
            }
            RuntimeStateBuildPhase::RootFields { fields, next } => {
                if let Some(field) = fields.get(*next).copied() {
                    let cell = self
                        .session_mut()
                        .root_fields
                        .get_mut(&field)
                        .expect("root field inventory remains stable during startup");
                    cell.currentness = Currentness::Dirty;
                    cell.value = None;
                    *next += 1;
                } else {
                    let lists = self.session_mut().derived_lists.keys().copied().collect();
                    build.phase = RuntimeStateBuildPhase::DerivedLists { lists, next: 0 };
                }
            }
            RuntimeStateBuildPhase::DerivedLists { lists, next } => {
                if let Some(list) = lists.get(*next).copied() {
                    let cell = self
                        .session_mut()
                        .derived_lists
                        .get_mut(&list)
                        .expect("derived list inventory remains stable during startup");
                    cell.currentness = Currentness::Dirty;
                    cell.items = None;
                    if let Some(window) = cell.window.as_mut() {
                        window.values_current = false;
                    }
                    *next += 1;
                } else {
                    build.phase = RuntimeStateBuildPhase::Complete;
                }
            }
            RuntimeStateBuildPhase::Complete => return Ok(true),
        }
        Ok(false)
    }

    fn poll_published_currentness_step(
        &mut self,
        build: &mut PublishedCurrentnessBuild,
    ) -> Result<bool, Error> {
        match &mut build.phase {
            PublishedCurrentnessPhase::CollectDirty { next } => {
                if let Some(field) = build.fields.get(*next).copied() {
                    if self
                        .session_mut()
                        .root_fields
                        .get(&field)
                        .is_some_and(|cell| cell.currentness != Currentness::Current)
                    {
                        build.dirty.push(field);
                    }
                    *next += 1;
                    return Ok(false);
                }
                if build.dirty.is_empty() {
                    return Ok(true);
                }
                build.phase = PublishedCurrentnessPhase::EvaluateDirty { next: 0 };
            }
            PublishedCurrentnessPhase::EvaluateDirty { next } => {
                if let Some(field) = build.dirty.get(*next).copied() {
                    let mut work = std::mem::take(&mut self.work);
                    let result = self.session_mut().ensure_root_field(field, None, &mut work);
                    self.work = work;
                    result?;
                    *next += 1;
                    return Ok(false);
                }
                build.completed_passes += 1;
                if build.completed_passes >= build.fields.len().saturating_add(1) {
                    return Err(Error::Evaluation(
                        "published fields did not converge at the currentness barrier".to_owned(),
                    ));
                }
                build.dirty.clear();
                build.phase = PublishedCurrentnessPhase::CollectDirty { next: 0 };
            }
        }
        Ok(false)
    }
}

impl MachineInstance {
    pub fn startup_metrics(&self) -> &TurnMetrics {
        &self.startup_metrics
    }

    #[cfg(test)]
    pub(crate) fn shares_template_metadata(&self, template: &MachineTemplate) -> bool {
        Arc::ptr_eq(&self.metadata, &template.metadata) && Arc::ptr_eq(&self.plan, &template.plan)
    }

    fn fresh_work(&self) -> Work {
        Work::with_limit(self.options.max_work_units_per_transaction)
    }

    fn strip_unleased_producer_resources(&mut self) {
        let ownerships = self
            .metadata
            .producer_function_instances
            .values()
            .map(|instance| instance.ownership.clone())
            .collect::<Vec<_>>();
        for ownership in ownerships {
            self.clear_owned_dynamic_dependencies(&ownership);
            self.remove_owned_resources(&ownership);
        }
    }

    fn clear_owned_dynamic_dependencies(
        &mut self,
        ownership: &boon_plan::ProducerFunctionOwnershipPlan,
    ) {
        for field in &ownership.fields {
            self.clear_consumer_dependencies(Consumer::Root(*field));
        }
        for list in &ownership.lists {
            self.clear_consumer_dependencies(Consumer::List(*list));
            let row_consumers = self
                .lists
                .get(list)
                .into_iter()
                .flat_map(|state| state.rows.iter())
                .flat_map(|(row, value)| {
                    value
                        .derived
                        .keys()
                        .map(|field| Consumer::Row(*row, *field))
                })
                .collect::<Vec<_>>();
            for consumer in row_consumers {
                self.clear_consumer_dependencies(consumer);
            }
        }
    }

    fn remove_owned_resources(&mut self, ownership: &boon_plan::ProducerFunctionOwnershipPlan) {
        for state in &ownership.states {
            self.root_states.remove(state);
            self.touched_root_states.remove(state);
        }
        for field in &ownership.fields {
            self.root_fields.remove(field);
        }
        for list in &ownership.lists {
            self.derived_lists.remove(list);
            self.lists.remove(list);
            self.touched_lists.remove(list);
        }
        for index in &ownership.indexes {
            self.ordered_indexes.remove(index);
            self.dirty_ordered_indexes.remove(index);
            self.dirty_ordered_index_rows.remove(index);
        }
        for source in &ownership.sources {
            self.root_source_bindings.remove(source);
        }
        self.touched_row_fields
            .retain(|(row, _)| !ownership.lists.contains(&row.list));
        self.pending_transient_effects
            .retain(|_, effect| !ownership.effects.contains(&effect.invocation_id));
        let removed = self
            .effect_activations
            .iter()
            .filter_map(|(consumer, activation)| {
                ownership
                    .effects
                    .contains(&activation.invocation_id)
                    .then_some(*consumer)
            })
            .collect::<Vec<_>>();
        for consumer in removed {
            self.effect_activations.remove(&consumer);
            self.clear_consumer_dependencies(Consumer::Effect(consumer));
        }
    }

    fn activate_producer_lease(
        &mut self,
        instance: &ProducerFunctionInstancePlan,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<(), Error> {
        if let Some(active) = &self.active_producer_lease {
            return Err(Error::Evaluation(format!(
                "producer call {} cannot start while call {} is unsettled",
                instance.call_site_id, active.call_site_id
            )));
        }
        let key = ProducerLeaseKey {
            origin: self.machine_origin,
            call_site_id: instance.call_site_id,
            call_owner: instance.owner.static_owner,
            call_instance_id,
        };
        let lease = self.producer_leases.remove(&key);
        let existed = lease.is_some();
        let mut lease = lease.unwrap_or_default();
        if lease.initialized && lease.seen_global_revision != self.global_dependency_revision {
            for cell in lease.root_fields.values_mut() {
                cell.currentness = Currentness::Dirty;
                cell.value = None;
            }
            for cell in lease.derived_lists.values_mut() {
                cell.currentness = Currentness::Dirty;
                cell.items = None;
                if let Some(window) = cell.window.as_mut() {
                    window.values_current = false;
                }
            }
            for list in lease.lists.values_mut() {
                for row in list.rows.values_mut() {
                    for currentness in row.derived.values_mut() {
                        *currentness = Currentness::Dirty;
                    }
                }
            }
            lease.dynamic_dependencies = DynamicDependencies::default();
            lease.distributed_current_call_demands.clear();
        }

        let mut saved_argument_imports = BTreeMap::new();
        let mut saved_argument_revisions = BTreeMap::new();
        for argument in &instance.arguments {
            saved_argument_imports.insert(
                argument.import_id,
                self.distributed_imports.remove(&argument.import_id),
            );
            saved_argument_revisions.insert(
                argument.import_id,
                self.distributed_import_revisions
                    .remove(&argument.import_id),
            );
            self.distributed_imports.insert(
                argument.import_id,
                lease
                    .distributed_imports
                    .remove(&argument.import_id)
                    .unwrap_or(Value::Error {
                        code: "remote_not_current".to_owned(),
                    }),
            );
            if let Some(revision) = lease
                .distributed_import_revisions
                .remove(&argument.import_id)
            {
                self.distributed_import_revisions
                    .insert(argument.import_id, revision);
            }
        }

        self.root_states.extend(lease.root_states);
        self.root_fields.extend(lease.root_fields);
        self.derived_lists.extend(lease.derived_lists);
        self.lists.extend(lease.lists);
        self.ordered_indexes.extend(lease.ordered_indexes);
        self.dirty_ordered_indexes
            .extend(lease.dirty_ordered_indexes);
        self.dirty_ordered_index_rows
            .extend(lease.dirty_ordered_index_rows);
        self.root_source_bindings.extend(lease.root_source_bindings);
        self.touched_root_states.extend(lease.touched_root_states);
        self.touched_lists.extend(lease.touched_lists);
        self.touched_row_fields.extend(lease.touched_row_fields);
        self.pending_transient_effects
            .extend(lease.pending_transient_effects);
        let saved_effect_activations =
            std::mem::replace(&mut self.effect_activations, lease.effect_activations);
        let saved_dynamic_dependencies =
            std::mem::replace(&mut self.dynamic_dependencies, lease.dynamic_dependencies);
        let saved_distributed_current_call_demands = std::mem::replace(
            &mut self.distributed_current_call_demands,
            lease.distributed_current_call_demands,
        );
        let saved_row_owned_call_results = std::mem::replace(
            &mut self.row_owned_call_results,
            lease.row_owned_call_results,
        );
        self.active_producer_lease = Some(ActiveProducerLease {
            key,
            call_site_id: instance.call_site_id,
            ownership: instance.ownership.clone(),
            existed,
            initialized: lease.initialized,
            saved_argument_imports,
            saved_argument_revisions,
            saved_dynamic_dependencies,
            saved_distributed_current_call_demands,
            saved_effect_activations,
            saved_row_owned_call_results,
            producer_result: lease.producer_result,
        });
        Ok(())
    }

    fn finish_active_producer_lease(&mut self, persist: bool) {
        let Some(active) = self.active_producer_lease.take() else {
            return;
        };
        let owned_lists = active
            .ownership
            .lists
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let owned_indexes = active
            .ownership
            .indexes
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let owned_effects = active
            .ownership
            .effects
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut lease = ProducerLeaseState {
            initialized: active.initialized,
            seen_global_revision: self.global_dependency_revision,
            root_states: take_map_where(&mut self.root_states, |state, _| {
                active.ownership.states.contains(state)
            }),
            root_fields: take_map_where(&mut self.root_fields, |field, _| {
                active.ownership.fields.contains(field)
            }),
            derived_lists: take_map_where(&mut self.derived_lists, |list, _| {
                owned_lists.contains(list)
            }),
            lists: take_map_where(&mut self.lists, |list, _| owned_lists.contains(list)),
            ordered_indexes: take_map_where(&mut self.ordered_indexes, |index, _| {
                owned_indexes.contains(index)
            }),
            dirty_ordered_indexes: take_set_where(&mut self.dirty_ordered_indexes, |index| {
                owned_indexes.contains(index)
            }),
            dirty_ordered_index_rows: take_map_where(
                &mut self.dirty_ordered_index_rows,
                |index, _| owned_indexes.contains(index),
            ),
            dynamic_dependencies: std::mem::replace(
                &mut self.dynamic_dependencies,
                active.saved_dynamic_dependencies,
            ),
            distributed_current_call_demands: std::mem::replace(
                &mut self.distributed_current_call_demands,
                active.saved_distributed_current_call_demands,
            ),
            row_owned_call_results: std::mem::replace(
                &mut self.row_owned_call_results,
                active.saved_row_owned_call_results,
            ),
            producer_result: active.producer_result,
            root_source_bindings: take_map_where(&mut self.root_source_bindings, |source, _| {
                active.ownership.sources.contains(source)
            }),
            touched_root_states: take_set_where(&mut self.touched_root_states, |state| {
                active.ownership.states.contains(state)
            }),
            touched_lists: take_set_where(&mut self.touched_lists, |list| {
                owned_lists.contains(list)
            }),
            touched_row_fields: take_set_where(&mut self.touched_row_fields, |(row, _)| {
                owned_lists.contains(&row.list)
            }),
            pending_transient_effects: take_map_where(
                &mut self.pending_transient_effects,
                |_, effect| owned_effects.contains(&effect.invocation_id),
            ),
            effect_activations: std::mem::replace(
                &mut self.effect_activations,
                active.saved_effect_activations,
            ),
            ..ProducerLeaseState::default()
        };
        for argument in self
            .metadata
            .producer_function_instances
            .get(&active.call_site_id)
            .into_iter()
            .flat_map(|instance| &instance.arguments)
        {
            if let Some(value) = self.distributed_imports.remove(&argument.import_id) {
                lease.distributed_imports.insert(argument.import_id, value);
            }
            if let Some(revision) = self
                .distributed_import_revisions
                .remove(&argument.import_id)
            {
                lease
                    .distributed_import_revisions
                    .insert(argument.import_id, revision);
            }
            if let Some(value) = active
                .saved_argument_imports
                .get(&argument.import_id)
                .cloned()
                .flatten()
            {
                self.distributed_imports.insert(argument.import_id, value);
            }
            if let Some(revision) = active
                .saved_argument_revisions
                .get(&argument.import_id)
                .copied()
                .flatten()
            {
                self.distributed_import_revisions
                    .insert(argument.import_id, revision);
            }
        }
        if persist {
            self.producer_leases.insert(active.key, lease);
        }
    }

    fn restore_active_producer_lease(&mut self) {
        let existed = self
            .active_producer_lease
            .as_ref()
            .is_some_and(|active| active.existed);
        self.finish_active_producer_lease(existed);
    }

    fn initialize_active_producer_lease(&mut self, work: &mut Work) -> Result<(), Error> {
        let ownership = self
            .active_producer_lease
            .as_ref()
            .filter(|active| !active.initialized)
            .map(|active| active.ownership.clone());
        let Some(ownership) = ownership else {
            return Ok(());
        };

        let root_sources = self
            .metadata
            .routes
            .values()
            .filter(|route| ownership.sources.contains(&route.source_id) && !route.scoped)
            .map(|route| route.source_id)
            .collect::<Vec<_>>();
        for source in root_sources {
            let binding_epoch = self.next_binding_id;
            self.next_binding_id = self
                .next_binding_id
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("source binding epoch overflow".to_owned()))?;
            self.root_source_bindings.insert(source, binding_epoch);
        }

        for field in ownership.fields.iter().copied() {
            if self.metadata.root_computations.contains_key(&field) {
                self.root_fields.insert(field, DerivedCell::default());
            }
        }
        for list in ownership.lists.iter().copied() {
            if self.metadata.list_computations.contains_key(&list) {
                self.derived_lists.insert(list, DerivedListCell::default());
            }
        }

        let scalar_slots = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| ownership.states.contains(&slot.state_id))
            .cloned()
            .collect::<Vec<_>>();
        for slot in scalar_slots.iter().filter(|slot| {
            !slot.indexed && matches!(slot.initializer, ScalarInitializerPlan::Constant { .. })
        }) {
            let value = self.initial_slot_value(slot)?;
            self.root_states.insert(slot.state_id, value);
        }

        let list_slots = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .filter(|slot| ownership.lists.contains(&slot.list_id))
            .cloned()
            .collect::<Vec<_>>();
        for slot in &list_slots {
            self.initialize_list(slot, work)?;
        }
        let initial_rows = list_slots
            .iter()
            .flat_map(|slot| self.list_row_ids(slot.list_id))
            .collect::<Vec<_>>();
        for row in initial_rows {
            self.initialize_missing_indexed_states(row, work)?;
        }

        for slot in scalar_slots.iter().filter(|slot| {
            !slot.indexed && matches!(slot.initializer, ScalarInitializerPlan::Expression { .. })
        }) {
            let ScalarInitializerPlan::Expression { expression } = &slot.initializer else {
                unreachable!("expression initializer was filtered above")
            };
            let mut bindings = BTreeMap::new();
            let evaluated =
                self.eval_row_expression(expression, None, None, None, None, &mut bindings, work)?;
            let value = self.materialize_eval(evaluated)?;
            self.root_states.insert(slot.state_id, value);
        }

        self.active_producer_lease
            .as_mut()
            .expect("producer lease remained active while initializing")
            .initialized = true;
        Ok(())
    }

    pub fn set_machine_origin(&mut self, origin: MachineOrigin) -> Result<(), Error> {
        if self.active_producer_lease.is_some() || self.turn_work.pending_settle {
            return Err(Error::Evaluation(
                "cannot switch machine origin while a turn is unsettled".to_owned(),
            ));
        }
        self.machine_origin = origin;
        Ok(())
    }

    pub fn reset_machine_origin(&mut self) -> Result<(), Error> {
        self.set_machine_origin(MachineOrigin::LOCAL)
    }

    pub fn drop_producer_origin(
        &mut self,
        origin: MachineOrigin,
    ) -> Result<Vec<TransientEffectCallId>, Error> {
        if self.active_producer_lease.is_some() || self.turn_work.pending_settle {
            return Err(Error::Evaluation(
                "cannot expire producer leases while a turn is unsettled".to_owned(),
            ));
        }
        let keys = self
            .producer_leases
            .keys()
            .copied()
            .filter(|key| key.origin == origin)
            .collect::<Vec<_>>();
        let mut cancelled = Vec::new();
        for key in keys {
            if let Some(lease) = self.producer_leases.remove(&key) {
                cancelled.extend(lease.pending_transient_effects.into_keys());
            }
        }
        cancelled.sort_unstable();
        Ok(cancelled)
    }

    pub fn drop_producer_call_instance_unsettled(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Option<Turn>, Error> {
        if self.active_producer_lease.is_some() || self.turn_work.pending_settle {
            return Err(Error::Evaluation(
                "cannot detach a producer call while a turn is unsettled".to_owned(),
            ));
        }
        let instance = self
            .metadata
            .producer_function_instances
            .get(&call_site_id)
            .ok_or_else(|| {
                Error::InvalidEvent(
                    "distributed producer call site is not declared by this endpoint".to_owned(),
                )
            })?;
        let key = ProducerLeaseKey {
            origin: self.machine_origin,
            call_site_id,
            call_owner: instance.owner.static_owner,
            call_instance_id,
        };
        let sequence = self.next_internal_turn_sequence()?;
        let Some(lease) = self.producer_leases.remove(&key) else {
            return Ok(None);
        };
        let mut work = self.take_internal_turn_work();
        work.cancelled_transient_effects
            .extend(lease.pending_transient_effects.keys().copied());
        work.cancelled_transient_effects.sort_unstable();
        work.detached_producer_leases.insert(key, lease);
        self.turn_sequence = sequence;
        work.finish_metrics();
        let turn = Turn {
            sequence,
            source_sequence: None,
            deltas: Vec::new(),
            authority_deltas: Vec::new(),
            durable_changes: Vec::new(),
            outbox_changes: Vec::new(),
            transient_effects: Vec::new(),
            cancelled_transient_effects: work.cancelled_transient_effects.clone(),
            transient_effect_credit_grants: Vec::new(),
            distributed_invocations: Vec::new(),
            metrics: work.metrics.clone(),
        };
        work.pending_settle = true;
        self.turn_work = work;
        Ok(Some(turn))
    }

    pub fn drop_producer_call_instance(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Option<Turn>, Error> {
        let turn = self.drop_producer_call_instance_unsettled(call_site_id, call_instance_id)?;
        if turn.is_some() {
            self.settle_turn();
        }
        Ok(turn)
    }

    pub fn new(plan: MachinePlan, options: SessionOptions) -> Result<Self, Error> {
        MachineInstanceBuilder::new(plan, options)?.build()
    }

    pub fn new_shared(plan: Arc<MachinePlan>, options: SessionOptions) -> Result<Self, Error> {
        MachineInstanceBuilder::new_shared(plan, options)?.build()
    }

    pub fn set_transient_effect_scope(&mut self, scope: u64) {
        self.transient_effect_scope = scope;
    }

    pub fn plan(&self) -> &MachinePlan {
        &self.plan
    }

    pub fn shared_plan(&self) -> Arc<MachinePlan> {
        Arc::clone(&self.plan)
    }

    pub fn update_session_context(
        &mut self,
        connection_status: SessionConnectionStatus,
        principal: SessionPrincipal,
    ) -> Result<Option<Turn>, Error> {
        self.update_distributed_context(connection_status, principal, Vec::new())
    }

    /// Atomically installs all host-owned distributed context before making
    /// any dependent value current. The batch is validated in full before the
    /// transaction starts; duplicate import IDs fail closed.
    pub fn update_distributed_context(
        &mut self,
        connection_status: SessionConnectionStatus,
        principal: SessionPrincipal,
        import_updates: Vec<DistributedImportUpdate>,
    ) -> Result<Option<Turn>, Error> {
        self.install_distributed_context(
            SessionContext::Available {
                status: connection_status,
                principal,
            },
            import_updates,
            DistributedContextInstall::Patch,
            DistributedContextTurn::Authority,
        )
    }

    /// Replaces the complete distributed execution context. Unlike patch
    /// updates, revisions belong to the replacement context, so a newly
    /// installed context may start again at revision one. Every omitted
    /// declared import becomes `remote_not_current` and loses its revision.
    pub fn replace_distributed_context(
        &mut self,
        session_context: SessionContext,
        import_updates: Vec<DistributedImportUpdate>,
    ) -> Result<Option<Turn>, Error> {
        self.install_distributed_context(
            session_context,
            import_updates,
            DistributedContextInstall::Replace,
            DistributedContextTurn::Authority,
        )
    }

    /// Replaces transient remote inputs for one execution scope without
    /// consuming a durable authority turn sequence. Server origin/global
    /// switching uses this path before a separately admitted source or effect
    /// turn.
    pub fn replace_distributed_execution_context(
        &mut self,
        session_context: SessionContext,
        import_updates: Vec<DistributedImportUpdate>,
    ) -> Result<Option<Turn>, Error> {
        self.install_distributed_context(
            session_context,
            import_updates,
            DistributedContextInstall::Replace,
            DistributedContextTurn::Execution,
        )
    }

    fn install_distributed_context(
        &mut self,
        session_context: SessionContext,
        import_updates: Vec<DistributedImportUpdate>,
        install: DistributedContextInstall,
        turn: DistributedContextTurn,
    ) -> Result<Option<Turn>, Error> {
        let (turn, result) = self.install_distributed_context_with_result(
            session_context,
            import_updates,
            install,
            turn,
            None,
        )?;
        debug_assert!(result.is_none());
        Ok(turn)
    }

    fn install_distributed_context_with_result(
        &mut self,
        session_context: SessionContext,
        import_updates: Vec<DistributedImportUpdate>,
        install: DistributedContextInstall,
        turn: DistributedContextTurn,
        result: Option<(&ValueRef, &DataTypePlan)>,
    ) -> Result<(Option<Turn>, Option<Value>), Error> {
        validate_session_context(&session_context)?;

        let mut seen_imports = BTreeSet::new();
        let mut next_imports = match install {
            DistributedContextInstall::Patch => self.distributed_imports.clone(),
            DistributedContextInstall::Replace => self
                .metadata
                .distributed_import_types
                .keys()
                .copied()
                .map(|import_id| {
                    (
                        import_id,
                        Value::Error {
                            code: "remote_not_current".to_owned(),
                        },
                    )
                })
                .collect(),
        };
        let mut next_revisions = match install {
            DistributedContextInstall::Patch => self.distributed_import_revisions.clone(),
            DistributedContextInstall::Replace => BTreeMap::new(),
        };
        for update in import_updates {
            let DistributedImportUpdate {
                import_id,
                content_revision,
                value,
            } = update;
            if !seen_imports.insert(import_id) {
                return Err(Error::InvalidEvent(
                    "distributed context update contains a duplicate import".to_owned(),
                ));
            }
            let data_type = self
                .metadata
                .distributed_import_types
                .get(&import_id)
                .ok_or_else(|| {
                    Error::InvalidEvent(
                        "distributed import is not declared by this endpoint".to_owned(),
                    )
                })?;
            if content_revision == 0 {
                return Err(Error::InvalidEvent(
                    "distributed import content revision must be positive".to_owned(),
                ));
            }
            validate_distributed_boundary_value(&value, data_type, "distributed import")?;

            if matches!(install, DistributedContextInstall::Patch) {
                let previous_revision = next_revisions.get(&import_id).copied().unwrap_or(0);
                let previous_value = next_imports.get(&import_id);
                if content_revision < previous_revision {
                    return Err(Error::InvalidEvent(format!(
                        "distributed import revision {content_revision} is stale; current revision is {previous_revision}"
                    )));
                }
                if content_revision == previous_revision {
                    if previous_value == Some(&value) {
                        continue;
                    }
                    return Err(Error::InvalidEvent(format!(
                        "distributed import revision {content_revision} conflicts with its current value"
                    )));
                }
            }
            next_imports.insert(import_id, value);
            next_revisions.insert(import_id, content_revision);
        }

        let context_changed = self.options.session_context != session_context;
        let changed_imports = self
            .metadata
            .distributed_import_types
            .keys()
            .copied()
            .filter(|import_id| {
                self.distributed_imports.get(import_id) != next_imports.get(import_id)
                    || self.distributed_import_revisions.get(import_id)
                        != next_revisions.get(import_id)
            })
            .collect::<Vec<_>>();
        let reset_call_results = matches!(install, DistributedContextInstall::Replace);
        let reset_call_result_keys = reset_call_results
            .then(|| {
                self.row_owned_call_results
                    .keys()
                    .copied()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !context_changed && changed_imports.is_empty() && reset_call_result_keys.is_empty() {
            let mut work = self.fresh_work();
            self.initialize_active_producer_lease(&mut work)?;
            let value = match result {
                Some((result, result_type)) => {
                    Some(self.read_distributed_graph_result(result, result_type, &mut work)?)
                }
                None => None,
            };
            if self.active_producer_lease.is_some() {
                work.finish_metrics();
                let turn = Turn {
                    sequence: self.turn_sequence,
                    source_sequence: None,
                    deltas: Vec::new(),
                    authority_deltas: Vec::new(),
                    durable_changes: Vec::new(),
                    outbox_changes: Vec::new(),
                    transient_effects: Vec::new(),
                    cancelled_transient_effects: Vec::new(),
                    transient_effect_credit_grants: Vec::new(),
                    distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                    metrics: std::mem::take(&mut work.metrics),
                };
                work.pending_settle = true;
                self.turn_work = work;
                return Ok((Some(turn), value));
            }
            return Ok((None, value));
        }

        let sequence = match turn {
            DistributedContextTurn::Authority => self.next_internal_turn_sequence()?,
            DistributedContextTurn::Execution => self.turn_sequence,
        };
        let mut work = self.take_internal_turn_work();
        work.distributed_context_undo = Some(DistributedContextUndo {
            session_context: self.options.session_context.clone(),
            imports: changed_imports
                .iter()
                .copied()
                .map(|import_id| {
                    (
                        import_id,
                        (
                            self.distributed_imports.get(&import_id).cloned(),
                            self.distributed_import_revisions.get(&import_id).copied(),
                        ),
                    )
                })
                .collect(),
            row_owned_call_results: reset_call_result_keys
                .iter()
                .copied()
                .map(|key| (key, self.row_owned_call_results.get(&key).cloned()))
                .collect(),
        });
        self.options.session_context = session_context;
        if reset_call_results {
            self.row_owned_call_results.clear();
        }
        for import_id in &changed_imports {
            self.distributed_imports.insert(
                *import_id,
                next_imports
                    .get(import_id)
                    .cloned()
                    .expect("every declared distributed import has a replacement value"),
            );
            match next_revisions.get(import_id).copied() {
                Some(content_revision) => {
                    self.distributed_import_revisions
                        .insert(*import_id, content_revision);
                }
                None => {
                    self.distributed_import_revisions.remove(import_id);
                }
            }
        }

        let result = (|| {
            self.initialize_active_producer_lease(&mut work)?;
            if context_changed {
                self.invalidate_session_info_fields(&mut work);
            }
            for import_id in changed_imports.iter().copied() {
                work.deltas.push(Delta::SetDistributedImport {
                    import_id,
                    value: self
                        .distributed_imports
                        .get(&import_id)
                        .cloned()
                        .expect("updated distributed import exists"),
                });
                self.invalidate_distributed_import(import_id, &mut work);
            }
            for (import_id, call_instance_id) in reset_call_result_keys.iter().copied() {
                self.invalidate_distributed_call_result(import_id, call_instance_id, &mut work);
            }
            self.ensure_published_current(None, &mut work)?;
            let result = match result {
                Some((result, result_type)) => {
                    Some(self.read_distributed_graph_result(result, result_type, &mut work)?)
                }
                None => None,
            };
            self.turn_sequence = sequence;
            work.finish_metrics();
            let turn = Turn {
                sequence,
                source_sequence: None,
                deltas: report_deltas(std::mem::take(&mut work.deltas)),
                authority_deltas: Vec::new(),
                durable_changes: Vec::new(),
                outbox_changes: Vec::new(),
                transient_effects: Vec::new(),
                cancelled_transient_effects: Vec::new(),
                transient_effect_credit_grants: Vec::new(),
                distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok((turn, result))
        })();
        self.finish_internal_turn_work(work, result)
            .map(|(turn, result)| (Some(turn), result))
    }

    fn read_distributed_graph_result(
        &mut self,
        result: &ValueRef,
        result_type: &DataTypePlan,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let call_site_id = self
            .active_producer_lease
            .as_ref()
            .map(|active| active.call_site_id)
            .ok_or_else(|| {
                Error::InvalidPlan(
                    "distributed producer result was read without an active producer lease"
                        .to_owned(),
                )
            })?;
        let consumer = Consumer::ProducerResult(call_site_id);
        self.clear_consumer_dependencies(consumer);
        let evaluated = self.eval_value_ref(result, None, None, None, Some(consumer), work)?;
        let value = self.materialize_eval(evaluated)?;
        validate_distributed_boundary_value(&value, result_type, "distributed function result")?;
        self.active_producer_lease
            .as_mut()
            .expect("producer lease remained active while reading its result")
            .producer_result = Some(value.clone());
        Ok(value)
    }

    fn invalidate_session_info_fields(&mut self, work: &mut Work) {
        let root_fields = self
            .metadata
            .session_info_root_fields
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for field in root_fields {
            self.mark_root_dirty(field, work);
        }
        let row_fields = self
            .metadata
            .session_info_row_fields
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for field in row_fields {
            let Some(list) = self.metadata.row_field_owner.get(&field).copied() else {
                continue;
            };
            for row in self.list_row_ids(list) {
                self.mark_row_dirty(row, field, work);
            }
        }
    }

    pub fn distributed_import_revision(&self, import_id: ImportId) -> Option<u64> {
        self.distributed_import_revisions
            .get(&import_id)
            .copied()
            .or_else(|| {
                let instance =
                    self.metadata
                        .producer_function_instances
                        .values()
                        .find(|instance| {
                            instance
                                .arguments
                                .iter()
                                .any(|argument| argument.import_id == import_id)
                        })?;
                self.producer_leases
                    .iter()
                    .find(|(key, _)| {
                        key.origin == self.machine_origin
                            && key.call_owner == instance.owner.static_owner
                    })?
                    .1
                    .distributed_import_revisions
                    .get(&import_id)
                    .copied()
            })
    }

    pub fn distributed_import_value_current(&self, import_id: ImportId) -> Result<Value, Error> {
        let leased = self
            .metadata
            .producer_function_instances
            .values()
            .find(|instance| {
                instance
                    .arguments
                    .iter()
                    .any(|argument| argument.import_id == import_id)
            })
            .and_then(|instance| {
                self.producer_leases
                    .iter()
                    .find(|(key, _)| {
                        key.origin == self.machine_origin
                            && key.call_owner == instance.owner.static_owner
                    })
                    .map(|(_, lease)| lease)
            })
            .and_then(|lease| lease.distributed_imports.get(&import_id))
            .cloned();
        leased
            .or_else(|| self.distributed_imports.get(&import_id).cloned())
            .ok_or_else(|| {
                Error::InvalidEvent(
                    "distributed import is not declared by this endpoint".to_owned(),
                )
            })
    }

    /// Installs a producer-owned value without fabricating a local SOURCE event.
    /// Revisions are producer-local monotonic content revisions; equal deliveries
    /// are idempotent and stale or conflicting deliveries fail closed.
    pub fn update_distributed_import(
        &mut self,
        import_id: ImportId,
        content_revision: u64,
        value: Value,
    ) -> Result<Option<Turn>, Error> {
        self.install_distributed_context(
            self.options.session_context.clone(),
            vec![DistributedImportUpdate::new(
                import_id,
                content_revision,
                value,
            )],
            DistributedContextInstall::Patch,
            DistributedContextTurn::Authority,
        )
    }

    pub fn distributed_export_value_current(
        &mut self,
        export_id: ExportId,
    ) -> Result<Value, Error> {
        let export = self
            .plan
            .distributed_endpoint
            .as_ref()
            .and_then(|endpoint| {
                endpoint
                    .endpoint
                    .value_exports
                    .iter()
                    .find(|export| export.export_id == export_id)
            })
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent(
                    "distributed value export is not declared by this endpoint".to_owned(),
                )
            })?;
        let mut work = self.fresh_work();
        let evaluated = self.eval_value_ref(&export.value, None, None, None, None, &mut work)?;
        let value = self.materialize_eval(evaluated)?;
        validate_distributed_boundary_value(&value, &export.data_type, "distributed value export")?;
        Ok(value)
    }

    pub fn evaluate_distributed_function_instance_unsettled(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        export_id: ExportId,
        content_revision: u64,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    ) -> Result<(Value, Option<Turn>), Error> {
        let instance = self
            .metadata
            .producer_function_instances
            .get(&call_site_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent(
                    "distributed producer call site is not declared by this endpoint".to_owned(),
                )
            })?;
        if instance.function_export_id != export_id {
            return Err(Error::InvalidEvent(
                "distributed producer call targets a different function export".to_owned(),
            ));
        }
        if content_revision == 0 {
            return Err(Error::InvalidEvent(
                "distributed producer call revision must be positive".to_owned(),
            ));
        }
        if arguments.len() != instance.arguments.len() {
            return Err(Error::InvalidEvent(format!(
                "distributed producer call received {} argument(s), expected {}",
                arguments.len(),
                instance.arguments.len()
            )));
        }
        let mut updates = Vec::with_capacity(instance.arguments.len());
        for parameter in &instance.arguments {
            let value = arguments
                .get(&parameter.argument_id)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidEvent(format!(
                        "distributed producer call is missing argument `{}`",
                        parameter.name
                    ))
                })?;
            validate_distributed_boundary_value(
                &value,
                &parameter.data_type,
                &format!("distributed argument `{}`", parameter.name),
            )?;
            updates.push(DistributedImportUpdate::new(
                parameter.import_id,
                content_revision,
                value,
            ));
        }
        if instance.mode == DistributedCallMode::Invocation {
            return self.evaluate_distributed_invocation_unsettled(
                &instance,
                call_instance_id,
                updates,
            );
        }
        self.activate_producer_lease(&instance, call_instance_id)?;
        let evaluated = self.install_distributed_context_with_result(
            self.options.session_context.clone(),
            updates,
            DistributedContextInstall::Patch,
            DistributedContextTurn::Execution,
            Some((&instance.result, &instance.result_type)),
        );
        match evaluated {
            Ok((turn, value)) => {
                if turn.is_none() {
                    self.finish_active_producer_lease(true);
                }
                Ok((
                    value.expect("producer result was requested from graph installation"),
                    turn,
                ))
            }
            Err(error) => {
                self.restore_active_producer_lease();
                Err(error)
            }
        }
    }

    fn evaluate_distributed_invocation_unsettled(
        &mut self,
        instance: &ProducerFunctionInstancePlan,
        call_instance_id: DistributedCallInstanceId,
        updates: Vec<DistributedImportUpdate>,
    ) -> Result<(Value, Option<Turn>), Error> {
        let source = instance.invocation_source.ok_or_else(|| {
            Error::InvalidPlan(format!(
                "distributed invocation call site {} has no private source",
                instance.call_site_id
            ))
        })?;
        self.activate_producer_lease(instance, call_instance_id)?;
        let mut work = self.take_internal_turn_work();
        let previous_last_sequence = self.last_sequence;
        let previous_turn_sequence = self.turn_sequence;
        let result = (|| {
            let mut seen_imports = BTreeSet::new();
            let mut previous_imports = BTreeMap::new();
            for update in &updates {
                if !seen_imports.insert(update.import_id) {
                    return Err(Error::InvalidEvent(format!(
                        "distributed invocation contains duplicate import {}",
                        update.import_id
                    )));
                }
                let previous_revision = self
                    .distributed_import_revisions
                    .get(&update.import_id)
                    .copied()
                    .unwrap_or(0);
                if update.content_revision <= previous_revision {
                    return Err(Error::InvalidEvent(format!(
                        "distributed invocation import {} sequence {} is not newer than {}",
                        update.import_id, update.content_revision, previous_revision
                    )));
                }
                previous_imports.insert(
                    update.import_id,
                    (
                        self.distributed_imports.get(&update.import_id).cloned(),
                        self.distributed_import_revisions
                            .get(&update.import_id)
                            .copied(),
                    ),
                );
            }
            work.distributed_context_undo = Some(DistributedContextUndo {
                session_context: self.options.session_context.clone(),
                imports: previous_imports,
                row_owned_call_results: BTreeMap::new(),
            });
            for update in &updates {
                self.distributed_imports
                    .insert(update.import_id, update.value.clone());
                self.distributed_import_revisions
                    .insert(update.import_id, update.content_revision);
            }
            self.initialize_active_producer_lease(&mut work)?;
            for update in &updates {
                work.deltas.push(Delta::SetDistributedImport {
                    import_id: update.import_id,
                    value: self
                        .distributed_imports
                        .get(&update.import_id)
                        .cloned()
                        .expect("invocation import was installed"),
                });
                self.invalidate_distributed_import(update.import_id, &mut work);
            }

            let source_sequence = previous_last_sequence
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| {
                    Error::Evaluation("internal source sequence exhausted".to_owned())
                })?;
            let mut event = SourceEvent {
                sequence: source_sequence,
                route: self.source_route_token(source, &[])?,
                source,
                target: None,
                payload: SourcePayload::default(),
            };
            self.validate_event(&event)?;
            self.route_event_with_work(&mut event, &[], &mut work)?;
            let value = self.read_distributed_graph_result(
                &instance.result,
                &instance.result_type,
                &mut work,
            )?;
            let producer_ownership = self
                .active_producer_lease
                .as_ref()
                .map(|lease| lease.ownership.clone())
                .ok_or_else(|| {
                    Error::InvalidPlan(
                        "distributed invocation lost its active producer ownership".to_owned(),
                    )
                })?;
            let durable_changes =
                self.durable_changes_excluding(&work.authority_deltas, Some(&producer_ownership))?;
            self.last_sequence = Some(source_sequence);
            self.turn_sequence = previous_turn_sequence
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))?;
            self.commit_transient_effects(&mut work)?;
            work.finish_metrics();
            let turn = Turn {
                sequence: self.turn_sequence,
                source_sequence: Some(source_sequence),
                deltas: report_deltas(std::mem::take(&mut work.deltas)),
                authority_deltas: report_authority_deltas(std::mem::take(
                    &mut work.authority_deltas,
                )),
                durable_changes,
                outbox_changes: std::mem::take(&mut work.outbox_changes),
                transient_effects: std::mem::take(&mut work.transient_effects),
                cancelled_transient_effects: std::mem::take(&mut work.cancelled_transient_effects),
                transient_effect_credit_grants: std::mem::take(
                    &mut work.transient_effect_credit_grants,
                ),
                distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok((value, turn))
        })();
        match result {
            Ok((value, turn)) => {
                self.turn_work = work;
                Ok((value, Some(turn)))
            }
            Err(error) => {
                work.allow_rollback();
                let rollback = self.rollback_turn(&mut work);
                self.last_sequence = previous_last_sequence;
                self.turn_sequence = previous_turn_sequence;
                self.restore_active_producer_lease();
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(Error::Evaluation(format!(
                        "distributed invocation failed with `{error}` and rollback failed with `{rollback}`"
                    ))),
                }
            }
        }
    }

    pub fn evaluate_distributed_function_instance(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        export_id: ExportId,
        content_revision: u64,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    ) -> Result<Value, Error> {
        let (value, turn) = self.evaluate_distributed_function_instance_unsettled(
            call_site_id,
            call_instance_id,
            export_id,
            content_revision,
            arguments,
        )?;
        if turn.is_some() {
            self.settle_turn();
        }
        Ok(value)
    }

    pub fn distributed_call_instances_current(
        &mut self,
        call_site_id: RemoteCallSiteId,
    ) -> Result<Vec<DistributedCurrentCallInstance>, Error> {
        let call = self
            .plan
            .distributed_endpoint
            .as_ref()
            .and_then(|endpoint| {
                endpoint
                    .endpoint
                    .remote_call_sites
                    .iter()
                    .find(|call| call.call_site_id == call_site_id)
            })
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent("remote call site is not declared by this endpoint".to_owned())
            })?;
        if call.mode != DistributedCallMode::Current {
            return Err(Error::InvalidEvent(
                "remote call site is not a Current call".to_owned(),
            ));
        }
        let mut instances =
            BTreeMap::<DistributedCallInstanceId, DistributedCurrentCallArguments>::new();
        for demands in self.distributed_current_call_demands.values() {
            for ((demanded_site, instance), arguments) in demands {
                if *demanded_site != call_site_id {
                    continue;
                }
                if let Some(existing) = instances.get(instance) {
                    if existing != arguments {
                        return Err(Error::InvalidPlan(
                            "remote call instance has conflicting arguments across retained consumers"
                                .to_owned(),
                        ));
                    }
                } else {
                    instances.insert(*instance, arguments.clone());
                }
            }
        }
        for lease in self.producer_leases.values() {
            for demands in lease.distributed_current_call_demands.values() {
                for ((demanded_site, instance), arguments) in demands {
                    if *demanded_site != call_site_id {
                        continue;
                    }
                    if let Some(existing) = instances.get(instance) {
                        if existing != arguments {
                            return Err(Error::InvalidPlan(
                                "remote call instance has conflicting arguments across producer leases"
                                    .to_owned(),
                            ));
                        }
                    } else {
                        instances.insert(*instance, arguments.clone());
                    }
                }
            }
        }
        Ok(instances
            .into_iter()
            .map(
                |(call_instance_id, arguments)| DistributedCurrentCallInstance {
                    call_instance_id,
                    arguments,
                },
            )
            .collect())
    }

    fn producer_lease_owning_current_call(
        &self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Option<ProducerLeaseKey>, Error> {
        let demand_key = (call_site_id, call_instance_id);
        let mut owners = self
            .producer_leases
            .iter()
            .filter_map(|(lease_key, lease)| {
                (lease_key.origin == self.machine_origin
                    && lease
                        .distributed_current_call_demands
                        .values()
                        .any(|demands| demands.contains_key(&demand_key)))
                .then_some(*lease_key)
            });
        let owner = owners.next();
        if owners.next().is_some() {
            return Err(Error::InvalidPlan(
                "current call instance is owned by multiple producer leases".to_owned(),
            ));
        }
        Ok(owner)
    }

    pub fn distributed_producer_call_result_current(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Value, Error> {
        if let Some(active) = &self.active_producer_lease {
            if active.call_site_id == call_site_id
                && active.key.call_instance_id == call_instance_id
            {
                return active.producer_result.clone().ok_or_else(|| {
                    Error::Evaluation(
                        "distributed producer call instance has no current result".to_owned(),
                    )
                });
            }
        }
        let instance = self
            .metadata
            .producer_function_instances
            .get(&call_site_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent(
                    "distributed producer call site is not declared by this endpoint".to_owned(),
                )
            })?;
        if instance.mode != DistributedCallMode::Current {
            return Err(Error::InvalidEvent(
                "distributed producer call site is not a Current call".to_owned(),
            ));
        }
        let lease_key = ProducerLeaseKey {
            origin: self.machine_origin,
            call_site_id,
            call_owner: instance.owner.static_owner,
            call_instance_id,
        };
        let Some(lease) = self.producer_leases.get(&lease_key) else {
            return Err(Error::InvalidEvent(
                "distributed producer call instance is not active".to_owned(),
            ));
        };
        if self.active_producer_lease.is_some() || self.turn_work.pending_settle {
            return lease.producer_result.clone().ok_or_else(|| {
                Error::Evaluation(
                    "distributed producer call instance has no current result".to_owned(),
                )
            });
        }
        self.activate_producer_lease(&instance, call_instance_id)?;
        let mut work = self.fresh_work();
        let result = (|| {
            self.initialize_active_producer_lease(&mut work)?;
            self.read_distributed_graph_result(&instance.result, &instance.result_type, &mut work)
        })();
        match result {
            Ok(value) => {
                self.finish_active_producer_lease(true);
                Ok(value)
            }
            Err(error) => {
                self.restore_active_producer_lease();
                Err(error)
            }
        }
    }

    pub fn update_distributed_call_result_unsettled(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        content_revision: u64,
        value: Value,
    ) -> Result<Option<Turn>, Error> {
        let call = self
            .plan
            .distributed_endpoint
            .as_ref()
            .and_then(|endpoint| {
                endpoint.endpoint.remote_call_sites.iter().find(|call| {
                    call.call_site_id == call_site_id && call.mode == DistributedCallMode::Current
                })
            })
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent(
                    "current remote call site is not declared by this endpoint".to_owned(),
                )
            })?;
        let owning_lease =
            self.producer_lease_owning_current_call(call_site_id, call_instance_id)?;
        let activated_producer = if let Some(owner) = owning_lease {
            let producer = self
                .metadata
                .producer_function_instances
                .get(&owner.call_site_id)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidPlan(
                        "producer lease references an undeclared call site".to_owned(),
                    )
                })?;
            self.activate_producer_lease(&producer, owner.call_instance_id)?;
            true
        } else {
            false
        };

        let result = (|| {
            let demand_key = (call_site_id, call_instance_id);
            let mut demanded_arguments = None;
            for arguments in self
                .distributed_current_call_demands
                .values()
                .filter_map(|demands| demands.get(&demand_key))
            {
                if demanded_arguments
                    .as_ref()
                    .is_some_and(|existing| existing != arguments)
                {
                    return Err(Error::InvalidPlan(
                        "remote call instance has conflicting current demands".to_owned(),
                    ));
                }
                demanded_arguments = Some(arguments.clone());
            }
            let demanded_arguments = demanded_arguments.ok_or_else(|| {
                Error::InvalidEvent("remote call instance is not currently demanded".to_owned())
            })?;
            let import_id = call.result.current_import_id().ok_or_else(|| {
                Error::InvalidPlan("current remote call site has no result import".to_owned())
            })?;
            if content_revision == 0 {
                return Err(Error::InvalidEvent(
                    "remote call instance revision must be positive".to_owned(),
                ));
            }
            validate_distributed_boundary_value(
                &value,
                &call.result_type,
                "distributed current call result",
            )?;
            let key = (import_id, call_instance_id);
            let previous = self
                .row_owned_call_results
                .get(&key)
                .filter(|previous| previous.arguments == demanded_arguments);
            let previous_revision = previous
                .map(|previous| previous.content_revision)
                .unwrap_or(0);
            if content_revision < previous_revision {
                return Err(Error::InvalidEvent(format!(
                    "remote call instance revision {content_revision} is stale; current revision is {previous_revision}"
                )));
            }
            if content_revision == previous_revision {
                if previous.is_some_and(|previous| previous.value == value) {
                    return Ok(None);
                }
                return Err(Error::InvalidEvent(format!(
                    "remote call instance revision {content_revision} conflicts with its current value"
                )));
            }

            let sequence = self.next_internal_turn_sequence()?;
            let mut work = self.take_internal_turn_work();
            work.distributed_context_undo = Some(DistributedContextUndo {
                session_context: self.options.session_context.clone(),
                imports: BTreeMap::new(),
                row_owned_call_results: BTreeMap::from([(
                    key,
                    self.row_owned_call_results.get(&key).cloned(),
                )]),
            });
            self.row_owned_call_results.insert(
                key,
                DistributedCurrentCallResult {
                    arguments: demanded_arguments,
                    content_revision,
                    value,
                },
            );
            let updated = (|| {
                self.initialize_active_producer_lease(&mut work)?;
                self.invalidate_distributed_call_result(import_id, call_instance_id, &mut work);
                if let Some(producer_call_site_id) = self
                    .active_producer_lease
                    .as_ref()
                    .map(|active| active.call_site_id)
                {
                    let producer = self
                        .metadata
                        .producer_function_instances
                        .get(&producer_call_site_id)
                        .cloned()
                        .ok_or_else(|| {
                            Error::InvalidPlan(
                                "active producer call site is not declared".to_owned(),
                            )
                        })?;
                    self.read_distributed_graph_result(
                        &producer.result,
                        &producer.result_type,
                        &mut work,
                    )?;
                } else {
                    self.ensure_published_current(None, &mut work)?;
                }
                self.turn_sequence = sequence;
                work.finish_metrics();
                let turn = Turn {
                    sequence,
                    source_sequence: None,
                    deltas: report_deltas(std::mem::take(&mut work.deltas)),
                    authority_deltas: Vec::new(),
                    durable_changes: Vec::new(),
                    outbox_changes: Vec::new(),
                    transient_effects: Vec::new(),
                    cancelled_transient_effects: Vec::new(),
                    transient_effect_credit_grants: Vec::new(),
                    distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                    metrics: std::mem::take(&mut work.metrics),
                };
                work.pending_settle = true;
                Ok(turn)
            })();
            self.finish_internal_turn_work(work, updated).map(Some)
        })();

        match result {
            Ok(None) if activated_producer => {
                self.finish_active_producer_lease(true);
                Ok(None)
            }
            Ok(turn) => Ok(turn),
            Err(error) if activated_producer => {
                self.restore_active_producer_lease();
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub fn update_distributed_call_result(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        content_revision: u64,
        value: Value,
    ) -> Result<Option<Turn>, Error> {
        let turn = self.update_distributed_call_result_unsettled(
            call_site_id,
            call_instance_id,
            content_revision,
            value,
        )?;
        if turn.is_some() {
            self.settle_turn();
        }
        Ok(turn)
    }

    fn collect_distributed_invocations_for_trigger(
        &mut self,
        trigger: &TriggerFrame<'_>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let calls = self
            .plan
            .distributed_endpoint
            .as_ref()
            .into_iter()
            .flat_map(|endpoint| &endpoint.endpoint.remote_call_sites)
            .filter(|call| call.mode == DistributedCallMode::Invocation)
            .filter(|call| {
                call.invocation_arms
                    .iter()
                    .any(|arm| Self::trigger_accepts(&arm.trigger, &trigger.active))
            })
            .cloned()
            .collect::<Vec<RemoteCallSitePlan>>();
        for call in calls {
            let mut admitted = false;
            for arm in call
                .invocation_arms
                .iter()
                .filter(|arm| Self::trigger_accepts(&arm.trigger, &trigger.active))
            {
                let mut bindings = BTreeMap::new();
                let gate = self.eval_row_expression(
                    &arm.gate,
                    trigger.active.target,
                    trigger.source_event,
                    None,
                    None,
                    &mut bindings,
                    work,
                )?;
                let gate = self.materialize_eval(gate)?;
                match gate {
                    Value::Bool(false) => continue,
                    Value::Bool(true) if admitted => {
                        return Err(Error::InvalidPlan(
                            "distributed invocation admitted more than one trigger arm".to_owned(),
                        ));
                    }
                    Value::Bool(true) => admitted = true,
                    _value => {
                        return Err(Error::InvalidPlan(
                            "distributed invocation gate produced a non-Bool value".to_owned(),
                        ));
                    }
                }
            }
            if !admitted {
                continue;
            }
            let mut arguments = BTreeMap::new();
            for argument in &call.arguments {
                let mut bindings = BTreeMap::new();
                let evaluated = self.eval_row_expression(
                    &argument.value,
                    trigger.active.target,
                    trigger.source_event,
                    None,
                    None,
                    &mut bindings,
                    work,
                )?;
                let value = self.materialize_eval(evaluated)?;
                validate_distributed_boundary_value(
                    &value,
                    &argument.data_type,
                    &format!("remote invocation argument `{}`", argument.name),
                )?;
                arguments.insert(argument.argument_id, value);
            }
            let (result_source, _) = call.result.invocation_source().ok_or_else(|| {
                Error::InvalidPlan("distributed invocation has no private result source".to_owned())
            })?;
            let result_owner_plan = self
                .metadata
                .routes
                .get(&result_source)
                .map(|route| route.owner.clone())
                .ok_or_else(|| {
                    Error::InvalidPlan(
                        "distributed invocation result source has no route".to_owned(),
                    )
                })?;
            let result_owner = instantiate_plan_owner(&result_owner_plan, &trigger.active)?;
            let result_ancestors = result_owner
                .ancestors
                .iter()
                .map(|row| RowId {
                    list: row.list,
                    key: row.key,
                    generation: row.generation,
                })
                .collect::<Vec<_>>();
            let result_route = self.source_route_token(result_source, &result_ancestors)?;
            let call_instance_id = if call.row_bindings.is_empty() {
                self.distributed_call_instance_id(call.call_site_id, &[])?
            } else {
                let call_owner = instantiate_plan_owner(&call.owner, &trigger.active)?;
                let mut rows = Vec::with_capacity(call.row_bindings.len());
                for binding in &call.row_bindings {
                    let index = call
                        .owner
                        .ancestors
                        .iter()
                        .position(|owner| owner.static_owner == binding.owner)
                        .ok_or_else(|| {
                            Error::InvalidPlan(
                                "distributed invocation row binding is outside its structural owner"
                                    .to_owned(),
                            )
                        })?;
                    let runtime_row = *call_owner.ancestors.get(index).ok_or_else(|| {
                        Error::InvalidPlan(
                            "distributed invocation has no runtime row for a required owner"
                                .to_owned(),
                        )
                    })?;
                    if runtime_row.list != binding.list {
                        return Err(Error::InvalidPlan(
                            "distributed invocation row binding resolved to the wrong list"
                                .to_owned(),
                        ));
                    }
                    rows.push(DistributedCallInstanceRow {
                        owner: binding.owner,
                        local: binding.local,
                        row: runtime_row,
                    });
                }
                self.distributed_call_instance_id(call.call_site_id, &rows)?
            };
            work.distributed_invocations.push(DistributedInvocation {
                call_site_id: call.call_site_id,
                call_instance_id,
                arguments,
                result_route,
            });
        }
        Ok(())
    }

    pub fn list_rows(&self, list: ListId) -> Vec<RowId> {
        if let Some(items) = self
            .derived_lists
            .get(&list)
            .filter(|cell| cell.currentness == Currentness::Current)
            .and_then(|cell| cell.items.as_ref())
        {
            return items.iter().filter_map(eval_row_id).collect();
        }
        self.list_row_ids(list)
    }

    #[cfg(test)]
    pub(crate) fn test_set_row_field(
        &mut self,
        row: RowId,
        field: FieldId,
        value: Value,
    ) -> Result<bool, Error> {
        let mut work = self.fresh_work();
        self.touched_row_fields.insert((row, field));
        self.set_row_authority_field(row, field, value, &mut work)
    }

    #[cfg(test)]
    pub(crate) fn test_ensure_ordered_index_current(
        &mut self,
        index: PlanListIndexId,
    ) -> Result<TurnMetrics, Error> {
        let mut work = self.fresh_work();
        self.ensure_ordered_index_current(index, None, &mut work)?;
        Ok(work.metrics)
    }

    pub fn list_value_current(&mut self, list: ListId) -> Result<Value, Error> {
        self.list_value_current_with_metrics(list)
            .map(|(value, _)| value)
    }

    pub fn list_rows_current(&mut self, list: ListId) -> Result<Vec<RowId>, Error> {
        let value = self.list_value_current(list)?;
        let Value::List(items) = value else {
            return Err(Error::Evaluation(format!(
                "ListId {} did not evaluate to a list",
                list.0
            )));
        };
        Ok(items
            .into_iter()
            .filter_map(|item| match item {
                Value::Row { id, .. } | Value::MappedRow { id, .. } => Some(id),
                _ => None,
            })
            .collect())
    }

    pub fn list_value_current_with_metrics(
        &mut self,
        list: ListId,
    ) -> Result<(Value, TurnMetrics), Error> {
        let mut work = self.fresh_work();
        let value =
            self.eval_value_ref(&ValueRef::List(list), None, None, None, None, &mut work)?;
        let value = self.materialize_eval(value)?.into_visible_facade();
        work.finish_metrics();
        Ok((value, work.metrics))
    }

    pub fn logical_row_count(&self) -> usize {
        self.lists.values().map(|list| list.order.len()).sum()
    }

    pub fn list_row_snapshots(&self, list: ListId) -> Result<Vec<RowSnapshot>, Error> {
        let Some(state) = self.lists.get(&list) else {
            return Ok(Vec::new());
        };
        state
            .order
            .iter()
            .map(|row| self.row_snapshot(*row))
            .collect()
    }

    pub fn list_row_snapshots_current(&mut self, list: ListId) -> Result<Vec<RowSnapshot>, Error> {
        if !self.metadata.list_computations.contains_key(&list) {
            return self.list_row_snapshots(list);
        }
        let mut work = self.fresh_work();
        let items = self.ensure_list_current(list, None, &mut work)?;
        items
            .into_iter()
            .map(|item| match item {
                EvalValue::Row(row) | EvalValue::MappedRow { id: row, .. } => {
                    self.row_snapshot(row)
                }
                _ => Err(Error::Evaluation(format!(
                    "derived list {} contains an item without stable row identity",
                    list.0
                ))),
            })
            .collect()
    }

    pub fn list_logical_len_current(&mut self, list: ListId) -> Result<u64, Error> {
        let mut work = self.fresh_work();
        let logical_len = self.list_logical_len_with_work(list, None, &mut work)?;
        work.finish_metrics();
        Ok(logical_len)
    }

    pub fn list_row_snapshots_window_current(
        &mut self,
        list: ListId,
        range: Range<u64>,
    ) -> Result<(u64, Vec<RowSnapshot>), Error> {
        if range.start > range.end {
            return Err(Error::Evaluation(
                "list row snapshot window has an inverted range".to_owned(),
            ));
        }
        let mut work = self.fresh_work();
        let (logical_len, selected) =
            self.list_eval_window_with_work(list, range, None, None, &mut work)?;
        let rows = selected
            .into_iter()
            .map(|item| match item {
                EvalValue::Row(row) | EvalValue::MappedRow { id: row, .. } => {
                    self.row_snapshot(row)
                }
                _ => Err(Error::Evaluation(format!(
                    "list {} window contains an item without stable row identity",
                    list.0
                ))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        work.finish_metrics();
        Ok((logical_len, rows))
    }

    pub fn row_snapshot(&self, row: RowId) -> Result<RowSnapshot, Error> {
        let state = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "row {}:{}:{} does not exist",
                    row.list.0, row.key, row.generation
                ))
            })?;
        Ok(RowSnapshot {
            id: row,
            fields: state
                .fields
                .iter()
                .filter(|(field, _)| {
                    state
                        .derived
                        .get(field)
                        .is_none_or(|currentness| *currentness == Currentness::Current)
                })
                .map(|(field, value)| (*field, value.clone().into_visible_facade()))
                .collect(),
        })
    }

    pub fn structural_owner_rows(&self, row: RowId) -> Result<Vec<RowId>, Error> {
        self.validate_row_ownership_for_row(row)?;
        Ok(self
            .row_owner_ancestors(row)?
            .iter()
            .map(|owner| RowId {
                list: owner.list,
                key: owner.key,
                generation: owner.generation,
            })
            .collect())
    }

    fn row_materialization_origin(
        &self,
        row: RowId,
    ) -> Result<Option<Vec<OwnerInstanceRow>>, Error> {
        let state = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "row {}:{}:{} has no materialization lineage",
                    row.list.0, row.key, row.generation
                ))
            })?;
        let Some(origin) = state.materialization_origin.clone() else {
            return Ok(None);
        };
        for (depth, owner) in origin.iter().enumerate() {
            let owner_row = RowId {
                list: owner.list,
                key: owner.key,
                generation: owner.generation,
            };
            if self.row_owner_ancestors(owner_row)? != &origin[..=depth] {
                return Err(Error::InvalidPlan(format!(
                    "row {}:{}:{} has stale materialization lineage at depth {depth}",
                    row.list.0, row.key, row.generation
                )));
            }
        }
        Ok(Some(origin))
    }

    fn row_owner_ancestors(&self, row: RowId) -> Result<&[OwnerInstanceRow], Error> {
        self.lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .map(|row| row.owner_ancestors.as_slice())
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "row {}:{}:{} has no structural owner",
                    row.list.0, row.key, row.generation
                ))
            })
    }

    fn validate_row_ownership_for_row(&self, row_id: RowId) -> Result<(), Error> {
        let ancestors = self.row_owner_ancestors(row_id)?;
        let expected_leaf = OwnerInstanceRow {
            list: row_id.list,
            key: row_id.key,
            generation: row_id.generation,
        };
        if ancestors.last() != Some(&expected_leaf) {
            return Err(Error::InvalidPlan(format!(
                "row {}:{}:{} structural owner has a different leaf",
                row_id.list.0, row_id.key, row_id.generation
            )));
        }
        for (depth, ancestor) in ancestors.iter().enumerate() {
            let ancestor_id = RowId {
                list: ancestor.list,
                key: ancestor.key,
                generation: ancestor.generation,
            };
            let actual = self.row_owner_ancestors(ancestor_id)?;
            if actual != &ancestors[..=depth] {
                return Err(Error::InvalidPlan(format!(
                    "row {}:{}:{} has a mixed structural owner at depth {depth}",
                    row_id.list.0, row_id.key, row_id.generation
                )));
            }
        }
        Ok(())
    }

    fn owner_instance_for_row(
        &self,
        plan: &PlanOwner,
        row: RowId,
    ) -> Result<OwnerInstanceId, Error> {
        let ancestors = self.row_owner_ancestors(row)?;
        if ancestors.len() != plan.ancestors.len() {
            return Err(Error::InvalidPlan(format!(
                "row {}:{}:{} has {} owner ancestors, owner {} requires {}",
                row.list.0,
                row.key,
                row.generation,
                ancestors.len(),
                plan.static_owner.0,
                plan.ancestors.len()
            )));
        }
        for (index, (actual, expected)) in ancestors.iter().zip(&plan.ancestors).enumerate() {
            if actual.list != expected.list {
                return Err(Error::InvalidPlan(format!(
                    "row {}:{}:{} owner depth {index} uses list {}, expected {}",
                    row.list.0, row.key, row.generation, actual.list.0, expected.list.0
                )));
            }
        }
        let leaf = ancestors.last().ok_or_else(|| {
            Error::InvalidPlan(format!(
                "row {}:{}:{} is not owned by a repeated scope",
                row.list.0, row.key, row.generation
            ))
        })?;
        if leaf.list != row.list || leaf.key != row.key || leaf.generation != row.generation {
            return Err(Error::InvalidPlan(format!(
                "row {}:{}:{} is not the leaf of its complete owner",
                row.list.0, row.key, row.generation
            )));
        }
        OwnerInstanceId::new(plan.static_owner, ancestors.iter().copied())
            .map_err(|detail| Error::InvalidPlan(detail.to_owned()))
    }

    fn state_owner_instance(
        &self,
        state: StateId,
        origin: &ActiveTrigger,
        row: Option<RowId>,
    ) -> Result<(PlanOwner, OwnerInstanceId), Error> {
        let plan = self
            .metadata
            .state_owners
            .get(&state)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("state {} has no owner plan", state.0)))?;
        let owner = if plan.ancestors.is_empty() {
            OwnerInstanceId::new(plan.static_owner, Vec::new())
                .map_err(|detail| Error::InvalidPlan(detail.to_owned()))?
        } else if origin.owner_plan.ancestors.starts_with(&plan.ancestors)
            && origin.owner.ancestors.len() >= plan.ancestors.len()
        {
            instantiate_plan_owner(&plan, origin)?
        } else {
            self.owner_instance_for_row(
                &plan,
                row.ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "state {} requires a keyed owner but its transition has no exact row",
                        state.0
                    ))
                })?,
            )?
        };
        Ok((plan, owner))
    }

    fn trigger_for_state_target<'a>(
        &self,
        state: StateId,
        origin: &TriggerFrame<'a>,
        row: RowId,
    ) -> Result<TriggerFrame<'a>, Error> {
        let (owner_plan, owner) = self.state_owner_instance(state, &origin.active, Some(row))?;
        Ok(TriggerFrame {
            active: ActiveTrigger {
                cause: origin.active.cause,
                owner_plan,
                owner,
                target: Some(row),
                sequence: origin.active.sequence,
            },
            source_event: origin.source_event,
        })
    }

    pub fn settle_published(&mut self) -> Result<(), Error> {
        let mut work = self.fresh_work();
        self.ensure_published_current(None, &mut work)
    }

    pub fn document_plan(&self) -> Option<&boon_plan::DocumentPlan> {
        self.plan.document_plan()
    }

    pub fn initial_document_patch_batch(&self) -> Option<&boon_plan::DocumentInitialPatchBatch> {
        self.plan.initial_document_patch_batch()
    }

    pub fn document_binding_value_target(
        &self,
        binding: boon_plan::DocumentBindingId,
        row: Option<RowId>,
    ) -> Result<Option<ValueTarget>, Error> {
        let document = self
            .document_plan()
            .ok_or_else(|| Error::InvalidPlan("MachinePlan has no document plan".to_owned()))?;
        let binding = document
            .view_bindings
            .iter()
            .find(|candidate| candidate.id == binding)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("document binding {} does not exist", binding.0))
            })?;
        match binding.target {
            boon_plan::DocumentBindingTarget::State { state } => {
                Ok(Some(ValueTarget::State(state)))
            }
            boon_plan::DocumentBindingTarget::Field { field } => {
                Ok(Some(ValueTarget::Field(field)))
            }
            boon_plan::DocumentBindingTarget::ScopedField { scope, field } => {
                let row = row.ok_or_else(|| {
                    Error::InvalidEvent(format!("document binding {} requires a row", binding.id.0))
                })?;
                let owner = self.metadata.list_by_scope.get(&scope).ok_or_else(|| {
                    Error::InvalidPlan(format!("document scope {} has no owning list", scope.0))
                })?;
                if row.list != *owner {
                    return Err(Error::InvalidEvent(format!(
                        "document binding {} row belongs to list {}, expected {}",
                        binding.id.0, row.list.0, owner.0
                    )));
                }
                Ok(Some(ValueTarget::RowField { row, field }))
            }
            boon_plan::DocumentBindingTarget::Source { .. }
            | boon_plan::DocumentBindingTarget::List { .. }
            | boon_plan::DocumentBindingTarget::Expression { .. } => Ok(None),
        }
    }

    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let mut snapshot = Snapshot {
            states: self
                .root_states
                .iter()
                .map(|(state, value)| (*state, value.clone().into_visible_facade()))
                .collect(),
            fields: BTreeMap::new(),
            lists: BTreeMap::new(),
        };
        for field in &self.metadata.published {
            let cell = self.root_fields.get(field).ok_or_else(|| {
                Error::InvalidPlan(format!("demanded field {} has no computation", field.0))
            })?;
            if cell.currentness != Currentness::Current {
                return Err(Error::Evaluation(format!(
                    "demanded field {} is not current",
                    field.0
                )));
            }
            snapshot.fields.insert(
                *field,
                cell.value
                    .clone()
                    .ok_or_else(|| {
                        Error::Evaluation(format!("demanded field {} has no value", field.0))
                    })?
                    .into_visible_facade(),
            );
        }
        for (list, state) in &self.lists {
            let rows = state
                .order
                .iter()
                .map(|row_id| {
                    let row = state.rows.get(row_id).ok_or_else(|| {
                        Error::Evaluation(format!("list {} order contains a missing row", list.0))
                    })?;
                    Ok(RowSnapshot {
                        id: *row_id,
                        fields: row
                            .fields
                            .iter()
                            .filter(|(field, _)| {
                                row.derived
                                    .get(field)
                                    .is_none_or(|currentness| *currentness == Currentness::Current)
                            })
                            .map(|(field, value)| (*field, value.clone().into_visible_facade()))
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            snapshot.lists.insert(*list, rows);
        }
        Ok(snapshot)
    }

    pub fn authority_snapshot(&self) -> Result<AuthoritySnapshot, Error> {
        let states = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| {
                !slot.indexed
                    && !self
                        .metadata
                        .producer_function_instances
                        .values()
                        .any(|instance| instance.ownership.states.contains(&slot.state_id))
            })
            .map(|slot| {
                let value = self
                    .root_states
                    .get(&slot.state_id)
                    .cloned()
                    .ok_or_else(|| {
                        Error::Evaluation(format!(
                            "authoritative state {} has no value",
                            slot.state_id.0
                        ))
                    })?;
                Ok((
                    slot.state_id,
                    ScalarAuthority {
                        touched: self.touched_root_states.contains(&slot.state_id),
                        value,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;

        let mut lists = BTreeMap::new();
        for slot in self.plan.storage_layout.list_slots.iter().filter(|slot| {
            !self
                .metadata
                .producer_function_instances
                .values()
                .any(|instance| instance.ownership.lists.contains(&slot.list_id))
        }) {
            lists.insert(slot.list_id, self.list_authority(slot.list_id)?);
        }

        Ok(AuthoritySnapshot {
            through_turn_sequence: self.turn_sequence,
            states,
            lists,
        })
    }

    fn list_authority(&self, list_id: ListId) -> Result<ListAuthority, Error> {
        let state = self.lists.get(&list_id).ok_or_else(|| {
            Error::Evaluation(format!("authoritative list {} is missing", list_id.0))
        })?;
        let authority_fields = self.authority_fields_for_list(list_id);
        let rows = state
            .order
            .iter()
            .map(|row_id| {
                let row = state.rows.get(row_id).ok_or_else(|| {
                    Error::Evaluation(format!(
                        "list {} order contains missing row {}:{}",
                        list_id.0, row_id.key, row_id.generation
                    ))
                })?;
                let fields = authority_fields
                    .iter()
                    .filter_map(|field| row.fields.get(field).cloned().map(|value| (*field, value)))
                    .collect();
                let touched_fields = authority_fields
                    .iter()
                    .filter(|field| self.touched_row_fields.contains(&(*row_id, **field)))
                    .copied()
                    .collect();
                Ok(RowAuthority {
                    id: *row_id,
                    source_order_token: state.order_token(*row_id).ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "list {} row {}:{} has no source-order token",
                            list_id.0, row_id.key, row_id.generation
                        ))
                    })?,
                    owner_ancestors: row.owner_ancestors.clone(),
                    materialization_origin: row.materialization_origin.clone(),
                    fields,
                    touched_fields,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(ListAuthority {
            touched: self.touched_lists.contains(&list_id),
            revision: state.revision,
            next_key: state.next_key,
            next_order_token: state.next_order_token,
            rows,
        })
    }

    pub fn durable_restore_image(
        &self,
        epoch: u64,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
    ) -> Result<boon_persistence::RestoreImage, Error> {
        self.restore_image(epoch, completed_migration_edges, false)
    }

    pub fn semantic_value_image(&self) -> Result<boon_persistence::RestoreImage, Error> {
        self.restore_image(0, BTreeSet::new(), true)
    }

    pub fn recovery_image(&self) -> Result<MachineRecoveryImage, Error> {
        if self.turn_work.pending_settle {
            return Err(Error::Evaluation(
                "cannot checkpoint a machine with an unsettled turn".to_owned(),
            ));
        }
        let distributed_imports = self
            .distributed_imports
            .iter()
            .filter(|(import_id, _)| {
                self.metadata
                    .recoverable_distributed_imports
                    .contains(import_id)
            })
            .map(|(import_id, value)| {
                Ok((
                    *import_id,
                    RecoveryDistributedImport {
                        revision: self.distributed_import_revisions.get(import_id).copied(),
                        value: runtime_value_to_data(value)?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        Ok(MachineRecoveryImage {
            authority: self.durable_restore_image(0, BTreeSet::new())?,
            last_source_sequence: self.last_sequence,
            session_context: self.options.session_context.clone(),
            distributed_imports,
        })
    }

    pub fn fork_settled(&self) -> Result<Self, Error> {
        if self.turn_work.pending_settle {
            return Err(Error::Evaluation(
                "cannot fork a machine with an unsettled turn".to_owned(),
            ));
        }
        Ok(self.clone())
    }

    fn restore_image(
        &self,
        epoch: u64,
        completed_migration_edges: BTreeSet<boon_plan::MigrationEdgeId>,
        include_untouched_values: bool,
    ) -> Result<boon_persistence::RestoreImage, Error> {
        let authority = self.authority_snapshot()?;
        let mut scalars = BTreeMap::new();
        for memory in self
            .plan
            .persistence
            .memory
            .iter()
            .filter(|memory| memory.kind == boon_plan::MemoryKind::Scalar)
        {
            let slot = self
                .plan
                .storage_layout
                .scalar_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot && !slot.indexed)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistence scalar {} has no root runtime slot",
                        memory.memory_id
                    ))
                })?;
            let scalar = authority.states.get(&slot.state_id).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "persistence scalar {} has no authority value",
                    memory.memory_id
                ))
            })?;
            if include_untouched_values || scalar.touched {
                scalars.insert(
                    memory.memory_id,
                    boon_persistence::StoredScalar {
                        touched: scalar.touched && !include_untouched_values,
                        value: stored_value(&scalar.value)?,
                    },
                );
            }
        }

        let mut lists = BTreeMap::new();
        for list_memory in &self.plan.persistence.lists {
            let slot = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.id == list_memory.runtime_slot)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "persistence list {} has no runtime slot",
                        list_memory.memory_id
                    ))
                })?;
            let list = authority.lists.get(&slot.list_id).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "persistence list {} has no authority value",
                    list_memory.memory_id
                ))
            })?;
            let durable_structure = self.metadata.durable_lists.contains(&slot.list_id);
            let mut stored = stored_list(
                &self.plan,
                list_memory,
                list,
                !durable_structure || (!include_untouched_values && !list.touched),
            )?;
            if !durable_structure {
                stored.touched = false;
                stored.next_key = 0;
                stored.next_order_token = 0;
                for row in &mut stored.rows {
                    row.source_order_token = 0;
                }
                if stored.rows.is_empty() {
                    continue;
                }
            } else if include_untouched_values {
                stored.touched = false;
                stored.next_key = list.next_key;
                for row in &mut stored.rows {
                    row.touched_fields.clear();
                }
            } else if !stored.touched && stored.rows.is_empty() {
                continue;
            }
            lists.insert(list_memory.memory_id, stored);
        }

        Ok(boon_persistence::RestoreImage {
            application: self.plan.application.identity.clone(),
            schema_version: self.plan.persistence.schema_version,
            schema_hash: self.plan.persistence.schema_hash,
            epoch,
            through_turn_sequence: if include_untouched_values {
                0
            } else {
                authority.through_turn_sequence
            },
            scalars,
            lists,
            completed_migration_edges,
            outbox: BTreeMap::new(),
            content_artifact_manifest: boon_persistence::ContentArtifactManifest::default(),
        })
    }

    pub fn durable_changes(
        &self,
        deltas: &[AuthorityDelta],
    ) -> Result<Vec<boon_persistence::DurableChange>, Error> {
        self.durable_changes_excluding(deltas, None)
    }

    fn durable_changes_excluding(
        &self,
        deltas: &[AuthorityDelta],
        process_local: Option<&boon_plan::ProducerFunctionOwnershipPlan>,
    ) -> Result<Vec<boon_persistence::DurableChange>, Error> {
        deltas
            .iter()
            .filter(|delta| match delta {
                AuthorityDelta::SetRoot { state, .. } => {
                    self.metadata.durable_root_states.contains(state)
                }
                AuthorityDelta::SetRowField { row, field, .. } => self
                    .metadata
                    .durable_row_fields
                    .contains(&(row.list, *field)),
                AuthorityDelta::ReplaceList { list_id, .. } => {
                    self.metadata.durable_lists.contains(list_id)
                }
                AuthorityDelta::InsertRow { row, .. } => {
                    self.metadata.durable_lists.contains(&row.id.list)
                }
                AuthorityDelta::RemoveRow { row, .. } => {
                    self.metadata.durable_lists.contains(&row.list)
                }
            })
            .filter(|delta| {
                process_local
                    .is_none_or(|ownership| !authority_delta_is_producer_local(delta, ownership))
            })
            .map(|delta| match delta {
                AuthorityDelta::SetRoot { state, value } => {
                    let memory = self
                        .plan
                        .persistence
                        .memory
                        .iter()
                        .filter(|memory| memory.kind == boon_plan::MemoryKind::Scalar)
                        .find(|memory| {
                            self.plan.storage_layout.scalar_slots.iter().any(|slot| {
                                slot.id == memory.runtime_slot
                                    && !slot.indexed
                                    && slot.state_id == *state
                            })
                        })
                        .ok_or_else(|| {
                            Error::InvalidPlan(format!(
                                "root state {} has no stable persistence identity",
                                state.0
                            ))
                        })?;
                    Ok(boon_persistence::DurableChange::SetScalar {
                        memory_id: memory.memory_id,
                        value: boon_persistence::StoredScalar {
                            touched: true,
                            value: stored_value(value)?,
                        },
                    })
                }
                AuthorityDelta::SetRowField {
                    row,
                    owner_ancestors,
                    materialization_origin,
                    field,
                    value,
                } => {
                    let memory = self.persistence_list(row.list)?;
                    let fields = stable_list_fields(memory)?;
                    let field_id = fields.get(field).copied().ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "list {} field {} has no stable persistence identity",
                            row.list.0, field.0
                        ))
                    })?;
                    Ok(boon_persistence::DurableChange::SetRowField {
                        memory_id: memory.memory_id,
                        list_revision: self
                            .lists
                            .get(&row.list)
                            .ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "durable row field references missing list {}",
                                    row.list.0
                                ))
                            })?
                            .revision,
                        row_key: row.key,
                        row_generation: row.generation,
                        owner: durable_owner_for_rows(&self.plan, owner_ancestors)?,
                        materialization_origin: materialization_origin
                            .as_ref()
                            .map(|origin| durable_owner_for_rows(&self.plan, origin))
                            .transpose()?,
                        field_id,
                        value: stored_value(value)?,
                    })
                }
                AuthorityDelta::ReplaceList { list_id, authority } => {
                    let memory = self.persistence_list(*list_id)?;
                    if !authority.touched {
                        return Err(Error::InvalidPlan(format!(
                            "replacement authority for list {} is not structurally touched",
                            list_id.0
                        )));
                    }
                    Ok(boon_persistence::DurableChange::SetList {
                        memory_id: memory.memory_id,
                        value: stored_list(&self.plan, memory, authority, false)?,
                    })
                }
                AuthorityDelta::InsertRow {
                    row,
                    index,
                    next_key,
                } => {
                    let memory = self.persistence_list(row.id.list)?;
                    let list = self.lists.get(&row.id.list).ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "durable row insertion references missing list {}",
                            row.id.list.0
                        ))
                    })?;
                    Ok(boon_persistence::DurableChange::InsertRow {
                        memory_id: memory.memory_id,
                        list_revision: list.revision,
                        index: *index,
                        row: stored_row(&self.plan, memory, row, false)?,
                        next_key: *next_key,
                        next_order_token: list.next_order_token,
                    })
                }
                AuthorityDelta::RemoveRow { row, next_key } => {
                    let memory = self.persistence_list(row.list)?;
                    let list = self.lists.get(&row.list).ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "durable row removal references missing list {}",
                            row.list.0
                        ))
                    })?;
                    Ok(boon_persistence::DurableChange::RemoveRow {
                        memory_id: memory.memory_id,
                        list_revision: list.revision,
                        row_key: row.key,
                        row_generation: row.generation,
                        next_key: *next_key,
                        next_order_token: list.next_order_token,
                    })
                }
            })
            .collect()
    }

    fn persistence_list(&self, list_id: ListId) -> Result<&boon_plan::ListMemoryPlan, Error> {
        self.plan
            .persistence
            .lists
            .iter()
            .find(|memory| {
                self.plan
                    .storage_layout
                    .list_slots
                    .iter()
                    .any(|slot| slot.id == memory.runtime_slot && slot.list_id == list_id)
            })
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "list {} has no stable persistence identity",
                    list_id.0
                ))
            })
    }

    fn validate_durable_restore_header(
        &self,
        image: &boon_persistence::RestoreImage,
    ) -> Result<(), Error> {
        if image.application != self.plan.application.identity {
            return Err(Error::InvalidPlan(
                "restore image application identity does not match MachinePlan".to_owned(),
            ));
        }
        if image.schema_version != self.plan.persistence.schema_version
            || image.schema_hash != self.plan.persistence.schema_hash
        {
            return Err(Error::InvalidPlan(
                "restore image schema does not match MachinePlan; migration activation is required"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn authority_fields_for_list(&self, list: ListId) -> BTreeSet<FieldId> {
        let indexed = self
            .metadata
            .indexed_state_field
            .iter()
            .filter_map(|(state, field)| {
                (self.metadata.indexed_state_owner.get(state) == Some(&list)).then_some(*field)
            });
        let constructor = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .into_iter()
            .flat_map(|slot| &slot.row_fields)
            .filter(|field| field.role.is_authority())
            .map(|field| field.field_id);
        constructor.chain(indexed).collect()
    }

    pub fn project_current(
        &mut self,
        targets: &[ValueTarget],
    ) -> Result<BTreeMap<ValueTarget, Value>, Error> {
        let mut values = BTreeMap::new();
        let mut work = self.fresh_work();
        for target in targets {
            let value = match *target {
                ValueTarget::State(state) => self.root_states.get(&state).cloned(),
                ValueTarget::Field(field) => {
                    if !self.metadata.published.contains(&field) {
                        return Err(Error::NotDemanded(field));
                    }
                    Some(self.ensure_root_field(field, None, &mut work)?)
                }
                ValueTarget::RowField { row, field } => {
                    let field = self.resolve_row_field_alias(row, field);
                    if self.metadata.row_computations.contains_key(&field) {
                        Some(self.ensure_row_field(row, field, None, &mut work)?)
                    } else {
                        Some(self.row_value(row, field)?)
                    }
                }
            };
            if let Some(value) = value {
                values.insert(*target, value.into_visible_facade());
            }
        }
        Ok(values)
    }

    /// Evaluates a compiler-owned typed expression against current machine
    /// state. This is the shared bridge used by retained outputs; it does not
    /// create a second expression or collection evaluator in the host.
    pub fn evaluate_plan_expression_current(
        &mut self,
        expression: PlanRowExpressionId,
        row: Option<RowId>,
        bindings: &[ExpressionLocalBinding],
    ) -> Result<Value, Error> {
        let mut locals = PlanLocalBindings::new();
        for binding in bindings {
            if locals
                .insert(
                    (binding.owner, binding.local),
                    EvalValue::Value(binding.value.clone()),
                )
                .is_some()
            {
                return Err(Error::InvalidPlan(format!(
                    "runtime expression repeats local binding {}:{}",
                    binding.owner.0, binding.local.0
                )));
            }
        }
        let mut work = self.fresh_work();
        let evaluated =
            self.eval_row_expression(expression, row, None, None, None, &mut locals, &mut work)?;
        self.materialize_eval(evaluated)
            .map(Value::into_visible_facade)
    }

    /// Establishes a currentness barrier for an already-owned demand set
    /// without cloning its values. Hosts use this after rollback/restore before
    /// exposing retained output state again.
    pub fn ensure_current(&mut self, targets: &[ValueTarget]) -> Result<(), Error> {
        let mut work = self.fresh_work();
        self.ensure_demanded_current(targets, None, &mut work)
    }

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, Error> {
        self.root_value_current_complete(name)
            .map(Value::into_visible_facade)
    }

    pub fn root_value_current_with_metrics(
        &mut self,
        name: &str,
    ) -> Result<(Value, TurnMetrics), Error> {
        let field = unique_root_name(&self.metadata.root_field_by_exact_name, name, "field")?
            .ok_or_else(|| Error::InvalidPlan(format!("no root field `{name}`")))?;
        let mut work = self.fresh_work();
        let value = self
            .ensure_root_field(field, None, &mut work)?
            .into_visible_facade();
        work.finish_metrics();
        Ok((value, work.metrics))
    }

    fn root_value_current_complete(&mut self, name: &str) -> Result<Value, Error> {
        if let Some(field) =
            unique_root_name(&self.metadata.root_field_by_exact_name, name, "field")?
        {
            let mut work = self.fresh_work();
            return self.ensure_root_field(field, None, &mut work);
        }
        if let Some(state) =
            unique_root_name(&self.metadata.root_state_by_exact_name, name, "state")?
        {
            return self
                .root_states
                .get(&state)
                .cloned()
                .ok_or_else(|| Error::Evaluation(format!("root state `{name}` has no value")));
        }
        if !name.starts_with("store.") {
            let qualified = format!("store.{name}");
            if let Some(field) =
                unique_root_name(&self.metadata.root_field_by_exact_name, &qualified, "field")?
            {
                let mut work = self.fresh_work();
                return self.ensure_root_field(field, None, &mut work);
            }
            if let Some(state) =
                unique_root_name(&self.metadata.root_state_by_exact_name, &qualified, "state")?
            {
                return self.root_states.get(&state).cloned().ok_or_else(|| {
                    Error::Evaluation(format!("root state `{qualified}` has no value"))
                });
            }
        }

        let local = local_name(name);
        let fields = self.metadata.root_field_by_name.get(local);
        let states = self.metadata.root_state_by_name.get(local);
        match (fields.map(Vec::as_slice), states.map(Vec::as_slice)) {
            (Some([field]), None) => {
                let field = *field;
                let mut work = self.fresh_work();
                self.ensure_root_field(field, None, &mut work)
            }
            (None, Some([state])) => self
                .root_states
                .get(state)
                .cloned()
                .ok_or_else(|| Error::Evaluation(format!("root state `{name}` has no value"))),
            (None, None) => Err(Error::InvalidPlan(format!("no root value `{name}`"))),
            _ => Err(Error::InvalidPlan(format!(
                "root value name `{name}` is ambiguous"
            ))),
        }
    }

    /// Reconstructs one host-owned non-visual output after establishing its
    /// currentness barrier. Output values are derived cache entries, never
    /// authority included in snapshots or durable storage.
    pub fn output_value_current(&mut self, name: &str) -> Result<Value, Error> {
        let output = self
            .plan
            .output_root(name)
            .cloned()
            .ok_or_else(|| Error::Evaluation(format!("output root `{name}` does not exist")))?;
        let boon_plan::OutputContractKind::HostValue { data_type } = output.contract else {
            return Err(Error::Evaluation(format!(
                "output root `{name}` is retained visual content, not a host data value"
            )));
        };
        let boon_plan::OutputValueRef::RuntimeValue { value, list_fields } = output.value else {
            return Err(Error::InvalidPlan(format!(
                "host output root `{name}` has no runtime value reference"
            )));
        };
        if let (ValueRef::List(list), boon_plan::DataTypePlan::List { item }) = (&value, &data_type)
        {
            return self.output_list_current(*list, item, &list_fields);
        }
        let mut work = self.fresh_work();
        let evaluated = self.eval_value_ref(&value, None, None, None, None, &mut work)?;
        let value = self.materialize_eval(evaluated)?;
        normalize_host_output_value(value)
    }

    fn output_list_current(
        &mut self,
        list: ListId,
        item_type: &boon_plan::DataTypePlan,
        fields: &[OutputListFieldRef],
    ) -> Result<Value, Error> {
        let mut work = self.fresh_work();
        self.materialize_typed_list(list, item_type, fields, None, &mut work)
    }

    fn materialize_typed_list(
        &mut self,
        list: ListId,
        item_type: &boon_plan::DataTypePlan,
        output_fields: &[OutputListFieldRef],
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        if self.metadata.list_computations.contains_key(&list) {
            let items = self.ensure_list_current(list, event, work)?;
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(self.materialize_typed_list_item(
                    item,
                    item_type,
                    output_fields,
                    event,
                    None,
                    work,
                )?);
            }
            return Ok(Value::List(values));
        }
        if !matches!(
            item_type,
            boon_plan::DataTypePlan::Record { open: false, .. }
        ) {
            let value_field = output_list_field(output_fields, list, "value")?;
            let rows = self.list_row_ids(list);
            work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
            let values = rows
                .into_iter()
                .map(|row| {
                    let value = if self.metadata.row_computations.contains_key(&value_field) {
                        self.ensure_row_field(row, value_field, event, work)?
                    } else {
                        self.row_value(row, value_field)?
                    };
                    normalize_scalar_list_item(value, item_type)
                })
                .collect::<Result<Vec<_>, Error>>()?;
            return Ok(Value::List(values));
        }
        let boon_plan::DataTypePlan::Record {
            fields,
            open: false,
        } = item_type
        else {
            return Err(Error::Evaluation(format!(
                "list output {} must expose a closed record item type",
                list.0
            )));
        };
        let rows = self.list_row_ids(list);
        let mut values = Vec::with_capacity(rows.len());
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        for row in rows {
            let mut record = BTreeMap::new();
            for output_field in fields {
                let field = output_list_field(output_fields, list, &output_field.name)?;
                let value = if self.metadata.row_computations.contains_key(&field) {
                    self.ensure_row_field(row, field, event, work)?
                } else {
                    self.row_value(row, field)?
                };
                record.insert(
                    output_field.name.clone(),
                    normalize_host_output_value(value)?,
                );
            }
            values.push(Value::Record(record));
        }
        Ok(Value::List(values))
    }

    fn materialize_typed_list_item(
        &mut self,
        item: EvalValue,
        item_type: &boon_plan::DataTypePlan,
        output_fields: &[OutputListFieldRef],
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let boon_plan::DataTypePlan::Record {
            fields,
            open: false,
        } = item_type
        else {
            return normalize_host_output_value(self.materialize_eval(item)?);
        };
        match item {
            EvalValue::Row(row) => {
                let mut record = BTreeMap::new();
                for output_field in fields {
                    let field = output_list_field(output_fields, row.list, &output_field.name)?;
                    self.register_row_dependency(consumer, row, field);
                    let value = if self.metadata.row_computations.contains_key(&field) {
                        self.ensure_row_field(row, field, event, work)?
                    } else {
                        self.row_value(row, field)?
                    };
                    record.insert(
                        output_field.name.clone(),
                        normalize_host_output_value(value)?,
                    );
                }
                Ok(Value::Record(record))
            }
            EvalValue::MappedRow {
                id: row,
                fields: mut mapped,
                ..
            } => {
                let mut record = BTreeMap::new();
                for output_field in fields {
                    let value = if let Some(value) = mapped.remove(&output_field.name) {
                        self.materialize_eval(value)?
                    } else {
                        let field = output_list_field(output_fields, row.list, &output_field.name)?;
                        self.register_row_dependency(consumer, row, field);
                        if self.metadata.row_computations.contains_key(&field) {
                            self.ensure_row_field(row, field, event, work)?
                        } else {
                            self.row_value(row, field)?
                        }
                    };
                    record.insert(
                        output_field.name.clone(),
                        normalize_host_output_value(value)?,
                    );
                }
                Ok(Value::Record(record))
            }
            EvalValue::Record(mut mapped) => {
                let mut record = BTreeMap::new();
                for output_field in fields {
                    let value = mapped.remove(&output_field.name).ok_or_else(|| {
                        Error::Evaluation(format!(
                            "derived list record is missing field `{}`",
                            output_field.name
                        ))
                    })?;
                    record.insert(
                        output_field.name.clone(),
                        normalize_host_output_value(self.materialize_eval(value)?)?,
                    );
                }
                Ok(Value::Record(record))
            }
            EvalValue::Value(Value::Record(record)) => {
                normalize_host_output_value(Value::Record(record))
            }
            value => Err(Error::Evaluation(format!(
                "derived list record output received {value:?}"
            ))),
        }
    }

    fn eval_typed_effect_expression(
        &mut self,
        expression: PlanRowExpressionId,
        data_type: &boon_plan::DataTypePlan,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        consumer: Consumer,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let value = self.eval_row_expression(
            expression,
            row.or_else(|| event.and_then(|event| event.target)),
            event,
            None,
            Some(consumer),
            &mut PlanLocalBindings::new(),
            work,
        )?;
        self.materialize_typed_effect_value(value, data_type, event, consumer, work)
    }

    fn materialize_typed_effect_value(
        &mut self,
        value: EvalValue,
        data_type: &boon_plan::DataTypePlan,
        event: Option<&SourceEvent>,
        consumer: Consumer,
        work: &mut Work,
    ) -> Result<Value, Error> {
        if let boon_plan::DataTypePlan::List { item } = data_type {
            return eval_to_list(value)?
                .into_iter()
                .map(|value| {
                    self.materialize_typed_effect_value(value, item, event, consumer, work)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List);
        }
        if matches!(&value, EvalValue::Value(value) if value.contains_host_binding()) {
            return normalize_effect_intent_value(self.materialize_eval(value)?);
        }
        if matches!(
            data_type,
            boon_plan::DataTypePlan::Record { open: false, .. }
        ) {
            return self.materialize_typed_list_item(
                value,
                data_type,
                &[],
                event,
                Some(consumer),
                work,
            );
        }
        normalize_effect_intent_value(self.materialize_eval(value)?)
    }

    pub fn inspect_value_current(&mut self, name: &str, max_rows: usize) -> Result<Value, Error> {
        if let Some((list, field)) = self.metadata.exact_list_field(name)? {
            return self.inspect_list_field_current(list, field, max_rows);
        }
        self.root_value_current(name)
    }

    pub fn inspect_list_field_current(
        &mut self,
        list: ListId,
        field: FieldId,
        max_rows: usize,
    ) -> Result<Value, Error> {
        let mut work = self.fresh_work();
        if self.metadata.list_computations.contains_key(&list) {
            self.ensure_list_current(list, None, &mut work)?;
        }
        let rows = self
            .lists
            .get(&list)
            .map(|state| {
                state
                    .order
                    .iter()
                    .copied()
                    .take(max_rows)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            let value = if self.metadata.row_computations.contains_key(&field) {
                self.ensure_row_field(row, field, None, &mut work)?
            } else {
                self.row_value(row, field)?
            };
            values.push(Value::Record(BTreeMap::from([
                ("key".to_owned(), Value::integer(row.key as i64)?),
                (
                    "generation".to_owned(),
                    Value::integer(row.generation as i64)?,
                ),
                ("value".to_owned(), value.into_visible_facade()),
            ])));
        }
        Ok(Value::List(values))
    }

    pub fn row_target_for_source(
        &self,
        source: SourceId,
        key: u64,
        generation: u64,
    ) -> Result<RowId, Error> {
        let route = self.metadata.routes.get(&source).ok_or_else(|| {
            Error::InvalidEvent(format!("source {} is not in the plan", source.0))
        })?;
        let scope = route
            .scope_id
            .ok_or_else(|| Error::InvalidEvent(format!("source {} is not row-scoped", source.0)))?;
        let list = *self.metadata.list_by_scope.get(&scope).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "source {} scope {} has no owning list",
                source.0, scope.0
            ))
        })?;
        let row = RowId {
            list,
            key,
            generation,
        };
        if !self
            .lists
            .get(&list)
            .is_some_and(|list| list.rows.contains_key(&row))
        {
            return Err(Error::InvalidEvent(format!(
                "source {} target row {}:{}:{} does not exist",
                source.0, list.0, key, generation
            )));
        }
        Ok(row)
    }

    pub fn source_route_token(
        &self,
        source: SourceId,
        ancestors: &[RowId],
    ) -> Result<SourceRouteToken, Error> {
        let route = self.metadata.routes.get(&source).ok_or_else(|| {
            Error::InvalidEvent(format!("source {} is not in the plan", source.0))
        })?;
        if route.owner.ancestors.len() != ancestors.len() {
            return Err(Error::InvalidEvent(format!(
                "source {} requires {} owner rows, received {}",
                source.0,
                route.owner.ancestors.len(),
                ancestors.len()
            )));
        }
        let mut owner_rows = Vec::with_capacity(ancestors.len());
        for (expected, row) in route.owner.ancestors.iter().zip(ancestors.iter().copied()) {
            if row.list != expected.list {
                return Err(Error::InvalidEvent(format!(
                    "source {} owner row list {} does not match plan list {}",
                    source.0, row.list.0, expected.list.0
                )));
            }
            if !self
                .lists
                .get(&row.list)
                .is_some_and(|list| list.rows.contains_key(&row))
            {
                return Err(Error::InvalidEvent(format!(
                    "source {} owner row {}:{}:{} does not exist",
                    source.0, row.list.0, row.key, row.generation
                )));
            }
            owner_rows.push(OwnerInstanceRow {
                list: row.list,
                key: row.key,
                generation: row.generation,
            });
        }
        let binding_epoch = if let Some(row) = ancestors.last().copied() {
            self.lists
                .get(&row.list)
                .and_then(|list| list.rows.get(&row))
                .and_then(|row| row.bindings.get(&source))
                .copied()
                .ok_or_else(|| {
                    Error::InvalidEvent(format!(
                        "source {} is not bound to owner row {}:{}:{}",
                        source.0, row.list.0, row.key, row.generation
                    ))
                })?
        } else {
            self.root_source_bindings
                .get(&source)
                .copied()
                .ok_or_else(|| {
                    Error::InvalidEvent(format!("source {} has no root binding", source.0))
                })?
        };
        let owner = OwnerInstanceId::new(route.owner.static_owner, owner_rows)
            .map_err(|detail| Error::InvalidPlan(detail.to_owned()))?;
        SourceRouteToken::new(self.options.program_revision, owner, source, binding_epoch)
            .map_err(|detail| Error::InvalidEvent(detail.to_owned()))
    }

    pub fn source_route_token_for_descendant_row(
        &self,
        source: SourceId,
        row: RowId,
    ) -> Result<SourceRouteToken, Error> {
        let route = self.metadata.routes.get(&source).ok_or_else(|| {
            Error::InvalidEvent(format!("source {} is not in the plan", source.0))
        })?;
        let mut candidates = Vec::new();
        let mut pending = vec![row];
        let mut visited = BTreeSet::new();
        while let Some(candidate_row) = pending.pop() {
            if !visited.insert(candidate_row) {
                return Err(Error::InvalidPlan(format!(
                    "row {}:{}:{} materialization provenance contains a cycle",
                    row.list.0, row.key, row.generation
                )));
            }
            let structural = self.structural_owner_rows(candidate_row)?;
            if !candidates.contains(&structural) {
                candidates.push(structural);
            }
            let Some(origin) = self.row_materialization_origin(candidate_row)? else {
                continue;
            };
            let origin = origin
                .into_iter()
                .map(|owner| RowId {
                    list: owner.list,
                    key: owner.key,
                    generation: owner.generation,
                })
                .collect::<Vec<_>>();
            if let Some(leaf) = origin.last().copied() {
                pending.push(leaf);
            }
            if !candidates.contains(&origin) {
                candidates.push(origin);
            }
        }
        let mut matching = candidates
            .into_iter()
            .filter(|candidate| route.owner.ancestors.len() <= candidate.len())
            .filter_map(|candidate| {
                let route_rows = candidate[..route.owner.ancestors.len()].to_vec();
                route
                    .owner
                    .ancestors
                    .iter()
                    .zip(&route_rows)
                    .all(|(expected, actual)| expected.list == actual.list)
                    .then_some(route_rows)
            })
            .collect::<Vec<_>>();
        matching.sort();
        matching.dedup();
        match matching.as_slice() {
            [route_rows] => self.source_route_token(source, route_rows),
            [] => Err(Error::InvalidEvent(format!(
                "source {} ownership does not match descendant row {}:{}:{}",
                source.0, row.list.0, row.key, row.generation
            ))),
            _ => Err(Error::InvalidEvent(format!(
                "source {} ownership is ambiguous for descendant row {}:{}:{}",
                source.0, row.list.0, row.key, row.generation
            ))),
        }
    }

    pub fn source_route_token_for_path(
        &self,
        path: &str,
        ancestors: &[RowId],
    ) -> Result<SourceRouteToken, Error> {
        let source = self
            .metadata
            .routes
            .values()
            .find(|route| route.path == path)
            .map(|route| route.source_id)
            .ok_or_else(|| {
                Error::InvalidEvent(format!("source path `{path}` is not in the plan"))
            })?;
        self.source_route_token(source, ancestors)
    }

    pub fn row_target_for_source_path(
        &self,
        path: &str,
        key: u64,
        generation: u64,
    ) -> Result<RowId, Error> {
        let source = self
            .metadata
            .routes
            .values()
            .find(|route| route.path == path)
            .map(|route| route.source_id)
            .ok_or_else(|| {
                Error::InvalidEvent(format!("source path `{path}` is not in the plan"))
            })?;
        self.row_target_for_source(source, key, generation)
    }

    fn initialize_storage_prelude(&mut self) -> Result<(), Error> {
        for field in self.metadata.root_computations.keys() {
            self.root_fields.insert(*field, DerivedCell::default());
        }
        for list in self.metadata.list_computations.keys() {
            self.derived_lists.insert(*list, DerivedListCell::default());
        }
        for slot in &self.plan.storage_layout.scalar_slots {
            if slot.indexed || matches!(slot.initializer, ScalarInitializerPlan::Expression { .. })
            {
                continue;
            }
            let value = self.initial_slot_value(slot)?;
            self.root_states.insert(slot.state_id, value);
        }
        for slot in &self.plan.storage_layout.list_slots {
            self.lists.entry(slot.list_id).or_default();
        }
        Ok(())
    }

    fn initialize_root_field_default(
        &mut self,
        slot: &ScalarStorageSlot,
        work: &mut Work,
    ) -> Result<(), Error> {
        if slot.indexed {
            return Err(Error::InvalidPlan(format!(
                "indexed state {} reached root default initialization",
                slot.state_id.0
            )));
        }
        let ScalarInitializerPlan::Expression { expression } = &slot.initializer else {
            return Err(Error::InvalidPlan(format!(
                "root expression initializer for state {} changed kind",
                slot.state_id.0
            )));
        };
        if !self.root_states.contains_key(&slot.state_id) {
            let mut bindings = BTreeMap::new();
            let evaluated =
                self.eval_row_expression(expression, None, None, None, None, &mut bindings, work)?;
            let value = self.materialize_eval(evaluated)?;
            self.root_states.insert(slot.state_id, value);
        }
        Ok(())
    }

    fn initialize_missing_indexed_states(
        &mut self,
        row: RowId,
        work: &mut Work,
    ) -> Result<(), Error> {
        let existing = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .map(|row| row.fields.keys().copied().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        let slots = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| self.metadata.indexed_state_owner.get(&slot.state_id) == Some(&row.list))
            .filter(|slot| {
                self.metadata
                    .indexed_state_field
                    .get(&slot.state_id)
                    .is_none_or(|field| !existing.contains(field))
            })
            .cloned()
            .collect::<Vec<_>>();
        for slot in slots {
            self.initialize_indexed_state(row, &slot, work)?;
        }
        Ok(())
    }

    fn rebuild_runtime_state_after_rollback(&mut self, work: &mut Work) -> Result<(), Error> {
        for list in self.lists.values_mut() {
            list.rebuild_owner_partitions()?;
        }
        self.ordered_indexes.clear();
        self.dirty_ordered_indexes = self.metadata.list_indexes.keys().copied().collect();
        self.dirty_ordered_index_rows.clear();
        self.dynamic_dependencies = DynamicDependencies::default();
        self.next_binding_id = 1;
        self.initialize_root_source_bindings()?;
        let rows = self
            .lists
            .values()
            .flat_map(|state| state.order.iter().copied())
            .collect::<Vec<_>>();
        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
        for row in &rows {
            if let Some(state) = self
                .lists
                .get_mut(&row.list)
                .and_then(|list| list.rows.get_mut(row))
            {
                state.bindings.clear();
                for currentness in state.derived.values_mut() {
                    *currentness = Currentness::Dirty;
                }
            }
        }
        for row in rows {
            let scope = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.list_id == row.list)
                .and_then(|slot| slot.scope_id);
            self.bind_row_sources(row, scope)?;
        }
        for cell in self.root_fields.values_mut() {
            cell.currentness = Currentness::Dirty;
            cell.value = None;
        }
        for cell in self.derived_lists.values_mut() {
            cell.currentness = Currentness::Dirty;
            cell.items = None;
            if let Some(window) = cell.window.as_mut() {
                window.values_current = false;
            }
        }
        self.rebuild_ordered_indexes(work)
    }

    fn initialize_root_source_bindings(&mut self) -> Result<(), Error> {
        self.root_source_bindings.clear();
        let sources = self
            .metadata
            .routes
            .values()
            .filter(|route| !route.scoped)
            .map(|route| route.source_id)
            .collect::<Vec<_>>();
        for source in sources {
            let binding_epoch = self.next_binding_id;
            self.next_binding_id = self
                .next_binding_id
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("source binding epoch overflow".to_owned()))?;
            self.root_source_bindings.insert(source, binding_epoch);
        }
        Ok(())
    }

    fn initialize_list(&mut self, slot: &ListStorageSlot, work: &mut Work) -> Result<(), Error> {
        self.lists.entry(slot.list_id).or_default();
        for row in 0..storage_initializer_row_count(slot)? {
            self.initialize_storage_row(slot, row, work)?;
        }
        Ok(())
    }

    fn initialize_storage_row(
        &mut self,
        slot: &ListStorageSlot,
        row_index: u64,
        work: &mut Work,
    ) -> Result<(), Error> {
        let explicit_count = u64::try_from(slot.initial_rows.len()).map_err(|_| {
            Error::InvalidPlan(format!(
                "list {} initial row count does not fit the runtime key space",
                slot.list_id.0
            ))
        })?;
        let (fields, default_fields) = if row_index < explicit_count {
            let initial = slot
                .initial_rows
                .get(usize::try_from(row_index).map_err(|_| {
                    Error::InvalidPlan("initial row index does not fit usize".to_owned())
                })?)
                .ok_or_else(|| Error::InvalidPlan("initial row index is absent".to_owned()))?;
            let mut fields = BTreeMap::new();
            let mut default_fields = BTreeSet::new();
            for field in &initial.fields {
                let id = field.field_id.ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "list {} initial field `{}` has no FieldId",
                        slot.list_id.0, field.name
                    ))
                })?;
                match &field.initializer {
                    PlanInitialListFieldInitializer::Constant { value } => {
                        fields.insert(id, constant_value(value)?);
                    }
                    PlanInitialListFieldInitializer::Expression { .. } => {
                        default_fields.insert(id);
                    }
                }
            }
            (fields, default_fields)
        } else if slot.initializer_kind == ListInitializerKind::Range {
            let range = slot.range.ok_or_else(|| {
                Error::InvalidPlan(format!("list {} range has no bounds", slot.list_id.0))
            })?;
            let offset = row_index - explicit_count;
            let value = i128::from(range.from)
                .checked_add(i128::from(offset))
                .and_then(|value| i64::try_from(value).ok())
                .filter(|value| *value <= range.to)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "list {} range row index is outside its bounds",
                        slot.list_id.0
                    ))
                })?;
            let index_field = self.metadata.list_authority_field(slot.list_id, "index")?;
            let value_field = self.metadata.list_authority_field(slot.list_id, "value")?;
            let number = Value::integer(value)?;
            (
                BTreeMap::from([(index_field, number.clone()), (value_field, number)]),
                BTreeSet::new(),
            )
        } else {
            return Err(Error::InvalidPlan(format!(
                "list {} initializer has no row {}",
                slot.list_id.0, row_index
            )));
        };
        let key = row_index
            .checked_add(1)
            .ok_or_else(|| Error::InvalidPlan("initial row key overflowed".to_owned()))?;
        self.insert_initial_row(slot, key, fields, default_fields, work)?;
        Ok(())
    }

    fn insert_initial_row(
        &mut self,
        slot: &ListStorageSlot,
        key: u64,
        fields: BTreeMap<FieldId, Value>,
        default_fields: BTreeSet<FieldId>,
        work: &mut Work,
    ) -> Result<RowId, Error> {
        work.consume(1)?;
        let row_id = RowId {
            list: slot.list_id,
            key,
            generation: 1,
        };
        let mut row = Row {
            owner_ancestors: vec![OwnerInstanceRow {
                list: row_id.list,
                key: row_id.key,
                generation: row_id.generation,
            }],
            fields,
            default_fields,
            ..Row::default()
        };
        for field in self.metadata.row_computations.keys() {
            if self.metadata.row_field_owner.get(field) == Some(&slot.list_id) {
                row.derived.insert(*field, Currentness::Dirty);
            }
        }
        for field in &row.default_fields {
            row.derived.insert(*field, Currentness::Dirty);
        }
        let next_key = key.checked_add(1).ok_or_else(|| {
            Error::Evaluation(format!(
                "list {} row-key allocator is exhausted",
                slot.list_id.0
            ))
        })?;
        let prepared_order = self
            .lists
            .get(&slot.list_id)
            .ok_or_else(|| {
                Error::Evaluation(format!("list {} was not initialized", slot.list_id.0))
            })?
            .prepare_push_ordered(row_id, None)?;
        let maintenance = prepared_order.maintenance.clone();
        charge_source_order_maintenance(work, &maintenance)?;
        let list = self.lists.get_mut(&slot.list_id).ok_or_else(|| {
            Error::Evaluation(format!("list {} was not initialized", slot.list_id.0))
        })?;
        prepared_order.commit(list);
        list.next_key = list.next_key.max(next_key);
        list.rows.insert(row_id, row);
        list.index_owner_partition_row(row_id)?;
        record_source_order_maintenance(work, &maintenance);
        self.bind_row_sources(row_id, slot.scope_id)?;
        Ok(row_id)
    }

    fn initialize_indexed_states(&mut self, row: RowId, work: &mut Work) -> Result<(), Error> {
        let slots = self
            .plan
            .storage_layout
            .scalar_slots
            .iter()
            .filter(|slot| self.metadata.indexed_state_owner.get(&slot.state_id) == Some(&row.list))
            .cloned()
            .collect::<Vec<_>>();
        for slot in slots {
            self.initialize_indexed_state(row, &slot, work)?;
        }
        Ok(())
    }

    fn initialize_indexed_state(
        &mut self,
        row: RowId,
        slot: &ScalarStorageSlot,
        work: &mut Work,
    ) -> Result<(), Error> {
        let target = *self
            .metadata
            .indexed_state_field
            .get(&slot.state_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed state {} has no row field",
                    slot.state_id.0
                ))
            })?;
        let value = match &slot.initializer {
            ScalarInitializerPlan::Expression { expression } => {
                let mut bindings = BTreeMap::new();
                let evaluated = self.eval_row_expression(
                    expression,
                    Some(row),
                    None,
                    Some(target),
                    None,
                    &mut bindings,
                    work,
                )?;
                self.materialize_eval(evaluated)?
            }
            ScalarInitializerPlan::Constant { .. } => self.initial_slot_value(slot)?,
        };
        self.set_row_authority_field(row, target, value, work)?;
        Ok(())
    }

    fn initial_slot_value(&self, slot: &ScalarStorageSlot) -> Result<Value, Error> {
        let ScalarInitializerPlan::Constant {
            constant_id: constant,
        } = slot.initializer
        else {
            return Err(Error::InvalidPlan(format!(
                "state {} expression initializer was read as a constant",
                slot.state_id.0
            )));
        };
        self.metadata
            .constants
            .get(&constant)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("missing constant {}", constant.0)))
    }
}

impl MachineInstance {
    pub fn apply(&mut self, event: SourceEvent) -> Result<Turn, Error> {
        self.apply_with_demand(event, &[])
    }

    pub fn apply_with_demand(
        &mut self,
        event: SourceEvent,
        demanded_targets: &[ValueTarget],
    ) -> Result<Turn, Error> {
        if self.turn_work.pending_settle {
            self.settle_turn();
        }
        let mut work = std::mem::take(&mut self.turn_work);
        work.begin_turn(self.last_sequence, self.turn_sequence);
        let result = self.apply_with_work(event, demanded_targets, &mut work);
        let result = match result {
            Ok(turn) => {
                work.pending_settle = true;
                Ok(turn)
            }
            Err(error) => match self.rollback_turn(&mut work) {
                Ok(()) => {
                    self.last_sequence = work.previous_last_sequence;
                    self.turn_sequence = work.previous_turn_sequence;
                    work.pending_settle = false;
                    Err(error)
                }
                Err(rollback) => Err(Error::Evaluation(format!(
                    "turn failed with `{error}` and rollback failed with `{rollback}`"
                ))),
            },
        };
        self.turn_work = work;
        result
    }

    pub fn begin_effect_dispatch(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
    ) -> Result<Turn, Error> {
        let attempt = match item.state {
            boon_persistence::DurableOutboxState::Pending => 1,
            boon_persistence::DurableOutboxState::ReconciliationRequired { attempt } => attempt
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("effect attempt overflow".to_owned()))?,
            _ => {
                return Err(Error::Evaluation(format!(
                    "outbox item {} is not ready for dispatch",
                    item.item_id
                )));
            }
        };
        let sequence = self.next_internal_turn_sequence()?;
        self.finish_outbox_only_turn(boon_persistence::DurableOutboxChange::BeginDispatch {
            item_id: item.item_id,
            expected_revision: item.revision,
            next_revision: item
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("outbox item revision overflow".to_owned()))?,
            attempt,
            turn_sequence: sequence,
        })
    }

    pub fn require_effect_reconciliation(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
    ) -> Result<Turn, Error> {
        let boon_persistence::DurableOutboxState::Dispatching { attempt } = item.state else {
            return Err(Error::Evaluation(format!(
                "outbox item {} is not dispatching",
                item.item_id
            )));
        };
        let sequence = self.next_internal_turn_sequence()?;
        self.finish_outbox_only_turn(
            boon_persistence::DurableOutboxChange::RequireReconciliation {
                item_id: item.item_id,
                expected_revision: item.revision,
                next_revision: item
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| Error::Evaluation("outbox item revision overflow".to_owned()))?,
                attempt,
                turn_sequence: sequence,
            },
        )
    }

    pub fn complete_effect(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
        outcome: boon_persistence::StoredValue,
    ) -> Result<Turn, Error> {
        self.complete_effect_with_demand(item, outcome, &[])
    }

    pub fn complete_effect_with_demand(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
        outcome: boon_persistence::StoredValue,
        demanded_targets: &[ValueTarget],
    ) -> Result<Turn, Error> {
        let attempt = match item.state {
            boon_persistence::DurableOutboxState::Dispatching { attempt }
            | boon_persistence::DurableOutboxState::ReconciliationRequired { attempt } => attempt,
            _ => {
                return Err(Error::Evaluation(format!(
                    "outbox item {} is not awaiting an outcome",
                    item.item_id
                )));
            }
        };
        let (op, effect) = self.effect_invocation(item.invocation_id)?;
        if effect.effect_id != item.effect_id {
            return Err(Error::InvalidPlan(format!(
                "outbox item {} effect does not match invocation {}",
                item.item_id, item.invocation_id
            )));
        }
        let mut completed = item.clone();
        completed.revision = item
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("outbox item revision overflow".to_owned()))?;
        completed.state = boon_persistence::DurableOutboxState::Completed {
            attempt,
            outcome: outcome.clone(),
        };
        let sequence = self.next_internal_turn_sequence()?;
        completed.updated_turn_sequence = sequence;
        let schema = self
            .plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == item.effect_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect {} has no durable outbox schema",
                    item.effect_id
                ))
            })?;
        boon_persistence::validate_outbox_item_schema(&completed, schema)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        let mut work = self.take_internal_turn_work();
        work.effect_reconciliation_sequence = Some(sequence);
        let result = (|| {
            let row = self.runtime_row_for_effect(item.target_row)?;
            let owner = self.runtime_owner(&effect.owner, &item.owner)?;
            if row.is_some_and(|row| {
                owner.leaf().is_none_or(|leaf| {
                    leaf.list != row.list
                        || leaf.key != row.key
                        || leaf.generation != row.generation
                })
            }) {
                return Err(Error::InvalidPlan(format!(
                    "outbox item {} target row does not match its structural owner",
                    item.item_id
                )));
            }
            self.apply_effect_outcome(
                &op,
                &effect.result,
                &owner,
                row,
                outcome.clone(),
                sequence,
                &mut work,
            )?;
            self.commit_pending_list_mutations(&mut work)?;
            self.ensure_demanded_current(demanded_targets, None, &mut work)?;
            self.reconcile_dirty_effects(&mut work)?;
            let durable_changes = self.durable_changes(&work.authority_deltas)?;
            work.outbox_changes
                .push(boon_persistence::DurableOutboxChange::Complete {
                    item_id: item.item_id,
                    expected_revision: item.revision,
                    next_revision: completed.revision,
                    attempt,
                    outcome,
                    turn_sequence: sequence,
                });
            self.commit_transient_effects(&mut work)?;
            self.turn_sequence = sequence;
            work.finish_metrics();
            let turn = Turn {
                sequence,
                source_sequence: None,
                deltas: report_deltas(std::mem::take(&mut work.deltas)),
                authority_deltas: report_authority_deltas(std::mem::take(
                    &mut work.authority_deltas,
                )),
                durable_changes,
                outbox_changes: std::mem::take(&mut work.outbox_changes),
                transient_effects: std::mem::take(&mut work.transient_effects),
                cancelled_transient_effects: std::mem::take(&mut work.cancelled_transient_effects),
                transient_effect_credit_grants: std::mem::take(
                    &mut work.transient_effect_credit_grants,
                ),
                distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok(turn)
        })();
        self.finish_internal_turn_work(work, result)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<Turn, Error> {
        self.complete_transient_effect_with_demand(call_id, outcome, &[])
    }

    pub fn complete_transient_effect_with_demand(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        demanded_targets: &[ValueTarget],
    ) -> Result<Turn, Error> {
        if call_id.launch_epoch != self.launch_epoch {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} belongs to a different session launch"
            )));
        }
        let pending = self
            .pending_transient_effects
            .get(&call_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent(format!(
                    "transient effect call {call_id} is unknown, cancelled, or already completed"
                ))
            })?;
        self.validate_current_effect_owner(&pending.owner, pending.target, call_id)?;
        let (op, effect) = self.effect_invocation(pending.invocation_id)?;
        if effect.effect_id != pending.effect_id {
            return Err(Error::InvalidPlan(format!(
                "transient effect call {call_id} no longer matches invocation {}",
                pending.invocation_id
            )));
        }
        let contract = self
            .plan
            .effects
            .iter()
            .find(|contract| contract.effect_id == effect.effect_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "transient effect call {call_id} has no effect contract"
                ))
            })?;
        if !matches!(
            contract.delivery,
            boon_plan::EffectDeliveryCardinality::Single
        ) {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} is a stream; deliver it with an explicit result sequence"
            )));
        }
        if !effect_replay_is_transient(&contract.replay)
            || contract.barrier != boon_plan::EffectBarrier::None
        {
            return Err(Error::InvalidPlan(format!(
                "transient effect call {call_id} is not process-local and barrier-free"
            )));
        }
        let result_type = &contract
            .schema
            .as_ref()
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "transient effect call {call_id} has no typed result schema"
                ))
            })?
            .result_type;
        validate_value_for_data_type(&outcome, result_type, "effect outcome")?;
        let stored_outcome = stored_value_for_data_type(&outcome, result_type)?;
        let sequence = self.next_internal_turn_sequence()?;
        let mut work = self.take_internal_turn_work();
        work.effect_reconciliation_sequence = Some(sequence);
        let result = (|| {
            self.apply_effect_outcome(
                &op,
                &effect.result,
                &pending.owner,
                pending.target,
                stored_outcome,
                sequence,
                &mut work,
            )?;
            self.commit_pending_list_mutations(&mut work)?;
            self.ensure_demanded_current(demanded_targets, None, &mut work)?;
            self.reconcile_dirty_effects(&mut work)?;
            let durable_changes = self.durable_changes(&work.authority_deltas)?;
            let removed = self
                .pending_transient_effects
                .remove(&call_id)
                .ok_or_else(|| {
                    Error::InvalidEvent(format!(
                        "transient effect call {call_id} was completed concurrently"
                    ))
                })?;
            work.completed_transient_effects.push((call_id, removed));
            self.commit_transient_effects(&mut work)?;
            self.turn_sequence = sequence;
            work.finish_metrics();
            let turn = Turn {
                sequence,
                source_sequence: None,
                deltas: report_deltas(std::mem::take(&mut work.deltas)),
                authority_deltas: report_authority_deltas(std::mem::take(
                    &mut work.authority_deltas,
                )),
                durable_changes,
                outbox_changes: Vec::new(),
                transient_effects: std::mem::take(&mut work.transient_effects),
                cancelled_transient_effects: std::mem::take(&mut work.cancelled_transient_effects),
                transient_effect_credit_grants: std::mem::take(
                    &mut work.transient_effect_credit_grants,
                ),
                distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok(turn)
        })();
        self.finish_internal_turn_work(work, result)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<Turn, Error> {
        self.deliver_transient_effect_result_with_demand(call_id, result_sequence, outcome, &[])
    }

    pub fn deliver_transient_effect_result_with_demand(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        demanded_targets: &[ValueTarget],
    ) -> Result<Turn, Error> {
        if call_id.launch_epoch != self.launch_epoch {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} belongs to a different session launch"
            )));
        }
        let previous_pending = self
            .pending_transient_effects
            .get(&call_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidEvent(format!(
                    "transient effect call {call_id} is unknown, cancelled, or already completed"
                ))
            })?;
        let mut pending = previous_pending.clone();
        self.validate_current_effect_owner(&pending.owner, pending.target, call_id)?;
        let boon_plan::EffectDeliveryCardinality::Stream {
            max_in_flight,
            credit_result_tags,
            terminal_result_tags,
            ..
        } = &pending.delivery
        else {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} is single-delivery"
            )));
        };
        if result_sequence != pending.next_result_sequence {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} expected result sequence {}, got {result_sequence}",
                pending.next_result_sequence
            )));
        }
        let (op, effect) = self.effect_invocation(pending.invocation_id)?;
        if effect.effect_id != pending.effect_id {
            return Err(Error::InvalidPlan(format!(
                "transient effect call {call_id} no longer matches invocation {}",
                pending.invocation_id
            )));
        }
        let contract = self
            .plan
            .effects
            .iter()
            .find(|contract| contract.effect_id == effect.effect_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "transient effect call {call_id} has no effect contract"
                ))
            })?;
        if contract.delivery != pending.delivery {
            return Err(Error::InvalidPlan(format!(
                "transient effect call {call_id} delivery contract changed while active"
            )));
        }
        let result_type = &contract
            .schema
            .as_ref()
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "transient effect call {call_id} has no typed result schema"
                ))
            })?
            .result_type;
        validate_value_for_data_type(&outcome, result_type, "stream effect outcome")?;
        let outcome_tag = effect_outcome_tag(&outcome).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "transient effect call {call_id} stream outcome has no variant tag"
            ))
        })?;
        let terminal = terminal_result_tags
            .binary_search_by(|candidate| candidate.as_str().cmp(outcome_tag))
            .is_ok();
        let consumes_credit = credit_result_tags
            .binary_search_by(|candidate| candidate.as_str().cmp(outcome_tag))
            .is_ok();
        if consumes_credit && pending.available_credits == 0 {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} has no delivery credit"
            )));
        }
        pending
            .semantic
            .accept(result_sequence, &outcome, terminal)
            .map_err(|error| {
                Error::InvalidEvent(format!(
                    "transient effect call {call_id} violated its stream contract: {error}"
                ))
            })?;
        let stored_outcome = stored_value_for_data_type(&outcome, result_type)?;
        let max_in_flight = *max_in_flight;
        let sequence = self.next_internal_turn_sequence()?;
        let mut work = self.take_internal_turn_work();
        work.effect_reconciliation_sequence = Some(sequence);
        let result = (|| {
            self.apply_effect_outcome(
                &op,
                &effect.result,
                &pending.owner,
                pending.target,
                stored_outcome,
                sequence,
                &mut work,
            )?;
            self.commit_pending_list_mutations(&mut work)?;
            self.ensure_demanded_current(demanded_targets, None, &mut work)?;
            self.reconcile_dirty_effects(&mut work)?;
            let durable_changes = self.durable_changes(&work.authority_deltas)?;

            if terminal {
                let removed = self
                    .pending_transient_effects
                    .remove(&call_id)
                    .ok_or_else(|| {
                        Error::InvalidEvent(format!(
                            "transient effect call {call_id} terminated concurrently"
                        ))
                    })?;
                work.completed_transient_effects.push((call_id, removed));
            } else {
                work.updated_transient_effects
                    .push((call_id, previous_pending.clone()));
                pending.next_result_sequence =
                    pending.next_result_sequence.checked_add(1).ok_or_else(|| {
                        Error::Evaluation(format!(
                            "transient effect call {call_id} exhausted result sequences"
                        ))
                    })?;
                if consumes_credit {
                    pending.available_credits = pending.available_credits.saturating_sub(1);
                    pending.available_credits = pending
                        .available_credits
                        .checked_add(1)
                        .unwrap_or(max_in_flight)
                        .min(max_in_flight);
                    work.transient_effect_credit_grants
                        .push(TransientEffectCreditGrant {
                            call_id,
                            credits: 1,
                        });
                }
                let active = self
                    .pending_transient_effects
                    .get_mut(&call_id)
                    .ok_or_else(|| {
                        Error::InvalidEvent(format!(
                            "transient effect call {call_id} was cancelled concurrently"
                        ))
                    })?;
                *active = pending;
            }

            self.commit_transient_effects(&mut work)?;
            self.turn_sequence = sequence;
            work.finish_metrics();
            let turn = Turn {
                sequence,
                source_sequence: None,
                deltas: report_deltas(std::mem::take(&mut work.deltas)),
                authority_deltas: report_authority_deltas(std::mem::take(
                    &mut work.authority_deltas,
                )),
                durable_changes,
                outbox_changes: Vec::new(),
                transient_effects: std::mem::take(&mut work.transient_effects),
                cancelled_transient_effects: std::mem::take(&mut work.cancelled_transient_effects),
                transient_effect_credit_grants: std::mem::take(
                    &mut work.transient_effect_credit_grants,
                ),
                distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok(turn)
        })();
        self.finish_internal_turn_work(work, result)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> Result<bool, Error> {
        if call_id.launch_epoch != self.launch_epoch {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} belongs to a different session launch"
            )));
        }
        Ok(self.pending_transient_effects.remove(&call_id).is_some())
    }

    pub fn cancel_transient_effects(
        &mut self,
        call_ids: &[TransientEffectCallId],
    ) -> Result<Option<Turn>, Error> {
        let call_ids = call_ids.iter().copied().collect::<BTreeSet<_>>();
        for call_id in &call_ids {
            if call_id.launch_epoch != self.launch_epoch {
                return Err(Error::InvalidEvent(
                    "transient effect cancellation belongs to a different session launch"
                        .to_owned(),
                ));
            }
        }
        let call_ids = call_ids
            .into_iter()
            .filter(|call_id| self.pending_transient_effects.contains_key(call_id))
            .collect::<Vec<_>>();
        if call_ids.is_empty() {
            return Ok(None);
        }

        let sequence = self.next_internal_turn_sequence()?;
        let mut work = self.take_internal_turn_work();
        let result = (|| {
            for call_id in call_ids {
                let pending = self
                    .pending_transient_effects
                    .remove(&call_id)
                    .ok_or_else(|| {
                        Error::InvalidEvent(
                            "transient effect was cancelled concurrently".to_owned(),
                        )
                    })?;
                work.completed_transient_effects.push((call_id, pending));
                work.cancelled_transient_effects.push(call_id);
            }
            self.turn_sequence = sequence;
            work.finish_metrics();
            let turn = Turn {
                sequence,
                source_sequence: None,
                deltas: Vec::new(),
                authority_deltas: Vec::new(),
                durable_changes: Vec::new(),
                outbox_changes: Vec::new(),
                transient_effects: Vec::new(),
                cancelled_transient_effects: std::mem::take(&mut work.cancelled_transient_effects),
                transient_effect_credit_grants: Vec::new(),
                distributed_invocations: std::mem::take(&mut work.distributed_invocations),
                metrics: std::mem::take(&mut work.metrics),
            };
            work.pending_settle = true;
            Ok(turn)
        })();
        self.finish_internal_turn_work(work, result).map(Some)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.pending_transient_effects.len()
    }

    pub fn pending_transient_effect_credits(&self, call_id: TransientEffectCallId) -> Option<u32> {
        self.pending_transient_effects
            .get(&call_id)
            .map(|pending| pending.available_credits)
    }

    fn next_internal_turn_sequence(&self) -> Result<u64, Error> {
        self.turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))
    }

    fn finish_outbox_only_turn(
        &mut self,
        change: boon_persistence::DurableOutboxChange,
    ) -> Result<Turn, Error> {
        let sequence = self.next_internal_turn_sequence()?;
        let mut work = self.take_internal_turn_work();
        work.outbox_changes.push(change);
        self.turn_sequence = sequence;
        work.finish_metrics();
        let turn = Turn {
            sequence,
            source_sequence: None,
            deltas: Vec::new(),
            authority_deltas: Vec::new(),
            durable_changes: Vec::new(),
            outbox_changes: std::mem::take(&mut work.outbox_changes),
            transient_effects: Vec::new(),
            cancelled_transient_effects: Vec::new(),
            transient_effect_credit_grants: Vec::new(),
            distributed_invocations: std::mem::take(&mut work.distributed_invocations),
            metrics: std::mem::take(&mut work.metrics),
        };
        work.pending_settle = true;
        self.turn_work = work;
        Ok(turn)
    }

    fn take_internal_turn_work(&mut self) -> Work {
        if self.turn_work.pending_settle {
            self.settle_turn();
        }
        let mut work = std::mem::take(&mut self.turn_work);
        work.begin_turn(self.last_sequence, self.turn_sequence);
        work
    }

    fn finish_internal_turn_work<T>(
        &mut self,
        mut work: Work,
        result: Result<T, Error>,
    ) -> Result<T, Error> {
        let result = match result {
            Ok(turn) => Ok(turn),
            Err(error) => match self.rollback_turn(&mut work) {
                Ok(()) => {
                    self.last_sequence = work.previous_last_sequence;
                    self.turn_sequence = work.previous_turn_sequence;
                    work.pending_settle = false;
                    Err(error)
                }
                Err(rollback) => Err(Error::Evaluation(format!(
                    "effect outcome failed with `{error}` and rollback failed with `{rollback}`"
                ))),
            },
        };
        self.turn_work = work;
        result
    }

    fn effect_invocation(
        &self,
        invocation_id: EffectInvocationId,
    ) -> Result<(PlanOp, boon_plan::EffectInvocationPlan), Error> {
        self.plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .find_map(|op| {
                let PlanOpKind::StateUpdate {
                    effect: Some(effect),
                    ..
                } = &op.kind
                else {
                    return None;
                };
                (effect.invocation_id == invocation_id).then(|| (op.clone(), effect.clone()))
            })
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect invocation {invocation_id} is absent from the active plan"
                ))
            })
    }

    fn runtime_row_for_effect(
        &self,
        target: Option<boon_persistence::DurableRowId>,
    ) -> Result<Option<RowId>, Error> {
        let Some(target) = target else {
            return Ok(None);
        };
        let memory = self
            .plan
            .persistence
            .lists
            .iter()
            .find(|memory| memory.memory_id == target.list_memory_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect target list {} is absent from the active plan",
                    target.list_memory_id
                ))
            })?;
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect target list {} has no runtime slot",
                    target.list_memory_id
                ))
            })?;
        let row = RowId {
            list: slot.list_id,
            key: target.row_key,
            generation: target.row_generation,
        };
        if !self
            .lists
            .get(&row.list)
            .is_some_and(|list| list.rows.contains_key(&row))
        {
            return Err(Error::Evaluation(format!(
                "effect target row {}:{}:{} no longer exists",
                row.list.0, row.key, row.generation
            )));
        }
        Ok(Some(row))
    }

    fn durable_owner(
        &self,
        owner: &OwnerInstanceId,
    ) -> Result<boon_persistence::DurableOwner, Error> {
        durable_owner_for_rows(&self.plan, &owner.ancestors)
    }

    fn runtime_owner_rows(
        &self,
        durable: &boon_persistence::DurableOwner,
    ) -> Result<Vec<OwnerInstanceRow>, Error> {
        durable
            .ancestors
            .iter()
            .enumerate()
            .map(|(index, stored)| {
                let list = self
                    .metadata
                    .durable_list_by_memory
                    .get(&stored.list_memory_id)
                    .ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "durable owner depth {index} references unknown list memory {}",
                            stored.list_memory_id
                        ))
                    })?;
                Ok(OwnerInstanceRow {
                    list: list.list_id,
                    key: stored.row_key,
                    generation: stored.row_generation,
                })
            })
            .collect()
    }

    fn runtime_owner(
        &self,
        plan: &PlanOwner,
        durable: &boon_persistence::DurableOwner,
    ) -> Result<OwnerInstanceId, Error> {
        if durable.ancestors.len() != plan.ancestors.len() {
            return Err(Error::InvalidPlan(format!(
                "durable owner for static owner {} has {} ancestors, expected {}",
                plan.static_owner.0,
                durable.ancestors.len(),
                plan.ancestors.len()
            )));
        }
        let mut ancestors = Vec::with_capacity(durable.ancestors.len());
        for (index, (stored, expected)) in durable.ancestors.iter().zip(&plan.ancestors).enumerate()
        {
            let memory = self
                .plan
                .persistence
                .lists
                .iter()
                .find(|memory| memory.memory_id == stored.list_memory_id)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "durable owner depth {index} references unknown list memory {}",
                        stored.list_memory_id
                    ))
                })?;
            let slot = self
                .plan
                .storage_layout
                .list_slots
                .iter()
                .find(|slot| slot.id == memory.runtime_slot)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "durable owner depth {index} list memory {} has no runtime slot",
                        stored.list_memory_id
                    ))
                })?;
            if slot.list_id != expected.list {
                return Err(Error::InvalidPlan(format!(
                    "durable owner depth {index} resolves list {}, expected {}",
                    slot.list_id.0, expected.list.0
                )));
            }
            let row = RowId {
                list: slot.list_id,
                key: stored.row_key,
                generation: stored.row_generation,
            };
            if !self
                .lists
                .get(&row.list)
                .is_some_and(|list| list.rows.contains_key(&row))
            {
                return Err(Error::Evaluation(format!(
                    "durable effect owner row {}:{}:{} is stale",
                    row.list.0, row.key, row.generation
                )));
            }
            let owner_row = OwnerInstanceRow {
                list: row.list,
                key: row.key,
                generation: row.generation,
            };
            let runtime_prefix = self.row_owner_ancestors(row)?;
            if runtime_prefix.len() != index + 1
                || runtime_prefix[..index] != ancestors
                || runtime_prefix[index] != owner_row
            {
                return Err(Error::InvalidPlan(format!(
                    "durable effect owner depth {index} does not match the row's structural owner"
                )));
            }
            ancestors.push(owner_row);
        }
        OwnerInstanceId::new(plan.static_owner, ancestors)
            .map_err(|detail| Error::InvalidPlan(detail.to_owned()))
    }

    fn apply_effect_outcome(
        &mut self,
        op: &PlanOp,
        route: &boon_plan::EffectResultRoute,
        owner: &OwnerInstanceId,
        row: Option<RowId>,
        outcome: boon_persistence::StoredValue,
        sequence: u64,
        work: &mut Work,
    ) -> Result<(), Error> {
        match route {
            boon_plan::EffectResultRoute::Target { target, .. } => {
                let value = runtime_value(outcome)?;
                self.apply_effect_result(op, target, owner, row, value, sequence, work)
            }
        }
    }

    fn apply_effect_result(
        &mut self,
        op: &PlanOp,
        target: &ValueRef,
        owner: &OwnerInstanceId,
        row: Option<RowId>,
        value: Value,
        sequence: u64,
        work: &mut Work,
    ) -> Result<(), Error> {
        let ValueRef::State(state) = target else {
            return Err(Error::InvalidPlan(format!(
                "effect invocation {} has a non-state result target",
                op.id.0
            )));
        };
        if op.indexed {
            let row = row.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed effect invocation {} has no durable row target",
                    op.id.0
                ))
            })?;
            let field = *self
                .metadata
                .indexed_state_field
                .get(state)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!("indexed state {} has no FieldId", state.0))
                })?;
            self.record_row_field_undo(row, field, work);
            self.touched_row_fields.insert((row, field));
            work.authority_deltas.push(AuthorityDelta::SetRowField {
                row,
                owner_ancestors: self.row_owner_ancestors(row)?.to_vec(),
                materialization_origin: self
                    .lists
                    .get(&row.list)
                    .and_then(|list| list.rows.get(&row))
                    .and_then(|row| row.materialization_origin.clone()),
                field,
                value: value.clone(),
            });
            self.set_row_authority_field(row, field, value, work)?;
            let trigger = TriggerFrame::effect(
                effect_invocation_id(op)?,
                effect_invocation_plan(op)?.owner.clone(),
                owner.clone(),
                Some(row),
                sequence,
            );
            self.route_state_transition(*state, &trigger, Some(row), work)?;
        } else {
            if row.is_some() {
                return Err(Error::InvalidPlan(format!(
                    "root effect invocation {} unexpectedly carries a row target",
                    op.id.0
                )));
            }
            self.record_root_undo(*state, work);
            self.touched_root_states.insert(*state);
            work.authority_deltas.push(AuthorityDelta::SetRoot {
                state: *state,
                value: value.clone(),
            });
            self.set_root_state(*state, value, work);
            let trigger = TriggerFrame::effect(
                effect_invocation_id(op)?,
                effect_invocation_plan(op)?.owner.clone(),
                owner.clone(),
                None,
                sequence,
            );
            self.route_state_transition(*state, &trigger, None, work)?;
        }
        Ok(())
    }

    fn route_state_transition(
        &mut self,
        state: StateId,
        origin: &TriggerFrame<'_>,
        row: Option<RowId>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let (owner_plan, owner) = self.state_owner_instance(state, &origin.active, row)?;
        let route_key = (state, owner.clone());
        if !work.active_state_routes.insert(route_key.clone()) {
            return Err(Error::InvalidPlan(format!(
                "state transition cycle re-entered state {} for owner {:?}",
                state.0, owner
            )));
        }
        let active = ActiveTrigger {
            cause: TriggerCause::State(state),
            owner_plan,
            owner,
            target: row,
            sequence: origin.active.sequence,
        };
        let previous_trigger = work.active_trigger.replace(active.clone());
        let trigger = TriggerFrame {
            active,
            source_event: origin.source_event,
        };
        let result = (|| {
            let derived = self
                .metadata
                .state_derived_by_state
                .get(&state)
                .cloned()
                .unwrap_or_default();
            let derived_lists = self
                .metadata
                .state_derived_lists_by_state
                .get(&state)
                .cloned()
                .unwrap_or_default();
            for field in &derived {
                self.mark_root_dirty(*field, work);
            }
            for list in &derived_lists {
                self.mark_list_dirty(*list, work);
            }
            for field in &derived {
                self.ensure_root_field(*field, None, work)?;
            }
            for list in &derived_lists {
                self.ensure_list_current(*list, None, work)?;
            }

            let updates = self
                .metadata
                .updates_by_state
                .get(&state)
                .cloned()
                .unwrap_or_default();
            for op in updates.iter().filter(|op| !update_branch_has_effect(op)) {
                self.execute_update(op, row, &trigger, work)?;
            }
            for field in &derived {
                self.mark_root_dirty(*field, work);
            }
            for list in &derived_lists {
                self.mark_list_dirty(*list, work);
            }
            for field in &derived {
                self.ensure_root_field(*field, None, work)?;
            }
            for list in &derived_lists {
                self.ensure_list_current(*list, None, work)?;
            }
            let mutations = self
                .metadata
                .mutations_by_state
                .get(&state)
                .cloned()
                .unwrap_or_default();
            self.stage_mutation_batch(&mutations, None, &trigger, work)?;
            for op in updates.iter().filter(|op| update_branch_has_effect(op)) {
                self.execute_update(op, row, &trigger, work)?;
            }
            self.collect_distributed_invocations_for_trigger(&trigger, work)?;
            Ok(())
        })();
        work.active_trigger = previous_trigger;
        work.active_state_routes.remove(&route_key);
        result
    }

    pub fn settle_turn(&mut self) {
        let had_pending_turn = self.turn_work.pending_settle;
        self.turn_work.settle();
        if !had_pending_turn {
            return;
        }
        if self.active_producer_lease.is_some() {
            self.finish_active_producer_lease(true);
        } else {
            self.global_dependency_revision = self.global_dependency_revision.saturating_add(1);
        }
    }

    pub fn rollback_unsettled_turn(&mut self) -> Result<(), Error> {
        if !self.turn_work.pending_settle {
            return Ok(());
        }
        let mut work = std::mem::take(&mut self.turn_work);
        let previous_last_sequence = work.previous_last_sequence;
        let previous_turn_sequence = work.previous_turn_sequence;
        let result = self.rollback_turn(&mut work);
        if result.is_ok() {
            self.last_sequence = previous_last_sequence;
            self.turn_sequence = previous_turn_sequence;
            work.pending_settle = false;
        }
        self.turn_work = work;
        if result.is_ok() && self.active_producer_lease.is_some() {
            self.restore_active_producer_lease();
        }
        result
    }

    fn record_root_undo(&self, state: StateId, work: &mut Work) {
        if work.undo_root_states.insert(state) {
            work.authority_undo.push(AuthorityUndo::RootState {
                state,
                value: self.root_states.get(&state).cloned(),
                touched: self.touched_root_states.contains(&state),
            });
        }
    }

    fn record_row_field_undo(&self, row: RowId, field: FieldId, work: &mut Work) {
        if work.undo_row_fields.insert((row, field)) {
            work.authority_undo.push(AuthorityUndo::RowField {
                row,
                field,
                value: self
                    .lists
                    .get(&row.list)
                    .and_then(|list| list.rows.get(&row))
                    .and_then(|row| row.fields.get(&field))
                    .cloned(),
                touched_field: self.touched_row_fields.contains(&(row, field)),
                touched_list: self.touched_lists.contains(&row.list),
            });
        }
    }

    fn mark_list_semantic_change(&mut self, list_id: ListId, work: &mut Work) -> Result<(), Error> {
        if !work.emit {
            return Ok(());
        }
        let next_revision = self
            .turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("list revision overflow".to_owned()))?;
        let list = self.lists.get_mut(&list_id).ok_or_else(|| {
            Error::Evaluation(format!("cannot revise missing list {}", list_id.0))
        })?;
        work.list_revision_undo
            .entry(list_id)
            .or_insert(list.revision);
        list.revision = next_revision;
        Ok(())
    }

    fn rollback_turn(&mut self, work: &mut Work) -> Result<(), Error> {
        work.allow_rollback();
        if let Some(undo) = work.distributed_context_undo.take() {
            self.options.session_context = undo.session_context;
            for (import_id, (value, revision)) in undo.imports {
                match value {
                    Some(value) => {
                        self.distributed_imports.insert(import_id, value);
                    }
                    None => {
                        self.distributed_imports.remove(&import_id);
                    }
                }
                match revision {
                    Some(revision) => {
                        self.distributed_import_revisions
                            .insert(import_id, revision);
                    }
                    None => {
                        self.distributed_import_revisions.remove(&import_id);
                    }
                }
            }
            for (key, previous) in undo.row_owned_call_results {
                match previous {
                    Some(previous) => {
                        self.row_owned_call_results.insert(key, previous);
                    }
                    None => {
                        self.row_owned_call_results.remove(&key);
                    }
                }
            }
        }
        for (key, lease) in std::mem::take(&mut work.detached_producer_leases) {
            if self.producer_leases.insert(key, lease).is_some() {
                return Err(Error::Evaluation(
                    "detached producer lease rollback collided with a live lease".to_owned(),
                ));
            }
        }
        for call_id in work.committed_transient_effects.drain(..) {
            self.pending_transient_effects.remove(&call_id);
        }
        for (call_id, pending) in work.completed_transient_effects.drain(..) {
            self.pending_transient_effects.insert(call_id, pending);
        }
        for (call_id, previous) in work.updated_transient_effects.drain(..) {
            self.pending_transient_effects.insert(call_id, previous);
        }
        for undo in work.authority_undo.drain(..).rev() {
            match undo {
                AuthorityUndo::RootState {
                    state,
                    value,
                    touched,
                } => {
                    match value {
                        Some(value) => {
                            self.root_states.insert(state, value);
                        }
                        None => {
                            self.root_states.remove(&state);
                        }
                    }
                    if touched {
                        self.touched_root_states.insert(state);
                    } else {
                        self.touched_root_states.remove(&state);
                    }
                }
                AuthorityUndo::RowField {
                    row,
                    field,
                    value,
                    touched_field,
                    touched_list,
                } => {
                    let fields = &mut self
                        .lists
                        .get_mut(&row.list)
                        .and_then(|list| list.rows.get_mut(&row))
                        .ok_or_else(|| {
                            Error::Evaluation(format!(
                                "rollback cannot find row {}:{}:{}",
                                row.list.0, row.key, row.generation
                            ))
                        })?
                        .fields;
                    match value {
                        Some(value) => {
                            fields.insert(field, value);
                        }
                        None => {
                            fields.remove(&field);
                        }
                    }
                    if touched_field {
                        self.touched_row_fields.insert((row, field));
                    } else {
                        self.touched_row_fields.remove(&(row, field));
                    }
                    if touched_list {
                        self.touched_lists.insert(row.list);
                    } else {
                        self.touched_lists.remove(&row.list);
                    }
                }
                AuthorityUndo::AppendRow {
                    row,
                    previous_next_key,
                    source_order,
                    touched_list,
                } => {
                    let list = self.lists.get_mut(&row.list).ok_or_else(|| {
                        Error::Evaluation(format!("rollback list {} is missing", row.list.0))
                    })?;
                    if let Some(removed) = list.rows.remove(&row) {
                        list.remove_owner_partition_row(row, &removed);
                    }
                    list.restore_source_order(&source_order)?;
                    list.next_key = previous_next_key;
                    self.touched_row_fields
                        .retain(|(candidate, _)| *candidate != row);
                    if touched_list {
                        self.touched_lists.insert(row.list);
                    } else {
                        self.touched_lists.remove(&row.list);
                    }
                }
                AuthorityUndo::RemoveRow {
                    row,
                    value,
                    source_order,
                    previous_next_key,
                    touched_list,
                    touched_fields,
                } => {
                    let list = self.lists.get_mut(&row.list).ok_or_else(|| {
                        Error::Evaluation(format!("rollback list {} is missing", row.list.0))
                    })?;
                    list.restore_source_order(&source_order)?;
                    if !list.rows.contains_key(&row) {
                        list.rows.insert(row, value);
                        list.index_owner_partition_row(row)?;
                    }
                    list.next_key = previous_next_key;
                    self.touched_row_fields
                        .retain(|(candidate, _)| *candidate != row);
                    self.touched_row_fields
                        .extend(touched_fields.into_iter().map(|field| (row, field)));
                    if touched_list {
                        self.touched_lists.insert(row.list);
                    } else {
                        self.touched_lists.remove(&row.list);
                    }
                }
                AuthorityUndo::ReorderRows {
                    list,
                    undo,
                    owner_partition,
                } => {
                    let list = self.lists.get_mut(&list).ok_or_else(|| {
                        Error::Evaluation(format!("rollback reordered list {} is missing", list.0))
                    })?;
                    list.restore_source_order(&undo)?;
                    if let Some((owner_prefix, previous_order)) = owner_partition {
                        list.owner_partitions
                            .get_mut(&owner_prefix)
                            .ok_or_else(|| {
                                Error::InvalidPlan(
                                    "rollback materialized owner partition is missing".to_owned(),
                                )
                            })?
                            .order = previous_order;
                    }
                }
            }
        }
        for (list_id, revision) in std::mem::take(&mut work.list_revision_undo) {
            let list = self.lists.get_mut(&list_id).ok_or_else(|| {
                Error::Evaluation(format!(
                    "rollback cannot restore revision for missing list {}",
                    list_id.0
                ))
            })?;
            list.revision = revision;
        }
        for (consumer, previous) in std::mem::take(&mut work.effect_activation_undo) {
            match previous {
                Some(previous) => {
                    self.effect_activations.insert(consumer, previous);
                }
                None => {
                    self.effect_activations.remove(&consumer);
                }
            }
        }
        work.undo_root_states.clear();
        work.undo_row_fields.clear();
        work.deltas.clear();
        work.authority_deltas.clear();
        work.outbox_changes.clear();
        work.transient_effects.clear();
        work.distributed_invocations.clear();
        work.pending_list_mutations.clear();
        work.emit = false;
        self.rebuild_runtime_state_after_rollback(work)?;
        self.ensure_published_current(None, work)?;
        self.rebuild_active_effect_dependencies(work)?;
        Ok(())
    }

    fn apply_with_work(
        &mut self,
        mut event: SourceEvent,
        demanded_targets: &[ValueTarget],
        work: &mut Work,
    ) -> Result<Turn, Error> {
        self.validate_event(&event)?;
        self.route_event_with_work(&mut event, demanded_targets, work)?;
        let durable_changes = self.durable_changes(&work.authority_deltas)?;

        self.last_sequence = Some(event.sequence);
        self.turn_sequence = self
            .turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))?;
        self.commit_transient_effects(work)?;
        work.finish_metrics();
        Ok(Turn {
            sequence: self.turn_sequence,
            source_sequence: Some(event.sequence),
            deltas: report_deltas(std::mem::take(&mut work.deltas)),
            authority_deltas: report_authority_deltas(std::mem::take(&mut work.authority_deltas)),
            durable_changes,
            outbox_changes: std::mem::take(&mut work.outbox_changes),
            transient_effects: std::mem::take(&mut work.transient_effects),
            cancelled_transient_effects: std::mem::take(&mut work.cancelled_transient_effects),
            transient_effect_credit_grants: std::mem::take(
                &mut work.transient_effect_credit_grants,
            ),
            distributed_invocations: std::mem::take(&mut work.distributed_invocations),
            metrics: std::mem::take(&mut work.metrics),
        })
    }

    fn commit_transient_effects(&mut self, work: &mut Work) -> Result<(), Error> {
        let mut latest = BTreeMap::<(EffectInvocationId, OwnerInstanceId), usize>::new();
        for (index, invocation) in work.transient_effects.iter().enumerate() {
            latest.insert((invocation.invocation_id, invocation.owner.clone()), index);
        }
        work.transient_effects = work
            .transient_effects
            .drain(..)
            .enumerate()
            .filter_map(|(index, invocation)| {
                (latest.get(&(invocation.invocation_id, invocation.owner.clone())) == Some(&index))
                    .then_some(invocation)
            })
            .collect();

        for index in 0..work.transient_effects.len() {
            let invocation_id = work.transient_effects[index].invocation_id;
            let effect_id = work.transient_effects[index].effect_id;
            let target = work.transient_effects[index].target;
            let owner = work.transient_effects[index].owner.clone();
            let call_id = work.transient_effects[index].call_id;
            self.cancel_pending_transient_effect_owner(
                invocation_id,
                &owner,
                self.transient_effect_scope,
                work,
            );
            let contract = self
                .plan
                .effects
                .iter()
                .find(|contract| contract.effect_id == effect_id)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "transient effect call {} has no effect contract",
                        call_id
                    ))
                })?;
            let available_credits = match contract.delivery {
                boon_plan::EffectDeliveryCardinality::Single => 1,
                boon_plan::EffectDeliveryCardinality::Stream {
                    initial_credits, ..
                } => initial_credits,
            };
            let semantic = TransientEffectSemanticValidator::for_invocation(
                &contract.host_operation,
                &work.transient_effects[index].intent,
            )
            .map_err(|error| {
                Error::InvalidPlan(format!(
                    "transient effect call {call_id} has invalid semantic stream configuration: {error}"
                ))
            })?;
            let pending = PendingTransientEffect {
                invocation_id,
                effect_id,
                owner,
                target,
                execution_scope: self.transient_effect_scope,
                delivery: contract.delivery.clone(),
                semantic,
                next_result_sequence: TRANSIENT_EFFECT_FIRST_RESULT_SEQUENCE,
                available_credits,
            };
            if self
                .pending_transient_effects
                .insert(call_id, pending)
                .is_some()
            {
                return Err(Error::Evaluation(format!(
                    "transient effect call {} was emitted more than once",
                    call_id
                )));
            }
            work.committed_transient_effects.push(call_id);
        }
        Ok(())
    }

    fn cancel_pending_transient_effect_owner(
        &mut self,
        invocation_id: EffectInvocationId,
        owner: &OwnerInstanceId,
        execution_scope: u64,
        work: &mut Work,
    ) {
        let call_ids = self
            .pending_transient_effects
            .iter()
            .filter_map(|(call_id, pending)| {
                (pending.invocation_id == invocation_id
                    && &pending.owner == owner
                    && pending.execution_scope == execution_scope)
                    .then_some(*call_id)
            })
            .collect::<Vec<_>>();
        for call_id in call_ids {
            if let Some(previous) = self.pending_transient_effects.remove(&call_id) {
                work.completed_transient_effects.push((call_id, previous));
                work.cancelled_transient_effects.push(call_id);
            }
        }
    }

    fn cancel_pending_transient_effects_for_row(&mut self, row: RowId, work: &mut Work) {
        let call_ids = self
            .pending_transient_effects
            .iter()
            .filter_map(|(call_id, pending)| {
                pending
                    .owner
                    .ancestors
                    .iter()
                    .any(|owner_row| {
                        owner_row.list == row.list
                            && owner_row.key == row.key
                            && owner_row.generation == row.generation
                    })
                    .then_some(*call_id)
            })
            .collect::<Vec<_>>();
        for call_id in call_ids {
            if let Some(previous) = self.pending_transient_effects.remove(&call_id) {
                work.completed_transient_effects.push((call_id, previous));
                work.cancelled_transient_effects.push(call_id);
            }
        }
    }

    fn route_event_with_work(
        &mut self,
        event: &mut SourceEvent,
        demanded_targets: &[ValueTarget],
        work: &mut Work,
    ) -> Result<(), Error> {
        work.effect_reconciliation_sequence = Some(event.sequence);
        let targets = self.event_targets(event, work)?;
        let metadata = Arc::clone(&self.metadata);
        let trigger = self.source_trigger_frame(event)?;

        if let Some(source_fields) = metadata.source_derived_by_source.get(&event.source) {
            for field in source_fields {
                self.mark_root_dirty(*field, work);
            }
            for field in source_fields {
                self.ensure_root_field(*field, Some(event), work)?;
            }
        }
        if let Some(source_lists) = metadata.source_derived_lists_by_source.get(&event.source) {
            for list in source_lists {
                self.mark_list_dirty(*list, work);
            }
            for list in source_lists {
                self.ensure_list_current(*list, Some(event), work)?;
            }
        }

        let scoped_update_row = metadata
            .routes
            .get(&event.source)
            .and_then(|route| route.scope_id)
            .and(event.target);
        if let Some(updates) = metadata.updates_by_source.get(&event.source) {
            for op in updates.iter().filter(|op| !update_branch_has_effect(op)) {
                if op.indexed {
                    let rows = self.indexed_update_targets(op, event, &targets)?;
                    self.execute_indexed_update_batch(op, &rows, event, work)?;
                } else {
                    self.execute_update(
                        op,
                        scoped_update_row,
                        &trigger.for_target(scoped_update_row),
                        work,
                    )?;
                }
            }
        }

        let mutations = metadata
            .mutations_by_source
            .get(&event.source)
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        self.stage_mutation_batch(&mutations, Some(event), &trigger, work)?;

        if let Some(updates) = metadata.updates_by_source.get(&event.source) {
            for op in updates.iter().filter(|op| update_branch_has_effect(op)) {
                if op.indexed {
                    let rows = self.indexed_update_targets(op, event, &targets)?;
                    self.execute_indexed_update_batch(op, &rows, event, work)?;
                } else {
                    self.execute_update(
                        op,
                        scoped_update_row,
                        &trigger.for_target(scoped_update_row),
                        work,
                    )?;
                }
            }
        }

        self.commit_pending_list_mutations(work)?;
        self.ensure_demanded_current(demanded_targets, Some(event), work)?;
        self.reconcile_dirty_effects(work)?;
        self.collect_distributed_invocations_for_trigger(&trigger, work)?;
        Ok(())
    }

    fn validate_event(&self, event: &SourceEvent) -> Result<(), Error> {
        if self.options.require_monotonic_sequences
            && self
                .last_sequence
                .is_some_and(|last| event.sequence <= last)
        {
            return Err(Error::InvalidEvent(format!(
                "sequence {} must be greater than {}",
                event.sequence,
                self.last_sequence.unwrap_or_default()
            )));
        }
        self.validate_event_route(event, false)
    }

    fn source_trigger_frame<'a>(&self, event: &'a SourceEvent) -> Result<TriggerFrame<'a>, Error> {
        let owner = self
            .metadata
            .routes
            .get(&event.source)
            .map(|route| route.owner.clone())
            .ok_or_else(|| Error::InvalidEvent(format!("unknown source {}", event.source.0)))?;
        Ok(TriggerFrame::source(event, owner))
    }

    fn validate_event_route(
        &self,
        event: &SourceEvent,
        _internal_effect_completion: bool,
    ) -> Result<(), Error> {
        if event.route.source != event.source {
            return Err(Error::InvalidEvent(format!(
                "route source {} does not match event source {}",
                event.route.source.0, event.source.0
            )));
        }
        let route_target = self.validate_current_source_route(&event.route)?;
        if route_target != event.target {
            return Err(Error::InvalidEvent(
                "event target does not match its complete owner route".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_current_source_route(
        &self,
        route: &SourceRouteToken,
    ) -> Result<Option<RowId>, Error> {
        route
            .validate()
            .map_err(|detail| Error::InvalidEvent(detail.to_owned()))?;
        if route.program_revision != self.options.program_revision {
            return Err(Error::InvalidEvent(format!(
                "route revision {} is stale; active revision is {}",
                route.program_revision, self.options.program_revision
            )));
        }
        let route_rows = route
            .owner
            .ancestors
            .iter()
            .map(|row| RowId {
                list: row.list,
                key: row.key,
                generation: row.generation,
            })
            .collect::<Vec<_>>();
        let expected = self.source_route_token(route.source, &route_rows)?;
        if expected != *route {
            return Err(Error::InvalidEvent(
                "source route owner, generation, or binding epoch is stale".to_owned(),
            ));
        }
        Ok(route_rows.last().copied())
    }

    fn validate_current_effect_owner(
        &self,
        owner: &OwnerInstanceId,
        target: Option<RowId>,
        call_id: TransientEffectCallId,
    ) -> Result<(), Error> {
        owner
            .validate()
            .map_err(|detail| Error::InvalidEvent(detail.to_owned()))?;
        for ancestor in &owner.ancestors {
            let row = RowId {
                list: ancestor.list,
                key: ancestor.key,
                generation: ancestor.generation,
            };
            if !self
                .lists
                .get(&row.list)
                .is_some_and(|list| list.rows.contains_key(&row))
            {
                return Err(Error::InvalidEvent(format!(
                    "transient effect call {call_id} owner row is stale"
                )));
            }
        }
        if let Some(target) = target
            && !self
                .lists
                .get(&target.list)
                .is_some_and(|list| list.rows.contains_key(&target))
        {
            return Err(Error::InvalidEvent(format!(
                "transient effect call {call_id} result row is stale"
            )));
        }
        Ok(())
    }

    fn event_targets(
        &mut self,
        event: &SourceEvent,
        _work: &mut Work,
    ) -> Result<Vec<RowId>, Error> {
        if let Some(row) = event.target {
            return Ok(vec![row]);
        }
        let route = self
            .metadata
            .routes
            .get(&event.source)
            .cloned()
            .ok_or_else(|| Error::InvalidEvent(format!("unknown source {}", event.source.0)))?;
        if route.scope_id.is_none() {
            return Ok(Vec::new());
        }
        Err(Error::InvalidEvent(format!(
            "scoped source {} has no complete owner route",
            event.source.0
        )))
    }

    fn indexed_update_targets(
        &self,
        op: &PlanOp,
        event: &SourceEvent,
        scoped_targets: &[RowId],
    ) -> Result<Vec<RowId>, Error> {
        let Some(ValueRef::State(state)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "indexed update op {} has no state output",
                op.id.0
            )));
        };
        let owner = *self
            .metadata
            .indexed_state_owner
            .get(&state)
            .ok_or_else(|| Error::InvalidPlan(format!("indexed state {} has no owner", state.0)))?;
        if let Some(target) = event.target {
            return Ok((target.list == owner)
                .then_some(target)
                .into_iter()
                .collect());
        }
        let route = self
            .metadata
            .routes
            .get(&event.source)
            .ok_or_else(|| Error::InvalidEvent(format!("unknown source {}", event.source.0)))?;
        if route.scope_id.is_some() {
            return Ok(scoped_targets
                .iter()
                .copied()
                .filter(|row| row.list == owner)
                .collect());
        }
        Ok(self.list_row_ids(owner))
    }

    fn execute_update(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        trigger: &TriggerFrame<'_>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let PlanOpKind::StateUpdate { effect, .. } = &op.kind else {
            return Err(Error::InvalidPlan(format!(
                "update region op {} is not a state update",
                op.id.0
            )));
        };
        if effect.is_some() {
            return self.stage_effect_invocation(op, row, trigger, work);
        }
        let Some(value) = self.evaluate_update(op, row, trigger.source_event, work)? else {
            return Ok(());
        };
        let Some(ValueRef::State(state)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "update op {} has no state output",
                op.id.0
            )));
        };
        if op.indexed {
            let row = row.ok_or_else(|| {
                Error::InvalidEvent(format!("indexed update op {} has no row target", op.id.0))
            })?;
            let field = *self
                .metadata
                .indexed_state_field
                .get(&state)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!("indexed state {} has no FieldId", state.0))
                })?;
            self.record_row_field_undo(row, field, work);
            let was_touched = !self.touched_row_fields.insert((row, field));
            let changed = self.set_row_authority_field(row, field, value.clone(), work)?;
            if changed || !was_touched {
                work.authority_deltas.push(AuthorityDelta::SetRowField {
                    row,
                    owner_ancestors: self.row_owner_ancestors(row)?.to_vec(),
                    materialization_origin: self
                        .lists
                        .get(&row.list)
                        .and_then(|list| list.rows.get(&row))
                        .and_then(|row| row.materialization_origin.clone()),
                    field,
                    value,
                });
            }
            if changed {
                self.route_state_transition(state, trigger, Some(row), work)?;
            }
        } else {
            self.record_root_undo(state, work);
            let was_touched = !self.touched_root_states.insert(state);
            let changed = self.set_root_state(state, value.clone(), work);
            if changed || !was_touched {
                work.authority_deltas
                    .push(AuthorityDelta::SetRoot { state, value });
            }
            if changed {
                self.route_state_transition(state, trigger, row, work)?;
            }
        }
        Ok(())
    }

    fn execute_indexed_update_batch(
        &mut self,
        op: &PlanOp,
        rows: &[RowId],
        event: &SourceEvent,
        work: &mut Work,
    ) -> Result<(), Error> {
        let PlanOpKind::StateUpdate { effect, .. } = &op.kind else {
            return Err(Error::InvalidPlan(format!(
                "update region op {} is not a state update",
                op.id.0
            )));
        };
        let Some(ValueRef::State(state)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "indexed update op {} has no state output",
                op.id.0
            )));
        };
        if effect.is_some() {
            for row in rows {
                let source_trigger = self.source_trigger_frame(event)?;
                let trigger = self.trigger_for_state_target(state, &source_trigger, *row)?;
                self.stage_effect_invocation(op, Some(*row), &trigger, work)?;
            }
            return Ok(());
        }
        let field = *self
            .metadata
            .indexed_state_field
            .get(&state)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("indexed state {} has no FieldId", state.0))
            })?;
        let mut pending = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(value) = self.evaluate_update(op, Some(*row), Some(event), work)? {
                pending.push((*row, value));
            }
        }
        for (row, value) in pending {
            self.record_row_field_undo(row, field, work);
            let was_touched = self.touched_row_fields.contains(&(row, field));
            self.touched_row_fields.insert((row, field));
            let changed = self.set_row_authority_field(row, field, value.clone(), work)?;
            if changed || !was_touched {
                work.authority_deltas.push(AuthorityDelta::SetRowField {
                    row,
                    owner_ancestors: self.row_owner_ancestors(row)?.to_vec(),
                    materialization_origin: self
                        .lists
                        .get(&row.list)
                        .and_then(|list| list.rows.get(&row))
                        .and_then(|row| row.materialization_origin.clone()),
                    field,
                    value: value.clone(),
                });
            }
            if !changed {
                continue;
            }
            let has_state_consumers = self.metadata.updates_by_state.contains_key(&state)
                || self.metadata.state_derived_by_state.contains_key(&state)
                || self
                    .metadata
                    .state_derived_lists_by_state
                    .contains_key(&state)
                || self.metadata.mutations_by_state.contains_key(&state);
            if has_state_consumers {
                let source_trigger = self.source_trigger_frame(event)?;
                let trigger = self.trigger_for_state_target(state, &source_trigger, row)?;
                self.route_state_transition(state, &trigger, Some(row), work)?;
            }
        }
        Ok(())
    }

    fn record_effect_activation_undo(&self, consumer: EffectConsumer, work: &mut Work) {
        work.effect_activation_undo
            .entry(consumer)
            .or_insert_with(|| self.effect_activations.get(&consumer).cloned());
    }

    fn reconcile_dirty_effects(&mut self, work: &mut Work) -> Result<(), Error> {
        while let Some(consumer) = work.pending_effect_reconciliations.pop_first() {
            let Some(activation) = self.effect_activations.get(&consumer).cloned() else {
                self.clear_consumer_dependencies(Consumer::Effect(consumer));
                continue;
            };
            if consumer.row.is_some_and(|row| !self.row_exists(row)) {
                self.record_effect_activation_undo(consumer, work);
                self.effect_activations.remove(&consumer);
                self.clear_consumer_dependencies(Consumer::Effect(consumer));
                continue;
            }
            let op = self
                .metadata
                .effect_updates_by_id
                .get(&consumer.op)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "active effect consumer references missing update op {}",
                        consumer.op.0
                    ))
                })?;
            let effect = effect_invocation_plan(&op)?;
            if effect.invocation_id != activation.invocation_id {
                return Err(Error::InvalidPlan(format!(
                    "active effect consumer op {} changed invocation identity",
                    consumer.op.0
                )));
            }
            let sequence = work
                .effect_reconciliation_sequence
                .or_else(|| activation.source_event.as_ref().map(|event| event.sequence))
                .unwrap_or_else(|| self.turn_sequence.saturating_add(1));
            let source_event = activation.source_event.clone();
            let trigger = TriggerFrame {
                active: ActiveTrigger {
                    cause: TriggerCause::Effect(effect.invocation_id),
                    owner_plan: effect.owner.clone(),
                    owner: activation.owner,
                    target: consumer.row,
                    sequence,
                },
                source_event: source_event.as_ref(),
            };
            self.stage_effect_invocation(&op, consumer.row, &trigger, work)?;
        }
        Ok(())
    }

    fn rebuild_active_effect_dependencies(&mut self, work: &mut Work) -> Result<(), Error> {
        let activations = self
            .effect_activations
            .iter()
            .map(|(consumer, activation)| (*consumer, activation.clone()))
            .collect::<Vec<_>>();
        for (effect_consumer, activation) in activations {
            if effect_consumer.row.is_some_and(|row| !self.row_exists(row)) {
                self.effect_activations.remove(&effect_consumer);
                continue;
            }
            let op = self
                .metadata
                .effect_updates_by_id
                .get(&effect_consumer.op)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "active effect consumer references missing update op {}",
                        effect_consumer.op.0
                    ))
                })?;
            let effect = effect_invocation_plan(&op)?;
            let source_event = activation.source_event;
            let consumer = Consumer::Effect(effect_consumer);
            self.clear_consumer_dependencies(consumer);
            let gate = self.eval_row_expression(
                &effect.gate,
                effect_consumer
                    .row
                    .or_else(|| source_event.as_ref().and_then(|event| event.target)),
                source_event.as_ref(),
                None,
                Some(consumer),
                &mut PlanLocalBindings::new(),
                work,
            )?;
            if !value_to_bool(&self.materialize_eval(gate)?)? {
                continue;
            }
            for field in &effect.intent_fields {
                self.eval_typed_effect_expression(
                    field.expression,
                    &field.data_type,
                    effect_consumer.row,
                    source_event.as_ref(),
                    consumer,
                    work,
                )?;
            }
        }
        work.pending_effect_reconciliations.clear();
        Ok(())
    }

    fn stage_effect_invocation(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        trigger: &TriggerFrame<'_>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let PlanOpKind::StateUpdate {
            effect: Some(effect),
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "update op {} has no effect invocation plan",
                op.id.0
            )));
        };
        let contract = self
            .plan
            .effects
            .iter()
            .find(|contract| contract.effect_id == effect.effect_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect invocation {} has no effect contract",
                    effect.invocation_id
                ))
            })?;
        let schema = contract.schema.as_ref().ok_or_else(|| {
            Error::InvalidPlan(format!(
                "effect invocation {} has no typed schema",
                effect.invocation_id
            ))
        })?;
        let result_row = if op.indexed {
            Some(row.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed effect invocation {} has no durable row target",
                    effect.invocation_id
                ))
            })?)
        } else {
            None
        };
        let owner = instantiate_plan_owner(&effect.owner, &trigger.active)?;
        if let Some(result_row) = result_row {
            let leaf = owner.leaf().ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "indexed effect invocation {} has no repeated owner",
                    effect.invocation_id
                ))
            })?;
            if leaf.list != result_row.list
                || leaf.key != result_row.key
                || leaf.generation != result_row.generation
            {
                return Err(Error::InvalidPlan(format!(
                    "indexed effect invocation {} result row does not match its complete owner",
                    effect.invocation_id
                )));
            }
        }
        let effect_consumer = EffectConsumer { op: op.id, row };
        let superseded = self
            .effect_activations
            .iter()
            .filter_map(|(candidate, activation)| {
                (*candidate != effect_consumer
                    && activation.invocation_id == effect.invocation_id
                    && activation.owner == owner)
                    .then_some(*candidate)
            })
            .collect::<Vec<_>>();
        for candidate in superseded {
            self.record_effect_activation_undo(candidate, work);
            self.effect_activations.remove(&candidate);
            self.clear_consumer_dependencies(Consumer::Effect(candidate));
            work.pending_effect_reconciliations.remove(&candidate);
        }
        self.record_effect_activation_undo(effect_consumer, work);
        self.effect_activations.insert(
            effect_consumer,
            EffectActivation {
                invocation_id: effect.invocation_id,
                owner: owner.clone(),
                source_event: trigger.source_event.cloned(),
            },
        );
        self.clear_consumer_dependencies(Consumer::Effect(effect_consumer));
        work.pending_effect_reconciliations.remove(&effect_consumer);
        let consumer = Consumer::Effect(effect_consumer);
        let gate = self.eval_row_expression(
            &effect.gate,
            row.or_else(|| trigger.source_event.and_then(|event| event.target)),
            trigger.source_event,
            None,
            Some(consumer),
            &mut PlanLocalBindings::new(),
            work,
        )?;
        if !value_to_bool(&self.materialize_eval(gate)?)? {
            self.cancel_pending_transient_effect_owner(
                effect.invocation_id,
                &owner,
                self.transient_effect_scope,
                work,
            );
            return Ok(());
        }
        let intent_values = effect
            .intent_fields
            .iter()
            .map(|field| {
                let value = self.eval_typed_effect_expression(
                    field.expression,
                    &field.data_type,
                    row,
                    trigger.source_event,
                    consumer,
                    work,
                )?;
                Ok((field.name.clone(), value))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let transient_intent = Value::Record(intent_values.clone());
        validate_value_for_data_type(&transient_intent, &schema.intent_type, "effect intent")?;
        let sequence = self
            .turn_sequence
            .checked_add(1)
            .ok_or_else(|| Error::Evaluation("authority turn sequence overflow".to_owned()))?;
        if effect_replay_is_transient(&contract.replay) {
            let call_id = TransientEffectCallId {
                launch_epoch: self.launch_epoch,
                sequence: self.next_transient_effect_sequence,
            };
            self.next_transient_effect_sequence = self
                .next_transient_effect_sequence
                .checked_add(1)
                .ok_or_else(|| {
                    Error::Evaluation("transient effect sequence exhausted".to_owned())
                })?;
            work.consume(1)?;
            work.transient_effects.push(TransientEffectInvocation {
                call_id,
                invocation_id: effect.invocation_id,
                effect_id: effect.effect_id,
                trigger_sequence: trigger.active.sequence,
                authority_turn_sequence: sequence,
                owner,
                target: result_row,
                intent: transient_intent,
                delivery: contract.delivery.clone(),
            });
            return Ok(());
        }
        if !matches!(contract.replay, boon_plan::EffectReplay::Idempotent { .. }) {
            return Err(Error::InvalidPlan(format!(
                "effect invocation {} has no executable replay policy",
                effect.invocation_id
            )));
        }
        let intent = boon_persistence::StoredValue::Record(
            effect
                .intent_fields
                .iter()
                .map(|field| {
                    let value = intent_values.get(&field.name).ok_or_else(|| {
                        Error::InvalidPlan(format!(
                            "effect invocation {} lost intent field `{}`",
                            effect.invocation_id, field.name
                        ))
                    })?;
                    Ok((
                        field.name.clone(),
                        stored_value_for_data_type(value, &field.data_type)?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, Error>>()?,
        );
        let target_row = result_row
            .map(|row| {
                Ok(boon_persistence::DurableRowId {
                    list_memory_id: self.persistence_list(row.list)?.memory_id,
                    row_key: row.key,
                    row_generation: row.generation,
                })
            })
            .transpose()?;
        let durable_owner = self.durable_owner(&owner)?;
        let idempotency_key = match effect.idempotency_key {
            boon_plan::EffectIdempotencyKeyPlan::InvocationTurnIntentSha256 => {
                boon_persistence::canonical_intent_key(&boon_persistence::StoredValue::Record(
                    BTreeMap::from([
                        (
                            "authority_turn_sequence".to_owned(),
                            boon_persistence::StoredValue::Text(sequence.to_string()),
                        ),
                        ("canonical_intent".to_owned(), intent.clone()),
                        (
                            "invocation_id".to_owned(),
                            boon_persistence::StoredValue::Text(effect.invocation_id.to_string()),
                        ),
                        ("owner".to_owned(), durable_owner_value(&durable_owner)),
                        (
                            "trigger_sequence".to_owned(),
                            boon_persistence::StoredValue::Text(
                                trigger.active.sequence.to_string(),
                            ),
                        ),
                    ]),
                ))
            }
        };
        let item = boon_persistence::DurableOutboxItem::pending(
            effect.invocation_id,
            effect.effect_id,
            idempotency_key,
            intent,
            durable_owner,
            target_row,
            sequence,
        );
        let schema = self
            .plan
            .persistence
            .effect_outbox
            .iter()
            .find(|schema| schema.effect_id == effect.effect_id)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "effect {} has no durable outbox schema",
                    effect.effect_id
                ))
            })?;
        boon_persistence::validate_outbox_item_schema(&item, schema)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        work.outbox_changes
            .push(boon_persistence::DurableOutboxChange::Enqueue { item });
        Ok(())
    }

    fn ensure_root_field(
        &mut self,
        field: FieldId,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        self.flush_list_access_dependencies(work)?;
        let currentness = self
            .root_fields
            .get(&field)
            .map(|cell| cell.currentness)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("field {} has no root computation", field.0))
            })?;
        match currentness {
            Currentness::Current => {
                return self
                    .root_fields
                    .get(&field)
                    .and_then(|cell| cell.value.clone())
                    .ok_or_else(|| {
                        Error::Evaluation(format!("current root field {} has no value", field.0))
                    });
            }
            Currentness::Evaluating => return Err(Error::Cycle { field, row: None }),
            Currentness::Dirty => {}
        }
        work.consume(1)?;
        self.root_fields
            .get_mut(&field)
            .expect("root cell checked above")
            .currentness = Currentness::Evaluating;
        let consumer = Consumer::Root(field);
        self.clear_consumer_dependencies(consumer);
        let op = self
            .metadata
            .root_computations
            .get(&field)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("root field {} has no plan op", field.0)))?;
        let evaluated = self.evaluate_root_computation(field, &op, event, work);
        let value = match evaluated {
            Ok(value) => value,
            Err(error) => {
                self.root_fields
                    .get_mut(&field)
                    .expect("root cell checked above")
                    .currentness = Currentness::Dirty;
                return Err(error);
            }
        };
        let old = self
            .root_fields
            .get(&field)
            .and_then(|cell| cell.value.clone());
        {
            let cell = self.root_fields.get_mut(&field).expect("root cell exists");
            cell.value = Some(value.clone());
            cell.currentness = Currentness::Current;
        }
        work.metrics.recomputed_field_count += 1;
        work.recomputed_targets.insert(ValueTarget::Field(field));
        if old.as_ref() != Some(&value) {
            self.invalidate_root_field(field, work);
            if work.emit {
                work.deltas.push(Delta::SetValue {
                    target: ValueTarget::Field(field),
                    value: value.clone(),
                });
            }
        }
        Ok(value)
    }

    fn ensure_list_current(
        &mut self,
        list: ListId,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Vec<EvalValue>, Error> {
        self.flush_list_access_dependencies(work)?;
        let currentness = self
            .derived_lists
            .get(&list)
            .map(|cell| cell.currentness)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("list {} has no derived computation", list.0))
            })?;
        match currentness {
            Currentness::Current
                if self
                    .derived_lists
                    .get(&list)
                    .is_some_and(|cell| cell.items.is_some()) =>
            {
                return self
                    .derived_lists
                    .get(&list)
                    .and_then(|cell| cell.items.clone())
                    .ok_or_else(|| {
                        Error::Evaluation(format!("current derived list {} has no items", list.0))
                    });
            }
            Currentness::Evaluating => return Err(Error::ListCycle { list }),
            Currentness::Current | Currentness::Dirty => {}
        }
        work.consume(1)?;
        let virtual_rows = {
            let cell = self
                .derived_lists
                .get_mut(&list)
                .expect("derived list checked above");
            cell.currentness = Currentness::Evaluating;
            cell.items = None;
            cell.window
                .take()
                .map(|window| window.rows_by_index.into_values().collect::<Vec<_>>())
                .unwrap_or_default()
        };
        for row in virtual_rows.into_iter().rev() {
            if self.row_exists(row)
                && let Err(error) = self.remove_row(row, work)
            {
                self.derived_lists
                    .get_mut(&list)
                    .expect("derived list checked above")
                    .currentness = Currentness::Dirty;
                return Err(error);
            }
        }
        let consumer = Consumer::List(list);
        self.clear_consumer_dependencies(consumer);
        let op = self
            .metadata
            .list_computations
            .get(&list)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("derived list {} has no plan op", list.0)))?;
        let evaluated = self.evaluate_list_computation(list, &op, event, work);
        let items = match evaluated {
            Ok(items) => items,
            Err(error) => {
                self.derived_lists
                    .get_mut(&list)
                    .expect("derived list checked above")
                    .currentness = Currentness::Dirty;
                return Err(error);
            }
        };
        let old = self
            .derived_lists
            .get(&list)
            .and_then(|cell| cell.items.clone());
        {
            let cell = self
                .derived_lists
                .get_mut(&list)
                .expect("derived list checked above");
            cell.items = Some(items.clone());
            cell.window = None;
            cell.currentness = Currentness::Current;
        }
        work.metrics.recomputed_list_count += 1;
        if old.as_ref() != Some(&items) {
            self.invalidate_list_structure(list, work);
        }
        Ok(items)
    }

    fn chunk_projection(&self, list: ListId) -> Option<(ListId, usize)> {
        if self
            .metadata
            .indexed_state_owner
            .values()
            .any(|owner| *owner == list)
        {
            return None;
        }
        self.metadata
            .list_computations
            .get(&list)
            .and_then(|op| match &op.kind {
                PlanOpKind::ListProjection {
                    projection: PlanListProjection::Chunk { source_list, size },
                } => Some((*source_list, *size)),
                _ => None,
            })
    }

    fn list_logical_len_with_work(
        &mut self,
        list: ListId,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<u64, Error> {
        self.register_list_dependency(consumer, list);
        if let Some((source_list, size)) = self.chunk_projection(list) {
            return self.ensure_chunk_logical_len_current(list, source_list, size, work);
        }
        let logical_len = if self.metadata.list_computations.contains_key(&list) {
            self.ensure_list_current(list, None, work)?.len()
        } else {
            self.lists.get(&list).map_or(0, |state| state.order.len())
        };
        u64::try_from(logical_len).map_err(|_| {
            Error::Evaluation("list row count does not fit the logical key space".to_owned())
        })
    }

    fn list_eval_window_with_work(
        &mut self,
        list: ListId,
        range: Range<u64>,
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<(u64, Vec<EvalValue>), Error> {
        if range.start > range.end {
            return Err(Error::Evaluation(
                "list value window has an inverted range".to_owned(),
            ));
        }
        self.register_list_dependency(consumer, list);
        if let Some((source_list, size)) = self.chunk_projection(list) {
            return self.ensure_chunk_window_current(list, source_list, size, range, event, work);
        }
        if self.metadata.list_computations.contains_key(&list) {
            let items = self.ensure_list_current(list, event, work)?;
            let logical_len = u64::try_from(items.len()).map_err(|_| {
                Error::Evaluation("list row count does not fit the logical key space".to_owned())
            })?;
            let start = usize::try_from(range.start)
                .unwrap_or(usize::MAX)
                .min(items.len());
            let end = usize::try_from(range.end)
                .unwrap_or(usize::MAX)
                .min(items.len());
            return Ok((
                logical_len,
                items
                    .into_iter()
                    .skip(start)
                    .take(end.saturating_sub(start))
                    .collect(),
            ));
        }
        let Some(state) = self.lists.get(&list) else {
            return Ok((0, Vec::new()));
        };
        let logical_len = state.order.len();
        let start = usize::try_from(range.start)
            .unwrap_or(usize::MAX)
            .min(logical_len);
        let end = usize::try_from(range.end)
            .unwrap_or(usize::MAX)
            .min(logical_len);
        let rows = state
            .order
            .range(start..end)
            .into_iter()
            .map(EvalValue::Row)
            .collect();
        Ok((
            u64::try_from(logical_len).map_err(|_| {
                Error::Evaluation("list row count does not fit the logical key space".to_owned())
            })?,
            rows,
        ))
    }

    fn ensure_chunk_logical_len_current(
        &mut self,
        list: ListId,
        source_list: ListId,
        size: usize,
        work: &mut Work,
    ) -> Result<u64, Error> {
        if size == 0 {
            return Err(Error::InvalidPlan(format!(
                "chunk projection for list {} has size zero",
                list.0
            )));
        }
        let cell = self
            .derived_lists
            .get(&list)
            .ok_or_else(|| Error::InvalidPlan(format!("list {} has no derived cache", list.0)))?;
        match cell.currentness {
            Currentness::Current => {
                if let Some(window) = &cell.window {
                    return Ok(window.logical_len);
                }
                if let Some(items) = &cell.items {
                    return u64::try_from(items.len()).map_err(|_| {
                        Error::Evaluation(
                            "chunk row count does not fit the logical key space".to_owned(),
                        )
                    });
                }
            }
            Currentness::Evaluating => return Err(Error::ListCycle { list }),
            Currentness::Dirty => {}
        }

        work.consume(1)?;
        let (mut window, had_full_items) = {
            let cell = self
                .derived_lists
                .get_mut(&list)
                .expect("derived chunk cache checked above");
            cell.currentness = Currentness::Evaluating;
            let had_full_items = cell.items.take().is_some();
            let window = cell.window.take().unwrap_or(DerivedListWindow {
                logical_len: 0,
                values_current: false,
                rows_by_index: BTreeMap::new(),
            });
            (window, had_full_items)
        };
        let full_rows = had_full_items
            .then(|| self.list_row_ids(list))
            .unwrap_or_default();
        for row in full_rows.into_iter().rev() {
            if self.row_exists(row)
                && let Err(error) = self.remove_row(row, work)
            {
                self.derived_lists
                    .get_mut(&list)
                    .expect("derived chunk cache checked above")
                    .currentness = Currentness::Dirty;
                return Err(error);
            }
        }

        let consumer = Consumer::List(list);
        self.clear_consumer_dependencies(consumer);
        let source_len = match self.list_logical_len_with_work(source_list, Some(consumer), work) {
            Ok(source_len) => source_len,
            Err(error) => {
                let cell = self
                    .derived_lists
                    .get_mut(&list)
                    .expect("derived chunk cache checked above");
                window.values_current = false;
                cell.window = Some(window);
                cell.currentness = Currentness::Dirty;
                return Err(error);
            }
        };
        let size = u64::try_from(size)
            .map_err(|_| Error::InvalidPlan("chunk size does not fit u64".to_owned()))?;
        let logical_len = source_len
            .checked_add(size.saturating_sub(1))
            .ok_or_else(|| Error::Evaluation("chunk logical length overflowed".to_owned()))?
            / size;
        window.logical_len = logical_len;
        window.values_current = false;
        let stale_rows = window
            .rows_by_index
            .iter()
            .filter_map(|(index, row)| {
                (*index >= logical_len && self.row_exists(*row)).then_some(*row)
            })
            .collect::<Vec<_>>();
        for row in stale_rows.into_iter().rev() {
            if let Err(error) = self.remove_row(row, work) {
                let cell = self
                    .derived_lists
                    .get_mut(&list)
                    .expect("derived chunk cache checked above");
                cell.window = Some(window);
                cell.currentness = Currentness::Dirty;
                return Err(error);
            }
        }
        window
            .rows_by_index
            .retain(|index, row| *index < logical_len && self.row_exists(*row));
        {
            let cell = self
                .derived_lists
                .get_mut(&list)
                .expect("derived chunk cache checked above");
            cell.window = Some(window);
            cell.currentness = Currentness::Current;
        }
        work.metrics.recomputed_list_count += 1;
        Ok(logical_len)
    }

    fn ensure_chunk_window_current(
        &mut self,
        list: ListId,
        source_list: ListId,
        size: usize,
        range: Range<u64>,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<(u64, Vec<EvalValue>), Error> {
        let logical_len = self.ensure_chunk_logical_len_current(list, source_list, size, work)?;
        let range = range.start.min(logical_len)..range.end.min(logical_len);
        if self.derived_lists.get(&list).is_some_and(|cell| {
            cell.currentness == Currentness::Current
                && cell.window.as_ref().is_some_and(|window| {
                    window.values_current
                        && (range.start..range.end)
                            .all(|index| window.rows_by_index.contains_key(&index))
                })
        }) {
            let rows = self
                .derived_lists
                .get(&list)
                .and_then(|cell| cell.window.as_ref())
                .expect("covered chunk window exists")
                .rows_by_index
                .range(range.clone())
                .map(|(_, row)| EvalValue::Row(*row))
                .collect();
            return Ok((logical_len, rows));
        }

        {
            let cell = self
                .derived_lists
                .get_mut(&list)
                .expect("derived chunk cache checked above");
            if cell.currentness == Currentness::Evaluating {
                return Err(Error::ListCycle { list });
            }
            cell.currentness = Currentness::Evaluating;
        }
        let consumer = Consumer::List(list);
        self.clear_consumer_dependencies(consumer);
        let result =
            self.reconcile_chunk_window(list, source_list, size, logical_len, range, event, work);
        match result {
            Ok((rows_by_index, rows)) => {
                let cell = self
                    .derived_lists
                    .get_mut(&list)
                    .expect("derived chunk cache checked above");
                cell.items = None;
                cell.window = Some(DerivedListWindow {
                    logical_len,
                    values_current: true,
                    rows_by_index,
                });
                cell.currentness = Currentness::Current;
                work.metrics.recomputed_list_count += 1;
                Ok((logical_len, rows))
            }
            Err(error) => {
                let cell = self
                    .derived_lists
                    .get_mut(&list)
                    .expect("derived chunk cache checked above");
                if let Some(window) = cell.window.as_mut() {
                    window.values_current = false;
                }
                cell.currentness = Currentness::Dirty;
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile_chunk_window(
        &mut self,
        list: ListId,
        source_list: ListId,
        size: usize,
        logical_len: u64,
        range: Range<u64>,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<(BTreeMap<u64, RowId>, Vec<EvalValue>), Error> {
        let size_u64 = u64::try_from(size)
            .map_err(|_| Error::InvalidPlan("chunk size does not fit u64".to_owned()))?;
        let source_start = range
            .start
            .checked_mul(size_u64)
            .ok_or_else(|| Error::Evaluation("chunk source window overflowed".to_owned()))?;
        let source_end = range
            .end
            .checked_mul(size_u64)
            .ok_or_else(|| Error::Evaluation("chunk source window overflowed".to_owned()))?;
        let (source_logical_len, source_items) = self.list_eval_window_with_work(
            source_list,
            source_start..source_end,
            event,
            Some(Consumer::List(list)),
            work,
        )?;
        let expected_logical_len = source_logical_len
            .checked_add(size_u64.saturating_sub(1))
            .ok_or_else(|| Error::Evaluation("chunk logical length overflowed".to_owned()))?
            / size_u64;
        if expected_logical_len != logical_len {
            return Err(Error::Evaluation(
                "chunk source changed during one bounded currentness read".to_owned(),
            ));
        }

        let mut desired = BTreeMap::new();
        for (offset, chunk) in source_items.chunks(size).enumerate() {
            let index = range
                .start
                .checked_add(u64::try_from(offset).unwrap_or(u64::MAX))
                .ok_or_else(|| Error::Evaluation("chunk index overflowed".to_owned()))?;
            let fields = self.projected_record_fields(
                list,
                BTreeMap::from([
                    (
                        "label".to_owned(),
                        EvalValue::Value(Value::Text(index.to_string())),
                    ),
                    ("items".to_owned(), EvalValue::List(chunk.to_vec())),
                ]),
            )?;
            desired.insert(index, fields);
        }
        if desired.len() != range.end.saturating_sub(range.start) as usize {
            return Err(Error::Evaluation(
                "chunk source window did not produce every requested logical row".to_owned(),
            ));
        }

        let mut rows_by_index = self
            .derived_lists
            .get(&list)
            .and_then(|cell| cell.window.as_ref())
            .map(|window| window.rows_by_index.clone())
            .unwrap_or_default();
        let desired_indices = desired.keys().copied().collect::<BTreeSet<_>>();
        let removed = rows_by_index
            .iter()
            .filter_map(|(index, row)| (!desired_indices.contains(index)).then_some((*index, *row)))
            .collect::<Vec<_>>();
        for (index, row) in removed.into_iter().rev() {
            rows_by_index.remove(&index);
            if self.row_exists(row) {
                self.remove_row(row, work)?;
            }
        }

        for (index, fields) in desired {
            let existing = rows_by_index
                .get(&index)
                .copied()
                .filter(|row| self.row_exists(*row));
            let row = if let Some(row) = existing {
                row
            } else {
                self.materialize_virtual_projection_row(list, index, fields.clone(), work)?
            };
            rows_by_index.insert(index, row);
            for (field, value) in fields {
                let unchanged = self
                    .lists
                    .get(&list)
                    .and_then(|state| state.rows.get(&row))
                    .and_then(|state| state.fields.get(&field))
                    == Some(&value);
                if unchanged {
                    continue;
                }
                self.record_row_field_undo(row, field, work);
                self.set_row_authority_field(row, field, value, work)?;
            }
        }

        let ordered_rows = rows_by_index.values().copied().collect::<Vec<_>>();
        if !ordered_rows.is_empty()
            && self.set_materialized_partition_order(list, 0, &ordered_rows, work)?
        {
            self.invalidate_list_structure(list, work);
        }
        let rows = rows_by_index
            .range(range)
            .map(|(_, row)| EvalValue::Row(*row))
            .collect();
        Ok((rows_by_index, rows))
    }

    fn ensure_published_current(
        &mut self,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let fields = self.metadata.published.iter().copied().collect::<Vec<_>>();
        for _ in 0..=fields.len() {
            let dirty = fields
                .iter()
                .copied()
                .filter(|field| {
                    self.root_fields
                        .get(field)
                        .is_some_and(|cell| cell.currentness != Currentness::Current)
                })
                .collect::<Vec<_>>();
            if dirty.is_empty() {
                return Ok(());
            }
            for field in dirty {
                self.ensure_root_field(field, event, work)?;
            }
        }
        Err(Error::Evaluation(
            "published fields did not converge at the currentness barrier".to_owned(),
        ))
    }

    fn ensure_demanded_current(
        &mut self,
        targets: &[ValueTarget],
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let max_passes = self
            .metadata
            .published
            .len()
            .saturating_add(targets.len())
            .saturating_add(1);
        for _ in 0..max_passes {
            self.ensure_published_current(event, work)?;
            for target in targets {
                match *target {
                    ValueTarget::State(_) => {}
                    ValueTarget::Field(field) => {
                        self.ensure_root_field(field, event, work)?;
                    }
                    ValueTarget::RowField { row, field } => {
                        if self.row_exists(row) {
                            let field = self.resolve_row_field_alias(row, field);
                            if self.metadata.row_computations.contains_key(&field) {
                                self.ensure_row_field(row, field, event, work)?;
                            } else {
                                self.row_value(row, field)?;
                            }
                        }
                    }
                }
            }
            self.ensure_published_current(event, work)?;
            self.flush_list_access_dependencies(work)?;
            self.ensure_all_ordered_indexes_current(work)?;
            let all_current = targets.iter().all(|target| match *target {
                ValueTarget::State(_) => true,
                ValueTarget::Field(field) => self
                    .root_fields
                    .get(&field)
                    .is_some_and(|cell| cell.currentness == Currentness::Current),
                ValueTarget::RowField { row, field } => {
                    let field = self.resolve_row_field_alias(row, field);
                    !self.row_exists(row)
                        || self
                            .lists
                            .get(&row.list)
                            .and_then(|list| list.rows.get(&row))
                            .is_some_and(|row| {
                                row.derived
                                    .get(&field)
                                    .is_none_or(|currentness| *currentness == Currentness::Current)
                            })
                }
            });
            if all_current {
                return Ok(());
            }
        }
        Err(Error::Evaluation(
            "document demands did not converge at the currentness barrier".to_owned(),
        ))
    }

    fn ensure_row_field(
        &mut self,
        row: RowId,
        field: FieldId,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        self.flush_list_access_dependencies(work)?;
        let field = self.resolve_row_field_alias(row, field);
        if self.touched_row_fields.contains(&(row, field))
            && self
                .lists
                .get(&row.list)
                .and_then(|list| list.rows.get(&row))
                .is_some_and(|row| row.default_fields.contains(&field))
        {
            return self.row_value(row, field);
        }
        let currentness = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.derived.get(&field))
            .copied();
        let Some(currentness) = currentness else {
            return self.row_value(row, field);
        };
        match currentness {
            Currentness::Current => return self.row_value(row, field),
            Currentness::Evaluating => {
                return Err(Error::Cycle {
                    field,
                    row: Some(row),
                });
            }
            Currentness::Dirty => {}
        }
        work.consume(1)?;
        self.set_row_currentness(row, field, Currentness::Evaluating)?;
        let consumer = Consumer::Row(row, field);
        self.clear_consumer_dependencies(consumer);
        let op = self.metadata.row_computations.get(&field).cloned();
        let default = op
            .is_none()
            .then(|| self.row_default_expression(row, field))
            .flatten();
        let evaluated = if let Some(op) = op {
            self.evaluate_derived_op(&op, Some(row), event, work)
        } else if let Some(expression) = default {
            self.eval_row_expression(
                expression,
                Some(row),
                event,
                Some(field),
                Some(consumer),
                &mut BTreeMap::new(),
                work,
            )
        } else {
            Err(Error::InvalidPlan(format!(
                "row field {} has no plan op or row default",
                field.0
            )))
        };
        let value = match evaluated.and_then(|value| self.materialize_eval(value)) {
            Ok(value) => value,
            Err(error) => {
                self.set_row_currentness(row, field, Currentness::Dirty)?;
                return Err(error);
            }
        };
        self.set_row_field(row, field, value.clone(), work)?;
        self.set_row_currentness(row, field, Currentness::Current)?;
        work.metrics.recomputed_field_count += 1;
        work.recomputed_targets
            .insert(ValueTarget::RowField { row, field });
        Ok(value)
    }

    fn row_default_expression(&self, row: RowId, field: FieldId) -> Option<PlanRowExpressionId> {
        let state = self.lists.get(&row.list)?.rows.get(&row)?;
        if !state.default_fields.contains(&field) || row.generation != 1 || row.key == 0 {
            return None;
        }
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == row.list)?;
        let row_index = usize::try_from(row.key.checked_sub(1)?).ok()?;
        slot.initial_rows
            .get(row_index)?
            .fields
            .iter()
            .find(|candidate| candidate.field_id == Some(field))?
            .initializer
            .expression()
    }

    fn evaluate_root_computation(
        &mut self,
        field: FieldId,
        op: &PlanOp,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let value = match &op.kind {
            PlanOpKind::DerivedValue { .. } => {
                let value = self.evaluate_derived_op(op, None, event, work)?;
                self.materialize_eval(value)?
            }
            PlanOpKind::ListProjection { projection } => {
                let value = self.evaluate_projection(
                    ValueRef::Field(field),
                    op.id,
                    projection,
                    event,
                    work,
                )?;
                self.materialize_eval(value)?
            }
            _ => {
                return Err(Error::Unsupported {
                    op: op.id,
                    detail: "operation cannot produce a root field".to_owned(),
                });
            }
        };
        Ok(value)
    }

    fn evaluate_list_computation(
        &mut self,
        list: ListId,
        op: &PlanOp,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Vec<EvalValue>, Error> {
        let evaluated = match &op.kind {
            PlanOpKind::DerivedValue { .. } => self.evaluate_derived_op(op, None, event, work)?,
            PlanOpKind::ListProjection { projection } => {
                let projected =
                    self.evaluate_projection(ValueRef::List(list), op.id, projection, event, work)?;
                self.reconcile_positional_list_projection(list, projected, work)?
            }
            _ => {
                return Err(Error::Unsupported {
                    op: op.id,
                    detail: "operation cannot produce a derived list".to_owned(),
                });
            }
        };
        match evaluated {
            EvalValue::List(items) => Ok(items),
            _ => Err(Error::InvalidPlan(format!(
                "list computation {} did not produce a list",
                op.id.0
            ))),
        }
    }

    fn reconcile_positional_list_projection(
        &mut self,
        list: ListId,
        projected: EvalValue,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let items = eval_to_list(projected)?;
        let desired = items
            .into_iter()
            .map(|item| {
                let fields = match item {
                    EvalValue::Record(fields) => fields,
                    EvalValue::Value(Value::Record(fields)) => fields
                        .into_iter()
                        .map(|(name, value)| (name, EvalValue::Value(value)))
                        .collect(),
                    other => {
                        return Err(Error::Evaluation(format!(
                            "list projection target {} produced non-record row {other:?}",
                            list.0
                        )));
                    }
                };
                self.projected_record_fields(list, fields)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let existing = self.list_row_ids_for_owner(list, &[])?;
        let capacity = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .ok_or_else(|| Error::InvalidPlan(format!("missing list slot {}", list.0)))?
            .capacity;
        if capacity.is_some_and(|capacity| desired.len() > capacity) {
            return Err(Error::Evaluation(format!(
                "list projection target {} would contain {} rows, exceeding capacity {}",
                list.0,
                desired.len(),
                capacity.unwrap_or_default()
            )));
        }
        work.consume(
            existing
                .len()
                .saturating_add(desired.len())
                .try_into()
                .unwrap_or(u64::MAX),
        )?;

        let common = existing.len().min(desired.len());
        for (row, fields) in existing.iter().copied().zip(&desired).take(common) {
            for (field, value) in fields {
                let unchanged = self
                    .lists
                    .get(&list)
                    .and_then(|state| state.rows.get(&row))
                    .and_then(|state| state.fields.get(field))
                    == Some(value);
                if unchanged {
                    continue;
                }
                self.record_row_field_undo(row, *field, work);
                self.set_row_authority_field(row, *field, value.clone(), work)?;
            }
        }
        for row in existing.iter().copied().skip(desired.len()).rev() {
            self.remove_row(row, work)?;
        }
        let mut rows = existing.into_iter().take(common).collect::<Vec<_>>();
        for fields in desired.into_iter().skip(common) {
            rows.push(self.append_row_with_owner_prefix(list, fields, &[], None, work)?);
        }
        Ok(EvalValue::List(
            rows.into_iter().map(EvalValue::Row).collect(),
        ))
    }

    fn projected_record_fields(
        &mut self,
        list: ListId,
        fields: BTreeMap<String, EvalValue>,
    ) -> Result<BTreeMap<FieldId, Value>, Error> {
        let fields_by_name = self
            .metadata
            .row_field_names
            .iter()
            .filter_map(|((owner, field), name)| (*owner == list).then(|| (name.clone(), *field)))
            .collect::<BTreeMap<_, _>>();
        if fields_by_name.is_empty() {
            return Err(Error::InvalidPlan(format!(
                "list projection target {} has no typed row fields",
                list.0
            )));
        }
        let fields = fields
            .into_iter()
            .map(|(name, value)| {
                let field = fields_by_name.get(&name).copied().ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "list projection target {} record field `{name}` has no compiled FieldId",
                        list.0
                    ))
                })?;
                Ok((field, self.materialize_eval(value)?))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        self.materialized_constructor_fields(list, fields)
    }

    fn evaluate_derived_op(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let PlanOpKind::DerivedValue {
            derived_kind,
            expression,
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "op {} is not a derived value",
                op.id.0
            )));
        };
        let Some(_) = expression else {
            let output_label = match op.output.as_ref() {
                Some(ValueRef::Field(field)) => {
                    debug_label(&self.plan.debug_map.fields, "field:", field.0)
                }
                _ => None,
            };
            return Err(Error::Unsupported {
                op: op.id,
                detail: format!(
                    "{derived_kind:?} derived value has no typed expression; output={:?}, path={}",
                    op.output,
                    output_label.unwrap_or("<unknown>")
                ),
            });
        };
        self.eval_expression_entry(
            ExpressionEntry::CurrentnessOp {
                op: op.id,
                row,
                event,
            },
            &mut BTreeMap::new(),
            work,
        )
        .map_err(|error| match error {
            Error::Evaluation(detail) => Error::Evaluation(format!(
                "derived op {} output {:?}: {detail}",
                op.id.0, op.output
            )),
            error => error,
        })
    }
}

fn update_branch_has_effect(op: &PlanOp) -> bool {
    matches!(
        &op.kind,
        PlanOpKind::StateUpdate {
            effect: Some(_),
            ..
        }
    )
}

fn effect_invocation_id(op: &PlanOp) -> Result<EffectInvocationId, Error> {
    Ok(effect_invocation_plan(op)?.invocation_id)
}

fn effect_invocation_plan(op: &PlanOp) -> Result<&EffectInvocationPlan, Error> {
    match &op.kind {
        PlanOpKind::StateUpdate {
            effect: Some(effect),
            ..
        } => Ok(effect),
        _ => Err(Error::InvalidPlan(format!(
            "update op {} has no effect invocation identity",
            op.id.0
        ))),
    }
}

fn sort_update_ops_by_dependencies(ops: &mut Vec<Arc<PlanOp>>) {
    ops.sort_by_key(|op| op.id);
    let mut producers = BTreeMap::<StateId, Vec<usize>>::new();
    for (index, op) in ops.iter().enumerate() {
        if update_branch_has_effect(op) {
            continue;
        }
        if let Some(ValueRef::State(state)) = op.output {
            producers.entry(state).or_default().push(index);
        }
    }

    let mut outgoing = vec![BTreeSet::<usize>::new(); ops.len()];
    let mut incoming = vec![0_usize; ops.len()];
    for (consumer, op) in ops.iter().enumerate() {
        let output = match op.output {
            Some(ValueRef::State(state)) => Some(state),
            _ => None,
        };
        for dependency in op.inputs.iter().filter_map(|input| match input {
            ValueRef::State(state) => Some(*state),
            _ => None,
        }) {
            if output == Some(dependency) {
                continue;
            }
            for producer in producers.get(&dependency).into_iter().flatten() {
                if *producer != consumer && outgoing[*producer].insert(consumer) {
                    incoming[consumer] += 1;
                }
            }
        }
    }

    let mut ready = BTreeSet::<(PlanOpId, usize)>::new();
    for (index, count) in incoming.iter().enumerate() {
        if *count == 0 {
            ready.insert((ops[index].id, index));
        }
    }
    let mut order = Vec::with_capacity(ops.len());
    while let Some((_, index)) = ready.pop_first() {
        order.push(index);
        for consumer in outgoing[index].iter().copied() {
            incoming[consumer] -= 1;
            if incoming[consumer] == 0 {
                ready.insert((ops[consumer].id, consumer));
            }
        }
    }

    if order.len() != ops.len() {
        let scheduled = order.iter().copied().collect::<BTreeSet<_>>();
        order.extend((0..ops.len()).filter(|index| !scheduled.contains(index)));
    }
    let previous = ops.clone();
    *ops = order
        .into_iter()
        .map(|index| Arc::clone(&previous[index]))
        .collect();
}

impl MachineInstance {
    fn resolve_row_field_alias(&self, row: RowId, field: FieldId) -> FieldId {
        let Some(state) = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
        else {
            return field;
        };
        if let Some(authority) = self
            .metadata
            .deferred_value_aliases
            .get(&(row.list, field))
            .copied()
            && state.fields.contains_key(&authority)
        {
            return authority;
        }
        if state.fields.contains_key(&field)
            || state.default_fields.contains(&field)
            || state.derived.contains_key(&field)
        {
            return field;
        }
        self.metadata
            .deferred_authority_aliases
            .get(&(row.list, field))
            .copied()
            .unwrap_or(field)
    }

    fn row_field_availability(&self, row: RowId, field: FieldId) -> RowFieldAvailability {
        let field = self.resolve_row_field_alias(row, field);
        let Some(state) = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
        else {
            return RowFieldAvailability::Missing;
        };
        if state.fields.contains_key(&field) {
            RowFieldAvailability::Stored
        } else if state.default_fields.contains(&field) {
            RowFieldAvailability::DefaultExpression
        } else if self.metadata.row_computations.contains_key(&field) {
            RowFieldAvailability::RowComputation
        } else {
            RowFieldAvailability::Missing
        }
    }

    fn row_value(&self, row: RowId, field: FieldId) -> Result<Value, Error> {
        let field = self.resolve_row_field_alias(row, field);
        let value = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.fields.get(&field))
            .cloned();
        value.ok_or_else(|| {
            let available = self
                .lists
                .get(&row.list)
                .and_then(|list| list.rows.get(&row))
                .map(|row| row.fields.keys().map(|field| field.0).collect::<Vec<_>>())
                .unwrap_or_default();
            Error::Evaluation(format!(
                "row {}:{}:{} has no field {}; available fields are {:?}",
                row.list.0, row.key, row.generation, field.0, available
            ))
        })
    }

    fn set_row_currentness(
        &mut self,
        row: RowId,
        field: FieldId,
        currentness: Currentness,
    ) -> Result<(), Error> {
        let derived = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .and_then(|row| row.derived.get_mut(&field))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "row {}:{}:{} has no derived field {}",
                    row.list.0, row.key, row.generation, field.0
                ))
            })?;
        *derived = currentness;
        if currentness == Currentness::Dirty {
            let _ = self.mark_ordered_index_row_dirty_for_field(row, field);
        }
        Ok(())
    }

    fn set_root_state(&mut self, state: StateId, value: Value, work: &mut Work) -> bool {
        if self.root_states.get(&state) == Some(&value) {
            return false;
        }
        self.root_states.insert(state, value.clone());
        work.dirty_states.insert(state);
        if work.emit {
            work.deltas.push(Delta::SetValue {
                target: ValueTarget::State(state),
                value,
            });
        }
        let dynamic_dependents = self
            .dynamic_dependencies
            .by_root_state
            .get(&state)
            .cloned()
            .unwrap_or_default();
        for dependent in dynamic_dependents {
            self.mark_consumer_dirty(dependent, work);
        }
        let dependents = self
            .metadata
            .dependencies
            .root_by_state
            .get(&state)
            .cloned()
            .unwrap_or_default();
        for field in dependents {
            self.mark_root_dirty(field, work);
        }
        let list_dependents = self
            .metadata
            .dependencies
            .list_by_state
            .get(&state)
            .cloned()
            .unwrap_or_default();
        for list in list_dependents {
            self.mark_list_dirty(list, work);
        }
        let row_dependents = self
            .metadata
            .dependencies
            .row_by_root_state
            .get(&state)
            .cloned()
            .unwrap_or_default();
        for (list, field) in row_dependents {
            for row in self.list_row_ids(list) {
                self.mark_row_dirty(row, field, work);
            }
        }
        true
    }

    fn set_row_field(
        &mut self,
        row: RowId,
        field: FieldId,
        value: Value,
        work: &mut Work,
    ) -> Result<bool, Error> {
        let old = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.fields.get(&field))
            .cloned();
        if old.as_ref() == Some(&value) {
            return Ok(false);
        }
        self.mark_list_semantic_change(row.list, work)?;
        self.lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot set field {} on missing row {}:{}:{}",
                    field.0, row.list.0, row.key, row.generation
                ))
            })?
            .fields
            .insert(field, value.clone());
        work.changed_rows.insert(row);
        if work.emit && !work.suppress_row_deltas.contains(&row) {
            work.deltas.push(Delta::SetValue {
                target: ValueTarget::RowField { row, field },
                value: value.clone(),
            });
        }
        self.invalidate_row_field(row, field, work);
        let fanout = self.mark_ordered_index_row_dirty_for_field(row, field);
        record_ordered_index_fanout(work, fanout);
        Ok(true)
    }

    fn set_row_authority_field(
        &mut self,
        row: RowId,
        field: FieldId,
        value: Value,
        work: &mut Work,
    ) -> Result<bool, Error> {
        let changed = self.set_row_field(row, field, value, work)?;
        if self.touched_row_fields.contains(&(row, field)) {
            self.suspend_row_default(row, field)?;
        }
        Ok(changed)
    }

    fn suspend_row_default(&mut self, row: RowId, field: FieldId) -> Result<(), Error> {
        let state = self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot suspend default field {} on missing row {}:{}:{}",
                    field.0, row.list.0, row.key, row.generation
                ))
            })?;
        if !state.default_fields.contains(&field) {
            return Ok(());
        }
        self.clear_consumer_dependencies(Consumer::Row(row, field));
        self.lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .expect("row existed while its default was suspended")
            .derived
            .insert(field, Currentness::Current);
        Ok(())
    }

    fn rebuild_ordered_indexes(&mut self, work: &mut Work) -> Result<(), Error> {
        let prepared = self.prepare_ordered_index_image(work)?;
        self.publish_ordered_index_image(prepared, work);
        Ok(())
    }

    fn prepare_ordered_index_image(
        &mut self,
        work: &mut Work,
    ) -> Result<PreparedOrderedIndexImage, Error> {
        let mut build = self.begin_ordered_index_image_build();
        while let Some(plan) = build.plans.get(build.next_plan).cloned() {
            let mut candidate = self.begin_ordered_index_candidate(plan, work)?;
            while let Some(row) = self.ordered_index_candidate_next_row(&candidate) {
                self.insert_ordered_index_candidate_row(&mut candidate, row, work)?;
                candidate.next_row += 1;
            }
            let (plan_id, index, report) = self.finish_ordered_index_candidate(candidate, work)?;
            self.extend_prepared_ordered_index_image(
                &mut build.prepared,
                plan_id,
                index,
                report,
                work,
            )?;
            build.next_plan += 1;
        }
        Ok(build.prepared)
    }

    fn begin_ordered_index_image_build(&self) -> OrderedIndexImageBuild {
        OrderedIndexImageBuild {
            plans: self
                .metadata
                .list_indexes
                .values()
                .cloned()
                .collect::<Vec<_>>(),
            next_plan: 0,
            current: None,
            prepared: PreparedOrderedIndexImage {
                indexes: BTreeMap::new(),
                logical_row_count: 0,
                entry_count: 0,
                expanded_key_count: 0,
                encoded_key_bytes: 0,
                structural_key_bytes: 0,
                payload_bytes: 0,
            },
        }
    }

    fn begin_ordered_index_candidate(
        &mut self,
        plan: PlanListIndex,
        work: &mut Work,
    ) -> Result<OrderedIndexCandidateBuild, Error> {
        work.metrics.ordered_index_full_rebuild_count = work
            .metrics
            .ordered_index_full_rebuild_count
            .saturating_add(1);
        let schema = ordered_index_schema(&plan)?;
        let limits = self.plan.target_profile.typed_list_index_limits();
        let index = OrderedIndex::new_with_limits(
            access_index_plan_id(plan.id),
            schema,
            AccessIndexResourceLimits::new(
                limits.max_entries_per_index,
                limits.max_encoded_key_bytes,
                limits.max_total_payload_bytes,
            ),
        );
        Ok(OrderedIndexCandidateBuild {
            plan,
            next_row: 0,
            index: Some(index),
            integrity: None,
        })
    }

    fn ordered_index_candidate_next_row(
        &self,
        candidate: &OrderedIndexCandidateBuild,
    ) -> Option<RowId> {
        self.lists
            .get(&candidate.plan.source_list)
            .and_then(|list| list.order.get(candidate.next_row))
            .copied()
    }

    fn insert_ordered_index_candidate_row(
        &mut self,
        candidate: &mut OrderedIndexCandidateBuild,
        row: RowId,
        work: &mut Work,
    ) -> Result<(), Error> {
        work.consume(1)?;
        let keys = self.evaluate_ordered_index_keys(&candidate.plan, row, work)?;
        work.metrics.ordered_index_key_evaluation_count = work
            .metrics
            .ordered_index_key_evaluation_count
            .saturating_add(1);
        let order_token = self.ordered_source_token(&candidate.plan, row)?;
        candidate
            .index
            .as_mut()
            .expect("candidate index exists while rows are inserted")
            .insert_many(access_row_id(row), source_order_token(order_token), keys)
            .map_err(|error| {
                if matches!(error, AccessError::ResourceLimitExceeded { .. }) {
                    work.metrics.ordered_index_resource_limit_failure_count = work
                        .metrics
                        .ordered_index_resource_limit_failure_count
                        .saturating_add(1);
                }
                Error::Evaluation(error.to_string())
            })
            .map(|_| ())
    }

    fn finish_ordered_index_candidate(
        &mut self,
        mut candidate: OrderedIndexCandidateBuild,
        work: &mut Work,
    ) -> Result<
        (
            PlanListIndexId,
            OrderedIndex,
            boon_list_access::IntegrityReport,
        ),
        Error,
    > {
        let index = candidate
            .index
            .take()
            .expect("candidate index exists before integrity validation");
        let mut integrity = index.into_integrity_task();
        loop {
            let poll = integrity.poll(usize::MAX).map_err(|error| {
                record_ordered_index_access_error(work, &error);
                Error::Evaluation(error.to_string())
            })?;
            let OrderedIndexIntegrityPoll::Ready(result) = poll else {
                continue;
            };
            let (index, report) = result.into_parts();
            record_ordered_index_integrity(
                work,
                report,
                candidate.plan.keys.iter().any(|key| {
                    matches!(
                        key.multiplicity,
                        PlanListIndexKeyMultiplicity::ListItems { .. }
                    )
                }),
            );
            return Ok((candidate.plan.id, index, report));
        }
    }

    fn extend_prepared_ordered_index_image(
        &self,
        prepared: &mut PreparedOrderedIndexImage,
        plan_id: PlanListIndexId,
        index: OrderedIndex,
        report: boon_list_access::IntegrityReport,
        work: &mut Work,
    ) -> Result<(), Error> {
        let limits = self.plan.target_profile.typed_list_index_limits();
        prepared.logical_row_count = prepared
            .logical_row_count
            .saturating_add(report.logical_rows);
        prepared.entry_count = prepared.entry_count.saturating_add(report.index_entries);
        if self
            .metadata
            .list_indexes
            .get(&plan_id)
            .is_some_and(|plan| {
                plan.keys.iter().any(|key| {
                    matches!(
                        key.multiplicity,
                        PlanListIndexKeyMultiplicity::ListItems { .. }
                    )
                })
            })
        {
            prepared.expanded_key_count = prepared
                .expanded_key_count
                .saturating_add(report.index_entries);
        }
        prepared.encoded_key_bytes = prepared
            .encoded_key_bytes
            .saturating_add(report.encoded_key_bytes);
        prepared.structural_key_bytes = prepared
            .structural_key_bytes
            .saturating_add(report.structural_key_bytes);
        prepared.payload_bytes = prepared
            .payload_bytes
            .saturating_add(report.payload_bytes());
        if prepared.entry_count > limits.max_startup_rebuild_entries {
            work.metrics.ordered_index_resource_limit_failure_count = work
                .metrics
                .ordered_index_resource_limit_failure_count
                .saturating_add(1);
            return Err(Error::Evaluation(format!(
                "typed index candidate image has {} entries; target profile `{}` permits {} startup rebuild entries",
                prepared.entry_count,
                self.plan.target_profile.as_str(),
                limits.max_startup_rebuild_entries
            )));
        }
        if prepared.payload_bytes > limits.max_total_payload_bytes {
            work.metrics.ordered_index_resource_limit_failure_count = work
                .metrics
                .ordered_index_resource_limit_failure_count
                .saturating_add(1);
            return Err(Error::Evaluation(format!(
                "typed index candidate image retains {} payload bytes; target profile `{}` permits {}",
                prepared.payload_bytes,
                self.plan.target_profile.as_str(),
                limits.max_total_payload_bytes
            )));
        }
        prepared.indexes.insert(plan_id, index);
        Ok(())
    }

    fn build_ordered_index_candidate(
        &mut self,
        plan: &PlanListIndex,
        work: &mut Work,
    ) -> Result<(OrderedIndex, boon_list_access::IntegrityReport), Error> {
        let mut candidate = self.begin_ordered_index_candidate(plan.clone(), work)?;
        while let Some(row) = self.ordered_index_candidate_next_row(&candidate) {
            self.insert_ordered_index_candidate_row(&mut candidate, row, work)?;
            candidate.next_row += 1;
        }
        let (_, index, report) = self.finish_ordered_index_candidate(candidate, work)?;
        Ok((index, report))
    }

    fn publish_ordered_index_image(
        &mut self,
        prepared: PreparedOrderedIndexImage,
        work: &mut Work,
    ) {
        self.ordered_indexes = prepared.indexes;
        self.dirty_ordered_indexes.clear();
        self.dirty_ordered_index_rows.clear();
        work.metrics.ordered_index_current_count =
            self.ordered_indexes.len().try_into().unwrap_or(u64::MAX);
        work.metrics.ordered_index_current_logical_row_count = prepared.logical_row_count;
        work.metrics.ordered_index_current_entry_count = prepared.entry_count;
        work.metrics.ordered_index_current_expanded_key_count = prepared.expanded_key_count;
        work.metrics.ordered_index_current_encoded_key_bytes = prepared.encoded_key_bytes;
        work.metrics.ordered_index_current_structural_key_bytes = prepared.structural_key_bytes;
        work.metrics.ordered_index_current_payload_bytes = prepared.payload_bytes;
    }

    fn flush_list_access_dependencies(&mut self, work: &mut Work) -> Result<(), Error> {
        if self.list_access_flush_in_progress
            || !self.evaluating_ordered_indexes.is_empty()
            || self.dynamic_dependencies.by_list_access.is_empty()
        {
            return Ok(());
        }
        self.list_access_flush_in_progress = true;
        let result = (|| {
            let max_passes = self.metadata.list_indexes.len().saturating_add(1);
            for _ in 0..max_passes {
                let subscribed_indexes = self
                    .dynamic_dependencies
                    .by_list_access
                    .keys()
                    .map(|(index, _)| *index)
                    .collect::<BTreeSet<_>>();
                let dirty_indexes = subscribed_indexes
                    .into_iter()
                    .filter(|index| {
                        self.dirty_ordered_indexes.contains(index)
                            || self
                                .dirty_ordered_index_rows
                                .get(index)
                                .is_some_and(|rows| !rows.is_empty())
                    })
                    .collect::<Vec<_>>();
                if dirty_indexes.is_empty() {
                    return Ok(());
                }

                for index_id in dirty_indexes {
                    self.ensure_ordered_index_current(index_id, None, work)?;
                }
            }
            Err(Error::Evaluation(
                "typed list access dependencies did not converge".to_owned(),
            ))
        })();
        self.list_access_flush_in_progress = false;
        result
    }

    fn ensure_ordered_index_current(
        &mut self,
        index_id: PlanListIndexId,
        requesting_consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let metadata = Arc::clone(&self.metadata);
        let source_list = metadata
            .list_indexes
            .get(&index_id)
            .map(|index| index.source_list)
            .ok_or_else(|| {
                Error::InvalidPlan(format!("missing typed list index {}", index_id.0))
            })?;
        if metadata.list_computations.contains_key(&source_list) {
            self.ensure_list_current(source_list, None, work)?;
        }
        let fully_dirty = self.dirty_ordered_indexes.contains(&index_id);
        let pending_rows = self
            .dirty_ordered_index_rows
            .get(&index_id)
            .is_some_and(|rows| !rows.is_empty());
        if self.ordered_indexes.contains_key(&index_id) && !fully_dirty && !pending_rows {
            self.record_ordered_index_footprint(work);
            return Ok(());
        }
        let dirty_rows_snapshot = self
            .dirty_ordered_index_rows
            .get(&index_id)
            .cloned()
            .unwrap_or_default();
        let (subscriptions, old_cursors) =
            self.ordered_index_subscriber_snapshot(index_id, &dirty_rows_snapshot);
        let plan = metadata.list_indexes.get(&index_id).ok_or_else(|| {
            Error::InvalidPlan(format!("missing typed list index {}", index_id.0))
        })?;
        if fully_dirty || !self.ordered_indexes.contains_key(&index_id) {
            let (index, report) = self.build_ordered_index_candidate(plan, work)?;
            let other_payload = self.ordered_index_payload_bytes_excluding(index_id);
            self.check_ordered_index_total_payload(
                other_payload.saturating_add(report.payload_bytes()),
                work,
            )?;
            self.ordered_indexes.insert(index_id, index);
            self.dirty_ordered_indexes.remove(&index_id);
            self.dirty_ordered_index_rows.remove(&index_id);
            self.publish_ordered_index_subscriber_changes(
                index_id,
                true,
                &dirty_rows_snapshot,
                &subscriptions,
                &old_cursors,
                requesting_consumer,
                work,
            )?;
            self.record_ordered_index_footprint(work);
            return Ok(());
        }

        let dirty_rows = self
            .dirty_ordered_index_rows
            .remove(&index_id)
            .unwrap_or_default();
        if dirty_rows.is_empty() {
            self.record_ordered_index_footprint(work);
            return Ok(());
        }
        work.consume(dirty_rows.len().try_into().unwrap_or(u64::MAX))?;
        let mut index = self.ordered_indexes.remove(&index_id).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "typed list index {} disappeared during incremental maintenance",
                index_id.0
            ))
        })?;
        let remaining_payload = self.ordered_index_payload_bytes_excluding(index_id);
        let payload_limit = self
            .plan
            .target_profile
            .typed_list_index_limits()
            .max_total_payload_bytes
            .saturating_sub(remaining_payload);
        if let Err(error) = index.set_max_payload_bytes(payload_limit) {
            self.ordered_indexes.insert(index_id, index);
            work.metrics.ordered_index_resource_limit_failure_count = work
                .metrics
                .ordered_index_resource_limit_failure_count
                .saturating_add(1);
            return Err(Error::Evaluation(error.to_string()));
        }
        let update_result = (|| {
            for row in dirty_rows.iter().copied() {
                work.metrics.ordered_index_incremental_row_count = work
                    .metrics
                    .ordered_index_incremental_row_count
                    .saturating_add(1);
                let access_row = access_row_id(row);
                let outcome = if self.row_exists(row) {
                    let keys = self.evaluate_ordered_index_keys(plan, row, work)?;
                    work.metrics.ordered_index_key_evaluation_count = work
                        .metrics
                        .ordered_index_key_evaluation_count
                        .saturating_add(1);
                    let order_token = source_order_token(self.ordered_source_token(plan, row)?);
                    if index.contains(access_row) {
                        index.update_many(access_row, order_token, keys)
                    } else {
                        index.insert_many(access_row, order_token, keys)
                    }
                } else {
                    index.remove(access_row)
                }
                .map_err(|error| {
                    if matches!(error, AccessError::ResourceLimitExceeded { .. }) {
                        work.metrics.ordered_index_resource_limit_failure_count = work
                            .metrics
                            .ordered_index_resource_limit_failure_count
                            .saturating_add(1);
                    }
                    Error::Evaluation(error.to_string())
                })?;
                match outcome {
                    MutationOutcome::Inserted => {
                        work.metrics.ordered_index_insert_count =
                            work.metrics.ordered_index_insert_count.saturating_add(1);
                    }
                    MutationOutcome::Updated => {
                        work.metrics.ordered_index_update_count =
                            work.metrics.ordered_index_update_count.saturating_add(1);
                    }
                    MutationOutcome::Removed => {
                        work.metrics.ordered_index_remove_count =
                            work.metrics.ordered_index_remove_count.saturating_add(1);
                    }
                    MutationOutcome::Unchanged | MutationOutcome::NotFound => {}
                }
            }
            Ok(())
        })();
        self.ordered_indexes.insert(index_id, index);
        if let Err(error) = update_result {
            self.dirty_ordered_index_rows
                .entry(index_id)
                .or_default()
                .extend(dirty_rows);
            return Err(error);
        }
        if let Some(rows) = self.dirty_ordered_index_rows.get_mut(&index_id) {
            for row in &dirty_rows {
                rows.remove(row);
            }
            if rows.is_empty() {
                self.dirty_ordered_index_rows.remove(&index_id);
            }
        }
        self.publish_ordered_index_subscriber_changes(
            index_id,
            false,
            &dirty_rows,
            &subscriptions,
            &old_cursors,
            requesting_consumer,
            work,
        )?;
        self.record_ordered_index_footprint(work);
        Ok(())
    }

    fn ordered_index_subscriber_snapshot(
        &self,
        index_id: PlanListIndexId,
        dirty_rows: &BTreeSet<RowId>,
    ) -> (ListAccessSubscriptions, OrderedIndexCursorSnapshot) {
        let subscriptions = self
            .dynamic_dependencies
            .by_list_access
            .iter()
            .filter_map(|((index, selection), consumers)| {
                (*index == index_id).then_some((selection.clone(), consumers.clone()))
            })
            .collect::<Vec<_>>();
        let old_cursors = self
            .ordered_indexes
            .get(&index_id)
            .map(|index| {
                dirty_rows
                    .iter()
                    .filter_map(|row| {
                        let cursors = index.cursor_keys_for(access_row_id(*row));
                        (!cursors.is_empty()).then_some((*row, cursors))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        (subscriptions, old_cursors)
    }

    fn publish_ordered_index_subscriber_changes(
        &mut self,
        index_id: PlanListIndexId,
        fully_dirty: bool,
        dirty_rows: &BTreeSet<RowId>,
        subscriptions: &ListAccessSubscriptions,
        old_cursors: &OrderedIndexCursorSnapshot,
        requesting_consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<(), Error> {
        if subscriptions.is_empty() {
            return Ok(());
        }
        let consumers = {
            let index = self.ordered_indexes.get(&index_id).ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list index {} disappeared while publishing access changes",
                    index_id.0
                ))
            })?;
            let mut consumers = BTreeSet::new();
            if fully_dirty {
                for (_, subscribed) in subscriptions {
                    consumers.extend(
                        subscribed
                            .iter()
                            .copied()
                            .filter(|consumer| Some(*consumer) != requesting_consumer),
                    );
                }
            } else {
                for row in dirty_rows {
                    let old = old_cursors.get(row).cloned().unwrap_or_default();
                    let new = index.cursor_keys_for(access_row_id(*row));
                    if old == new {
                        continue;
                    }
                    for (selection, subscribed) in subscriptions {
                        let matches =
                            old.iter().chain(&new).try_fold(false, |matches, cursor| {
                                evaluated_list_access_selection_matches_key(
                                    index,
                                    selection,
                                    cursor.key(),
                                )
                                .map(|matched| matches || matched)
                                .map_err(|error| Error::Evaluation(error.to_string()))
                            })?;
                        if matches {
                            consumers.extend(
                                subscribed
                                    .iter()
                                    .copied()
                                    .filter(|consumer| Some(*consumer) != requesting_consumer),
                            );
                        }
                    }
                }
            }
            consumers
        };
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
        Ok(())
    }

    fn ordered_index_payload_bytes_excluding(&self, excluded: PlanListIndexId) -> u64 {
        self.ordered_indexes
            .iter()
            .filter(|(index, _)| **index != excluded)
            .fold(0_u64, |total, (_, index)| {
                total.saturating_add(index.payload_bytes())
            })
    }

    fn check_ordered_index_total_payload(
        &self,
        payload_bytes: u64,
        work: &mut Work,
    ) -> Result<(), Error> {
        let limits = self.plan.target_profile.typed_list_index_limits();
        if payload_bytes > limits.max_total_payload_bytes {
            work.metrics.ordered_index_resource_limit_failure_count = work
                .metrics
                .ordered_index_resource_limit_failure_count
                .saturating_add(1);
            return Err(Error::Evaluation(format!(
                "typed indexes retain {payload_bytes} payload bytes; target profile `{}` permits {}",
                self.plan.target_profile.as_str(),
                limits.max_total_payload_bytes
            )));
        }
        Ok(())
    }

    fn record_ordered_index_footprint(&self, work: &mut Work) {
        work.metrics.ordered_index_current_count =
            self.ordered_indexes.len().try_into().unwrap_or(u64::MAX);
        work.metrics.ordered_index_current_logical_row_count =
            self.ordered_indexes.values().fold(0_u64, |total, index| {
                total.saturating_add(index.metrics().logical_rows)
            });
        work.metrics.ordered_index_current_entry_count =
            self.ordered_indexes.values().fold(0_u64, |total, index| {
                total.saturating_add(index.metrics().index_entries)
            });
        work.metrics.ordered_index_current_expanded_key_count = self
            .ordered_indexes
            .iter()
            .filter(|(index_id, _)| {
                self.metadata
                    .list_indexes
                    .get(index_id)
                    .is_some_and(|plan| {
                        plan.keys.iter().any(|key| {
                            matches!(
                                key.multiplicity,
                                PlanListIndexKeyMultiplicity::ListItems { .. }
                            )
                        })
                    })
            })
            .fold(0_u64, |total, (_, index)| {
                total.saturating_add(index.metrics().index_entries)
            });
        work.metrics.ordered_index_current_encoded_key_bytes =
            self.ordered_indexes.values().fold(0_u64, |total, index| {
                total.saturating_add(index.encoded_key_bytes())
            });
        work.metrics.ordered_index_current_structural_key_bytes =
            self.ordered_indexes.values().fold(0_u64, |total, index| {
                total.saturating_add(index.structural_key_bytes())
            });
        work.metrics.ordered_index_current_payload_bytes =
            self.ordered_indexes.values().fold(0_u64, |total, index| {
                total.saturating_add(index.payload_bytes())
            });
    }

    fn ensure_all_ordered_indexes_current(&mut self, work: &mut Work) -> Result<(), Error> {
        let indexes = self
            .metadata
            .list_indexes
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for index in indexes {
            self.ensure_ordered_index_current(index, None, work)?;
        }
        Ok(())
    }

    fn ordered_source_token(&self, plan: &PlanListIndex, row: RowId) -> Result<u128, Error> {
        self.lists
            .get(&plan.source_list)
            .and_then(|list| list.order_token(row))
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list index {} row {}:{}:{} has no stable source order token",
                    plan.id.0, row.list.0, row.key, row.generation
                ))
            })
    }

    fn mark_ordered_index_row_dirty_for_field(&mut self, row: RowId, field: FieldId) -> usize {
        let indexes = self
            .metadata
            .ordered_indexes_by_row_field
            .get(&(row.list, field))
            .cloned()
            .unwrap_or_default();
        let fanout = indexes.len();
        for index in indexes {
            if !self.dirty_ordered_indexes.contains(&index) {
                self.dirty_ordered_index_rows
                    .entry(index)
                    .or_default()
                    .insert(row);
            }
        }
        fanout
    }

    fn prepare_ordered_index_rows_dirty_for_list(
        &self,
        list: ListId,
        rows: impl IntoIterator<Item = RowId>,
    ) -> PreparedOrderedIndexDirty {
        let indexes = self
            .metadata
            .ordered_indexes_by_list
            .get(&list)
            .cloned()
            .unwrap_or_default();
        let rows = rows.into_iter().collect::<BTreeSet<_>>();
        let mut prepared = PreparedOrderedIndexDirty::default();
        for index in indexes {
            if self.dirty_ordered_indexes.contains(&index) {
                continue;
            }
            let entry = prepared.rows.entry(index).or_default();
            for row in &rows {
                if !self
                    .dirty_ordered_index_rows
                    .get(&index)
                    .is_some_and(|dirty| dirty.contains(row))
                    && entry.insert(*row)
                {
                    prepared.fanout = prepared.fanout.saturating_add(1);
                }
            }
        }
        prepared
    }

    fn commit_ordered_index_dirty(&mut self, prepared: PreparedOrderedIndexDirty) {
        for (index, rows) in prepared.rows {
            self.dirty_ordered_index_rows
                .entry(index)
                .or_default()
                .extend(rows);
        }
    }

    fn evaluate_ordered_index_keys(
        &mut self,
        plan: &PlanListIndex,
        row: RowId,
        work: &mut Work,
    ) -> Result<Vec<StructuralKey>, Error> {
        if !self.evaluating_ordered_indexes.insert(plan.id) {
            return Err(Error::Evaluation(format!(
                "typed ordered index {} forms a recursive key dependency",
                plan.id.0
            )));
        }
        let result = (|| {
            let mut bindings = BTreeMap::new();
            let mut components = Vec::with_capacity(plan.keys.len());
            for (key_position, key) in plan.keys.iter().enumerate() {
                let value = self.eval_contextual_body(
                    (key.owner, key.row_local),
                    EvalValue::Row(row),
                    &key.expression,
                    Some(row),
                    None,
                    None,
                    None,
                    &mut bindings,
                    work,
                )?;
                let value = self.materialize_eval(value)?;
                let values = match key.multiplicity {
                    PlanListIndexKeyMultiplicity::One => {
                        vec![structural_index_value(key, value).map_err(|error| {
                            ordered_index_key_error(plan, row, key_position, key, error)
                        })?]
                    }
                    PlanListIndexKeyMultiplicity::ListItems { max_items } => {
                        let Value::List(values) = value else {
                            return Err(ordered_index_key_error(
                                plan,
                                row,
                                key_position,
                                key,
                                Error::Evaluation(
                                    "expanded typed index key did not evaluate as LIST".to_owned(),
                                ),
                            ));
                        };
                        if values.len() > usize::from(max_items) {
                            return Err(ordered_index_key_error(
                                plan,
                                row,
                                key_position,
                                key,
                                Error::Evaluation(format!(
                                    "expanded typed index key produced {} items; maximum is {max_items}",
                                    values.len()
                                )),
                            ));
                        }
                        values
                            .into_iter()
                            .map(|value| structural_index_value(key, value))
                            .collect::<Result<BTreeSet<_>, _>>()
                            .map_err(|error| {
                                ordered_index_key_error(plan, row, key_position, key, error)
                            })?
                            .into_iter()
                            .collect()
                    }
                };
                components.push(values);
            }
            let mut products = vec![Vec::with_capacity(plan.keys.len())];
            for values in components {
                if values.is_empty() {
                    return Ok(Vec::new());
                }
                let mut next = Vec::with_capacity(products.len().saturating_mul(values.len()));
                for prefix in products {
                    for value in &values {
                        let mut parts = prefix.clone();
                        parts.push(value.clone());
                        next.push(parts);
                    }
                }
                products = next;
            }
            products
                .into_iter()
                .map(|parts| {
                    StructuralKey::new(parts).map_err(|error| Error::Evaluation(error.to_string()))
                })
                .collect()
        })();
        self.evaluating_ordered_indexes.remove(&plan.id);
        result
    }

    fn list_row_ids(&self, list: ListId) -> Vec<RowId> {
        self.lists
            .get(&list)
            .map(|list| list.order.to_vec())
            .unwrap_or_default()
    }

    fn row_exists(&self, row: RowId) -> bool {
        self.lists
            .get(&row.list)
            .is_some_and(|list| list.rows.contains_key(&row))
    }

    fn mark_root_dirty(&mut self, field: FieldId, work: &mut Work) {
        if self
            .root_fields
            .get(&field)
            .is_some_and(|cell| cell.currentness == Currentness::Evaluating)
        {
            return;
        }
        let became_dirty = self.root_fields.get_mut(&field).map(|cell| {
            let became_dirty = cell.currentness == Currentness::Current;
            if became_dirty {
                cell.currentness = Currentness::Dirty;
            }
            became_dirty
        });
        let consumer = Consumer::Root(field);
        let first_in_turn = work.dirty_consumers.insert(consumer);
        if became_dirty.is_none() || (!became_dirty.unwrap_or_default() && !first_in_turn) {
            return;
        }
        self.clear_consumer_dependencies(consumer);
        self.invalidate_root_field(field, work);
    }

    fn invalidate_root_field(&mut self, field: FieldId, work: &mut Work) {
        let dynamic_dependents = self
            .dynamic_dependencies
            .by_root_field
            .get(&field)
            .cloned()
            .unwrap_or_default();
        for dependent in dynamic_dependents {
            self.mark_consumer_dirty(dependent, work);
        }
        let root_dependents = self
            .metadata
            .dependencies
            .root_by_field
            .get(&field)
            .cloned()
            .unwrap_or_default();
        for dependent in root_dependents {
            if dependent != field {
                self.mark_root_dirty(dependent, work);
            }
        }
        let list_dependents = self
            .metadata
            .dependencies
            .list_by_field
            .get(&field)
            .cloned()
            .unwrap_or_default();
        for list in list_dependents {
            self.mark_list_dirty(list, work);
        }
        let row_dependents = self
            .metadata
            .dependencies
            .row_by_root_field
            .get(&field)
            .cloned()
            .unwrap_or_default();
        for (list, dependent) in row_dependents {
            for row in self.list_row_ids(list) {
                self.mark_row_dirty(row, dependent, work);
            }
        }
    }

    fn mark_row_dirty(&mut self, row: RowId, field: FieldId, work: &mut Work) {
        if self
            .lists
            .get(&row.list)
            .and_then(|list| list.rows.get(&row))
            .and_then(|row| row.derived.get(&field))
            .is_some_and(|currentness| *currentness == Currentness::Evaluating)
        {
            return;
        }
        let became_dirty = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .and_then(|row| row.derived.get_mut(&field))
            .map(|currentness| {
                let became_dirty = *currentness == Currentness::Current;
                if became_dirty {
                    *currentness = Currentness::Dirty;
                }
                became_dirty
            });
        let consumer = Consumer::Row(row, field);
        let first_in_turn = work.dirty_consumers.insert(consumer);
        if became_dirty == Some(true) {
            let fanout = self.mark_ordered_index_row_dirty_for_field(row, field);
            record_ordered_index_fanout(work, fanout);
        }
        if became_dirty.is_none() || (!became_dirty.unwrap_or_default() && !first_in_turn) {
            return;
        }
        let dynamic_dependents = self
            .dynamic_dependencies
            .by_row_field
            .get(&(row, field))
            .cloned()
            .unwrap_or_default();
        self.clear_consumer_dependencies(consumer);
        let dependents = self
            .metadata
            .dependencies
            .row_by_field
            .get(&(row.list, field))
            .cloned()
            .unwrap_or_default();
        for dependent in dependents {
            if dependent != field {
                self.mark_row_dirty(row, dependent, work);
            }
        }
        for dependent in dynamic_dependents {
            if dependent != consumer {
                self.mark_consumer_dirty(dependent, work);
            }
        }
    }

    fn mark_list_dirty(&mut self, list: ListId, work: &mut Work) {
        if self
            .derived_lists
            .get(&list)
            .is_some_and(|cell| cell.currentness == Currentness::Evaluating)
        {
            return;
        }
        let became_dirty = self.derived_lists.get_mut(&list).map(|cell| {
            let became_dirty = cell.currentness == Currentness::Current;
            if became_dirty {
                cell.currentness = Currentness::Dirty;
            }
            if let Some(window) = cell.window.as_mut() {
                window.values_current = false;
            }
            became_dirty
        });
        let consumer = Consumer::List(list);
        let first_in_turn = work.dirty_consumers.insert(consumer);
        if became_dirty.is_none() || (!became_dirty.unwrap_or_default() && !first_in_turn) {
            return;
        }
        self.clear_consumer_dependencies(consumer);
        self.invalidate_list_structure(list, work);
    }

    fn mark_consumer_dirty(&mut self, consumer: Consumer, work: &mut Work) {
        work.metrics.dependency_fanout_count += 1;
        match consumer {
            Consumer::Root(field) => self.mark_root_dirty(field, work),
            Consumer::List(list) => self.mark_list_dirty(list, work),
            Consumer::Row(row, field) => self.mark_row_dirty(row, field, work),
            Consumer::ProducerResult(call_site_id) => {
                let first_in_turn = work.dirty_consumers.insert(consumer);
                if !first_in_turn {
                    return;
                }
                self.clear_consumer_dependencies(consumer);
                if let Some(active) = self
                    .active_producer_lease
                    .as_mut()
                    .filter(|active| active.call_site_id == call_site_id)
                {
                    active.producer_result = None;
                }
            }
            Consumer::Effect(effect) => {
                let first_in_turn = work.dirty_consumers.insert(consumer);
                if !first_in_turn && work.pending_effect_reconciliations.contains(&effect) {
                    return;
                }
                self.clear_consumer_dependencies(consumer);
                if self.effect_activations.contains_key(&effect) {
                    work.pending_effect_reconciliations.insert(effect);
                }
            }
        }
    }

    fn invalidate_row_field(&mut self, row: RowId, field: FieldId, work: &mut Work) {
        let mut consumers = self
            .dynamic_dependencies
            .by_row_field
            .get(&(row, field))
            .cloned()
            .unwrap_or_default();
        let static_dependents = self
            .metadata
            .dependencies
            .row_by_field
            .get(&(row.list, field))
            .cloned()
            .unwrap_or_default();
        for dependent in static_dependents {
            consumers.insert(Consumer::Row(row, dependent));
        }
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
    }

    fn invalidate_list_structure(&mut self, list: ListId, work: &mut Work) {
        let mut consumers = self
            .dynamic_dependencies
            .by_list
            .get(&list)
            .cloned()
            .unwrap_or_default();
        for field in self
            .metadata
            .dependencies
            .root_by_list
            .get(&list)
            .cloned()
            .unwrap_or_default()
        {
            consumers.insert(Consumer::Root(field));
        }
        for dependent in self
            .metadata
            .dependencies
            .list_by_list
            .get(&list)
            .cloned()
            .unwrap_or_default()
        {
            consumers.insert(Consumer::List(dependent));
        }
        for (owner, field) in self
            .metadata
            .dependencies
            .row_by_list
            .get(&list)
            .cloned()
            .unwrap_or_default()
        {
            for row in self.list_row_ids(owner) {
                consumers.insert(Consumer::Row(row, field));
            }
        }
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
    }

    fn invalidate_distributed_import(&mut self, import_id: ImportId, work: &mut Work) {
        let consumers = self
            .dynamic_dependencies
            .by_distributed_import
            .get(&import_id)
            .cloned()
            .unwrap_or_default();
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
    }

    fn invalidate_distributed_call_result(
        &mut self,
        import_id: ImportId,
        call_instance_id: DistributedCallInstanceId,
        work: &mut Work,
    ) {
        let consumers = self
            .dynamic_dependencies
            .by_distributed_call_result
            .get(&(import_id, call_instance_id))
            .cloned()
            .unwrap_or_default();
        for consumer in consumers {
            self.mark_consumer_dirty(consumer, work);
        }
    }

    fn register_row_dependency(
        &mut self,
        consumer: Option<Consumer>,
        row: RowId,
        field: FieldId,
    ) -> bool {
        let field = self.resolve_row_field_alias(row, field);
        if let Some(consumer) = consumer {
            let dependency = Consumer::Row(row, field);
            let creates_cycle =
                dependency == consumer || self.dynamic_dependency_reaches(dependency, consumer);
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::RowField(row, field));
            creates_cycle
        } else {
            false
        }
    }

    fn register_root_state_dependency(&mut self, consumer: Option<Consumer>, state: StateId) {
        if let Some(consumer) = consumer {
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::RootState(state));
        }
    }

    fn register_root_field_dependency(&mut self, consumer: Option<Consumer>, field: FieldId) {
        if let Some(consumer) = consumer
            && consumer != Consumer::Root(field)
        {
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::RootField(field));
        }
    }

    fn dynamic_dependency_reaches(&self, start: Consumer, target: Consumer) -> bool {
        let mut pending = vec![start];
        let mut visited = BTreeSet::new();
        while let Some(consumer) = pending.pop() {
            if consumer == target {
                return true;
            }
            if !visited.insert(consumer) {
                continue;
            }
            pending.extend(
                self.dynamic_dependencies
                    .by_consumer
                    .get(&consumer)
                    .into_iter()
                    .flatten()
                    .filter_map(|dependency| match dependency {
                        DynamicDependency::RootField(field)
                            if self.metadata.root_computations.contains_key(field) =>
                        {
                            Some(Consumer::Root(*field))
                        }
                        DynamicDependency::RowField(row, field) => {
                            Some(Consumer::Row(*row, *field))
                        }
                        DynamicDependency::List(list)
                            if self.metadata.list_computations.contains_key(list) =>
                        {
                            Some(Consumer::List(*list))
                        }
                        DynamicDependency::RootState(_)
                        | DynamicDependency::RootField(_)
                        | DynamicDependency::ListAccess(_, _)
                        | DynamicDependency::List(_)
                        | DynamicDependency::DistributedImport(_)
                        | DynamicDependency::DistributedCallResult(_, _) => None,
                    }),
            );
        }
        false
    }

    fn register_list_dependency(&mut self, consumer: Option<Consumer>, list: ListId) {
        if let Some(consumer) = consumer {
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::List(list));
        }
    }

    fn register_list_access_dependency(
        &mut self,
        consumer: Option<Consumer>,
        index: PlanListIndexId,
        selection: &EvaluatedListAccessSelection,
    ) {
        if let Some(consumer) = consumer {
            self.dynamic_dependencies.insert(
                consumer,
                DynamicDependency::ListAccess(index, selection.clone()),
            );
        }
    }

    fn register_distributed_import_dependency(
        &mut self,
        consumer: Option<Consumer>,
        import_id: ImportId,
    ) {
        if let Some(consumer) = consumer {
            self.dynamic_dependencies
                .insert(consumer, DynamicDependency::DistributedImport(import_id));
        }
    }

    fn clear_consumer_dependencies(&mut self, consumer: Consumer) {
        self.dynamic_dependencies.clear(consumer);
        self.distributed_current_call_demands.remove(&consumer);
    }

    fn register_distributed_current_call_demand(
        &mut self,
        consumer: Consumer,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        arguments: DistributedCurrentCallArguments,
    ) -> Result<(), Error> {
        let key = (call_site_id, call_instance_id);
        let demands = self
            .distributed_current_call_demands
            .entry(consumer)
            .or_default();
        if let Some(existing) = demands.get(&key) {
            if existing != &arguments {
                return Err(Error::InvalidPlan(
                    "remote call instance has conflicting arguments in one retained consumer"
                        .to_owned(),
                ));
            }
        } else {
            demands.insert(key, arguments);
        }
        Ok(())
    }

    fn distributed_call_instance_id(
        &self,
        call_site_id: RemoteCallSiteId,
        rows: &[DistributedCallInstanceRow],
    ) -> Result<DistributedCallInstanceId, Error> {
        let parent = self
            .active_producer_lease
            .as_ref()
            .map(|active| active.key.call_instance_id);
        DistributedCallInstanceId::from_context(call_site_id, parent, rows)
            .map_err(|error| Error::InvalidPlan(error.to_string()))
    }

    fn register_distributed_call_result_dependency(
        &mut self,
        consumer: Consumer,
        import_id: ImportId,
        call_instance_id: DistributedCallInstanceId,
    ) {
        self.dynamic_dependencies.insert(
            consumer,
            DynamicDependency::DistributedCallResult(import_id, call_instance_id),
        );
    }

    fn materialize_eval(&mut self, value: EvalValue) -> Result<Value, Error> {
        let mut tasks = vec![EvalMaterializationTask::Evaluate(value)];
        let mut values = Vec::<Value>::new();
        while let Some(task) = tasks.pop() {
            match task {
                EvalMaterializationTask::Evaluate(value) => match value {
                    EvalValue::Value(value) => values.push(value),
                    EvalValue::Row(row) => values.push(row_identity_value(row)),
                    EvalValue::List(items) => {
                        let capacity = items.len();
                        ensure_value_continuation_capacity(
                            &tasks,
                            1,
                            "evaluation materialization",
                        )?;
                        tasks.push(EvalMaterializationTask::Continue(
                            EvalMaterializationCollection::List {
                                remaining: items.into_iter(),
                                output: Vec::with_capacity(capacity),
                            },
                        ));
                    }
                    EvalValue::Record(fields) => {
                        ensure_value_continuation_capacity(
                            &tasks,
                            1,
                            "evaluation materialization",
                        )?;
                        tasks.push(EvalMaterializationTask::Continue(
                            EvalMaterializationCollection::Record {
                                remaining: fields.into_iter(),
                                output: BTreeMap::new(),
                                mapped_row: None,
                            },
                        ));
                    }
                    EvalValue::MappedRow { id, fields, .. } => {
                        ensure_value_continuation_capacity(
                            &tasks,
                            1,
                            "evaluation materialization",
                        )?;
                        tasks.push(EvalMaterializationTask::Continue(
                            EvalMaterializationCollection::Record {
                                remaining: fields.into_iter(),
                                output: BTreeMap::new(),
                                mapped_row: Some(id),
                            },
                        ));
                    }
                    EvalValue::OrderedList { items, .. } => {
                        let capacity = items.len();
                        ensure_value_continuation_capacity(
                            &tasks,
                            1,
                            "evaluation materialization",
                        )?;
                        tasks.push(EvalMaterializationTask::Continue(
                            EvalMaterializationCollection::OrderedList {
                                remaining: items.into_iter(),
                                output: Vec::with_capacity(capacity),
                            },
                        ));
                    }
                },
                EvalMaterializationTask::Continue(mut collection) => {
                    let next = match &mut collection {
                        EvalMaterializationCollection::List { remaining, .. } => remaining
                            .next()
                            .map(|value| (EvalMaterializationSlot::Item, value)),
                        EvalMaterializationCollection::Record { remaining, .. } => remaining
                            .next()
                            .map(|(name, value)| (EvalMaterializationSlot::Field(name), value)),
                        EvalMaterializationCollection::OrderedList { remaining, .. } => remaining
                            .next()
                            .map(|item| (EvalMaterializationSlot::Item, item.value)),
                    };
                    if let Some((slot, value)) = next {
                        ensure_value_continuation_capacity(
                            &tasks,
                            2,
                            "evaluation materialization",
                        )?;
                        tasks.push(EvalMaterializationTask::Append { collection, slot });
                        tasks.push(EvalMaterializationTask::Evaluate(value));
                    } else {
                        let value = match collection {
                            EvalMaterializationCollection::List { output, .. }
                            | EvalMaterializationCollection::OrderedList { output, .. } => {
                                Value::List(output)
                            }
                            EvalMaterializationCollection::Record {
                                output,
                                mapped_row: Some(id),
                                ..
                            } => Value::MappedRow { id, fields: output },
                            EvalMaterializationCollection::Record {
                                output,
                                mapped_row: None,
                                ..
                            } => Value::Record(output),
                        };
                        values.push(value);
                    }
                }
                EvalMaterializationTask::Append {
                    mut collection,
                    slot,
                } => {
                    let value = values.pop().ok_or_else(|| {
                        Error::InvalidPlan(
                            "evaluation materialization produced no child value".to_owned(),
                        )
                    })?;
                    match (&mut collection, slot) {
                        (
                            EvalMaterializationCollection::List { output, .. }
                            | EvalMaterializationCollection::OrderedList { output, .. },
                            EvalMaterializationSlot::Item,
                        ) => output.push(value),
                        (
                            EvalMaterializationCollection::Record { output, .. },
                            EvalMaterializationSlot::Field(name),
                        ) => {
                            output.insert(name, value);
                        }
                        _ => {
                            return Err(Error::InvalidPlan(
                                "evaluation materialization continuation type mismatch".to_owned(),
                            ));
                        }
                    }
                    ensure_value_continuation_capacity(&tasks, 1, "evaluation materialization")?;
                    tasks.push(EvalMaterializationTask::Continue(collection));
                }
            }
        }
        if values.len() != 1 {
            return Err(Error::InvalidPlan(format!(
                "evaluation materialization completed with {} values",
                values.len()
            )));
        }
        values.pop().ok_or_else(|| {
            Error::InvalidPlan("evaluation materialization produced no root value".to_owned())
        })
    }
}

impl MachineInstance {
    fn reconcile_materialized_list(
        &mut self,
        list_id: ListId,
        authority_source_list: Option<ListId>,
        field_ids: &BTreeMap<String, FieldId>,
        row_field_copies: &[PlanMaterializedRowFieldCopy],
        value: EvalValue,
        owner_prefix: &[OwnerInstanceRow],
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let items = eval_to_list(value)?;
        let desired_len = items.len();
        let current_len = self.list_row_ids(list_id).len();
        let existing = self.list_row_ids_for_owner(list_id, owner_prefix)?;
        work.consume(existing.len().try_into().unwrap_or(u64::MAX))?;
        let projected_len = current_len
            .checked_sub(existing.len())
            .and_then(|len| len.checked_add(desired_len))
            .ok_or_else(|| Error::Evaluation("materialized list size overflow".to_owned()))?;
        let capacity = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list_id)
            .ok_or_else(|| Error::InvalidPlan(format!("missing list slot {}", list_id.0)))?
            .capacity;
        if capacity.is_some_and(|capacity| projected_len > capacity) {
            return Err(Error::Evaluation(format!(
                "materialized list {} would contain {} rows, exceeding capacity {}",
                list_id.0,
                projected_len,
                capacity.unwrap_or_default()
            )));
        }
        work.consume(desired_len.try_into().unwrap_or(u64::MAX))?;
        if authority_source_list.is_none()
            && items.iter().all(|item| {
                matches!(
                    item,
                    EvalValue::Record(_) | EvalValue::Value(Value::Record(_))
                )
            })
        {
            return self.reconcile_unkeyed_value_list_records(
                list_id,
                field_ids,
                items,
                owner_prefix,
                work,
            );
        }
        let desired = items
            .into_iter()
            .map(|item| {
                self.materialized_row_fields(
                    list_id,
                    authority_source_list,
                    field_ids,
                    row_field_copies,
                    item,
                    event,
                    consumer,
                    work,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut desired_origins = BTreeSet::new();
        for (origin, _) in &desired {
            if !desired_origins.insert(origin.clone()) {
                return Err(Error::Evaluation(format!(
                    "materialized list {} produced duplicate structural row identity",
                    list_id.0
                )));
            }
        }
        let state_fields = self
            .metadata
            .indexed_state_field
            .iter()
            .filter_map(|(state, field)| {
                (self.metadata.indexed_state_owner.get(state) == Some(&list_id)).then_some(*field)
            })
            .collect::<BTreeSet<_>>();
        if let Some(authority_source_list) = authority_source_list {
            let mut ordered_rows = Vec::with_capacity(desired_len);
            for (origin, fields) in desired {
                let leaf = origin.last().copied().ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "materialized list {} authority-backed row has no structural origin",
                        list_id.0
                    ))
                })?;
                if leaf.list != authority_source_list {
                    return Err(Error::InvalidPlan(format!(
                        "materialized list {} authority-backed row originates in ListId {}, expected {}",
                        list_id.0, leaf.list.0, authority_source_list.0
                    )));
                }
                let row = RowId {
                    list: list_id,
                    key: leaf.key,
                    generation: leaf.generation,
                };
                let current_origin = self.row_owner_ancestors(row)?;
                let mut target_origin = origin.clone();
                let target_leaf = target_origin.last_mut().expect("origin has a leaf");
                target_leaf.list = list_id;
                if current_origin != target_origin.as_slice() {
                    return Err(Error::InvalidPlan(format!(
                        "materialized list {} authority row {}:{} has stale structural identity",
                        list_id.0, row.key, row.generation
                    )));
                }
                for (field, value) in fields {
                    if state_fields.contains(&field) {
                        continue;
                    }
                    self.record_row_field_undo(row, field, work);
                    self.set_row_authority_field(row, field, value, work)?;
                }
                ordered_rows.push(row);
            }
            return Ok(EvalValue::List(
                ordered_rows.into_iter().map(EvalValue::Row).collect(),
            ));
        }
        let mut existing_by_origin = self
            .lists
            .get(&list_id)
            .and_then(|list| list.owner_partitions.get(owner_prefix))
            .map(|partition| partition.by_materialization_origin.clone())
            .unwrap_or_default();
        if existing_by_origin.len() != existing.len() {
            return Err(Error::InvalidPlan(format!(
                "list {} owner partition mixes materialized and authoritative rows",
                list_id.0
            )));
        }
        let insertion_index = self
            .lists
            .get(&list_id)
            .and_then(|list| list.order.minimum_position(&existing))
            .unwrap_or(current_len);
        let unmatched = existing_by_origin
            .iter()
            .filter_map(|(origin, row)| (!desired_origins.contains(origin)).then_some(*row))
            .collect::<Vec<_>>();
        for row in unmatched.into_iter().rev() {
            self.remove_row(row, work)?;
        }

        let mut ordered_rows = Vec::with_capacity(desired_len);
        for (origin, fields) in desired {
            let row = if let Some(row) = existing_by_origin.remove(&origin) {
                row
            } else {
                self.append_row_with_owner_prefix(
                    list_id,
                    fields.clone(),
                    owner_prefix,
                    Some(origin),
                    work,
                )?
            };
            for (field, value) in fields {
                if state_fields.contains(&field) {
                    continue;
                }
                self.record_row_field_undo(row, field, work);
                self.set_row_authority_field(row, field, value, work)?;
            }
            ordered_rows.push(row);
        }
        if self.set_materialized_partition_order(list_id, insertion_index, &ordered_rows, work)? {
            work.authority_deltas.push(AuthorityDelta::ReplaceList {
                list_id,
                authority: self.list_authority(list_id)?,
            });
            self.invalidate_list_structure(list_id, work);
        }
        Ok(EvalValue::List(
            ordered_rows.into_iter().map(EvalValue::Row).collect(),
        ))
    }

    fn reconcile_value_list_authority(
        &mut self,
        authority: &PlanValueListAuthority,
        value: EvalValue,
        row: Option<RowId>,
        _event: Option<&SourceEvent>,
        _consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let owner_prefix = row
            .map(|row| self.row_owner_ancestors(row).map(<[_]>::to_vec))
            .transpose()?
            .unwrap_or_default();
        let items = eval_to_list(value)?;
        work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
        self.reconcile_unkeyed_value_list_records(
            authority.list_id,
            &authority.fields,
            items,
            &owner_prefix,
            work,
        )
    }

    fn reconcile_unkeyed_value_list_records(
        &mut self,
        list_id: ListId,
        fields_by_name: &BTreeMap<String, FieldId>,
        items: Vec<EvalValue>,
        owner_prefix: &[OwnerInstanceRow],
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let desired = items
            .into_iter()
            .map(|item| {
                let fields = match item {
                    EvalValue::Record(fields) => fields,
                    EvalValue::Value(Value::Record(fields)) => fields
                        .into_iter()
                        .map(|(name, value)| (name, EvalValue::Value(value)))
                        .collect(),
                    other => {
                        return Err(Error::Evaluation(format!(
                            "typed value-list authority {} received non-record row {other:?}",
                            list_id.0
                        )));
                    }
                };
                let fields = fields
                    .into_iter()
                    .map(|(name, value)| {
                        let field = fields_by_name.get(&name).copied().ok_or_else(|| {
                            Error::InvalidPlan(format!(
                                "typed value-list authority {} record field `{name}` has no compiled FieldId",
                                list_id.0
                            ))
                        })?;
                        Ok((field, self.materialize_eval(value)?))
                    })
                    .collect::<Result<BTreeMap<_, _>, Error>>()?;
                self.materialized_constructor_fields(list_id, fields)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let existing = self.list_row_ids_for_owner(list_id, owner_prefix)?;
        let unchanged = existing.len() == desired.len()
            && existing.iter().zip(&desired).all(|(row, fields)| {
                self.lists
                    .get(&list_id)
                    .and_then(|list| list.rows.get(row))
                    .is_some_and(|stored| {
                        fields
                            .iter()
                            .all(|(field, value)| stored.fields.get(field) == Some(value))
                    })
            });
        if unchanged {
            return Ok(EvalValue::List(
                existing.into_iter().map(EvalValue::Row).collect(),
            ));
        }

        for row in existing.into_iter().rev() {
            self.remove_row(row, work)?;
        }
        let mut rows = Vec::with_capacity(desired.len());
        for fields in desired {
            rows.push(self.append_row_with_owner_prefix(
                list_id,
                fields,
                owner_prefix,
                None,
                work,
            )?);
        }
        Ok(EvalValue::List(
            rows.into_iter().map(EvalValue::Row).collect(),
        ))
    }

    fn set_materialized_partition_order(
        &mut self,
        list_id: ListId,
        insertion_index: usize,
        ordered_rows: &[RowId],
        work: &mut Work,
    ) -> Result<bool, Error> {
        let (prepared, owner_partition) = {
            let list = self
                .lists
                .get(&list_id)
                .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
            let Some(prepared) = list.prepare_reorder_rows(insertion_index, ordered_rows, None)?
            else {
                return Ok(false);
            };
            let owner_partition = ordered_rows
                .first()
                .map(|first| {
                    let row = list.rows.get(first).ok_or_else(|| {
                        Error::InvalidPlan("materialized partition row disappeared".to_owned())
                    })?;
                    let (_, owner_prefix) = row.owner_ancestors.split_last().ok_or_else(|| {
                        Error::InvalidPlan("materialized partition row has no owner".to_owned())
                    })?;
                    let owner_prefix = owner_prefix.to_vec();
                    let previous_order = list
                        .owner_partitions
                        .get(&owner_prefix)
                        .ok_or_else(|| {
                            Error::InvalidPlan("materialized owner partition is missing".to_owned())
                        })?
                        .order
                        .clone();
                    Ok((owner_prefix, previous_order))
                })
                .transpose()?;
            (prepared, owner_partition)
        };
        if work.emit {
            self.turn_sequence
                .checked_add(1)
                .ok_or_else(|| Error::Evaluation("list revision overflow".to_owned()))?;
        }
        let maintenance = prepared.maintenance.clone();
        let prepared_index_dirty = self.prepare_ordered_index_rows_dirty_for_list(
            list_id,
            maintenance.changed_order_rows.iter().copied(),
        );
        charge_source_order_maintenance(work, &maintenance)?;
        prepared_index_dirty.charge(work)?;
        let undo = {
            let list = self
                .lists
                .get_mut(&list_id)
                .expect("prepared materialized list remains present");
            let undo = prepared.commit(list);
            if let Some((owner_prefix, _)) = &owner_partition {
                list.owner_partitions
                    .get_mut(owner_prefix)
                    .expect("prepared owner partition remains present")
                    .order = ordered_rows.to_vec();
            }
            undo
        };
        work.authority_undo.push(AuthorityUndo::ReorderRows {
            list: list_id,
            undo,
            owner_partition,
        });
        record_source_order_maintenance(work, &maintenance);
        self.mark_list_semantic_change(list_id, work)?;
        let fanout = prepared_index_dirty.fanout;
        self.commit_ordered_index_dirty(prepared_index_dirty);
        record_ordered_index_fanout(work, fanout);
        Ok(true)
    }

    fn list_row_ids_for_owner(
        &self,
        list_id: ListId,
        owner_prefix: &[OwnerInstanceRow],
    ) -> Result<Vec<RowId>, Error> {
        let Some(list) = self.lists.get(&list_id) else {
            return Ok(Vec::new());
        };
        Ok(list
            .owner_partitions
            .get(owner_prefix)
            .map(|partition| partition.order.clone())
            .unwrap_or_default())
    }

    fn materialized_row_fields(
        &mut self,
        list_id: ListId,
        authority_source_list: Option<ListId>,
        field_ids: &BTreeMap<String, FieldId>,
        row_field_copies: &[PlanMaterializedRowFieldCopy],
        item: EvalValue,
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<(Vec<OwnerInstanceRow>, BTreeMap<FieldId, Value>), Error> {
        let (origin, fields, captures) = match item {
            EvalValue::Record(fields) => (Vec::new(), fields, BTreeMap::new()),
            EvalValue::MappedRow {
                id,
                fields,
                captures,
            } => (self.row_owner_ancestors(id)?.to_vec(), fields, captures),
            EvalValue::Value(Value::Record(fields)) => (
                Vec::new(),
                fields
                    .into_iter()
                    .map(|(name, value)| (name, EvalValue::Value(value)))
                    .collect(),
                BTreeMap::new(),
            ),
            EvalValue::Value(Value::MappedRow { id, fields }) => (
                self.row_owner_ancestors(id)?.to_vec(),
                fields
                    .into_iter()
                    .map(|(name, value)| (name, EvalValue::Value(value)))
                    .collect(),
                BTreeMap::new(),
            ),
            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => {
                let origin = self.row_owner_ancestors(row)?.to_vec();
                if row.list == list_id && authority_source_list == Some(list_id) {
                    return Ok((origin, BTreeMap::new()));
                }
                let copies = row_field_copies
                    .iter()
                    .filter(|copy| copy.source_list == row.list)
                    .copied()
                    .collect::<Vec<_>>();
                if copies.is_empty() {
                    return Err(Error::Evaluation(format!(
                        "materialized list {} has no typed field copies for source list {}",
                        list_id.0, row.list.0
                    )));
                }
                let mut fields = BTreeMap::new();
                for copy in copies {
                    self.register_row_dependency(consumer, row, copy.source_field);
                    let value = self.ensure_row_field(row, copy.source_field, event, work)?;
                    fields.insert(copy.target_field, value);
                }
                return Ok((
                    origin,
                    self.materialized_constructor_fields(list_id, fields)?,
                ));
            }
            other => {
                return Err(Error::Evaluation(format!(
                    "materialized list {} produced non-record row {other:?}",
                    list_id.0
                )));
            }
        };
        let mut fields = fields
            .into_iter()
            .map(|(name, value)| {
                let field = field_ids.get(&name).copied().ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "materialized list {} record field `{name}` has no compiled FieldId",
                        list_id.0
                    ))
                })?;
                Ok((field, self.materialize_eval(value)?))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        for (field, value) in captures {
            if self.metadata.row_field_owner.get(&field) != Some(&list_id)
                || !self.metadata.capture_fields.contains(&field)
            {
                return Err(Error::InvalidPlan(format!(
                    "materialized list {} received capture field {} owned elsewhere",
                    list_id.0, field.0
                )));
            }
            let value = self.materialize_eval(value)?;
            if fields.insert(field, value).is_some() {
                return Err(Error::InvalidPlan(format!(
                    "materialized list {} capture field {} conflicts with a visible field",
                    list_id.0, field.0
                )));
            }
        }
        Ok((
            origin,
            self.materialized_constructor_fields(list_id, fields)?,
        ))
    }

    fn materialized_constructor_fields(
        &self,
        list_id: ListId,
        mut fields: BTreeMap<FieldId, Value>,
    ) -> Result<BTreeMap<FieldId, Value>, Error> {
        for ((candidate_list, name), authority_field) in &self.metadata.list_authority_fields {
            if *candidate_list != list_id {
                continue;
            }
            let Some(value_fields) = self
                .metadata
                .list_fields_by_name
                .get(&(list_id, name.clone()))
            else {
                continue;
            };
            let mut values = value_fields
                .iter()
                .filter_map(|field| fields.get(field))
                .cloned();
            let Some(value) = values.next() else {
                continue;
            };
            if values.any(|candidate| candidate != value) {
                return Err(Error::InvalidPlan(format!(
                    "materialized list {} produced conflicting value fields for constructor `{name}`",
                    list_id.0
                )));
            }
            if let Some(existing) = fields.get(authority_field)
                && existing != &value
            {
                return Err(Error::InvalidPlan(format!(
                    "materialized list {} produced conflicting authority field for constructor `{name}`",
                    list_id.0
                )));
            }
            fields.insert(*authority_field, value);
        }
        Ok(fields)
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_value_ref(
        &mut self,
        value_ref: &ValueRef,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        self.eval_expression_entry(
            ExpressionEntry::ValueRef {
                value_ref: value_ref.clone(),
                context: ExpressionContext {
                    row,
                    event,
                    output,
                    consumer,
                },
            },
            &mut BTreeMap::new(),
            work,
        )
    }
    fn rollback_expression_currentness(&mut self, target: ExpressionCurrentnessTarget) {
        match target {
            ExpressionCurrentnessTarget::Root(field) => {
                if let Some(cell) = self.root_fields.get_mut(&field)
                    && cell.currentness == Currentness::Evaluating
                {
                    cell.currentness = Currentness::Dirty;
                }
            }
            ExpressionCurrentnessTarget::Row(row, field) => {
                if self
                    .lists
                    .get(&row.list)
                    .and_then(|list| list.rows.get(&row))
                    .and_then(|row| row.derived.get(&field))
                    == Some(&Currentness::Evaluating)
                {
                    let _ = self.set_row_currentness(row, field, Currentness::Dirty);
                }
            }
            ExpressionCurrentnessTarget::List(list) => {
                if let Some(cell) = self.derived_lists.get_mut(&list)
                    && cell.currentness == Currentness::Evaluating
                {
                    cell.currentness = Currentness::Dirty;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_indexed_contextual_candidates(
        &mut self,
        access: &PlanContextualIndexedAccess,
        context: ExpressionContext<'_>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<Vec<RowId>, Error> {
        let metadata = Arc::clone(&self.metadata);
        let index_plan = metadata.list_indexes.get(&access.index).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "typed contextual access references missing index {}",
                access.index.0
            ))
        })?;
        let (selection, _) = self.evaluate_list_access_selection(
            &access.selection,
            index_plan,
            context.row,
            context.event,
            context.output,
            context.consumer,
            bindings,
            work,
        )?;
        self.register_list_access_dependency(context.consumer, access.index, &selection);
        self.ensure_ordered_index_current(access.index, context.consumer, work)?;
        let index = self.ordered_indexes.remove(&access.index).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "typed contextual access index {} is not current",
                access.index.0
            ))
        })?;
        let limits = self.options.list_access_work_limits;
        let mut tracker = AccessWorkTracker::new(AccessWorkLimits::new(
            limits.max_index_seeks,
            limits.max_key_ranges,
            limits.max_keys_visited,
            limits.max_candidates_visited,
            limits.max_rows_returned,
            limits.max_branch_polls,
            0,
        ));
        let result = (|| {
            let mut stream =
                open_evaluated_list_access(&index, &selection, None, index_plan.keys.len())
                    .map_err(|error| Error::Evaluation(error.to_string()))?;
            let mut candidates = Vec::new();
            while let Some(candidate) = stream
                .next(&mut tracker)
                .map_err(|error| Error::Evaluation(error.to_string()))?
            {
                work.consume(1)?;
                let candidate = runtime_row_id(index_plan.source_list, candidate.row_id());
                if !self.row_exists(candidate) {
                    return Err(Error::InvalidPlan(format!(
                        "typed contextual access on index {} returned stale row {}:{}:{}",
                        access.index.0, candidate.list.0, candidate.key, candidate.generation
                    )));
                }
                candidates.push(candidate);
            }
            Ok(candidates)
        })();
        let metrics = tracker.metrics();
        self.ordered_indexes.insert(access.index, index);
        work.metrics.indexed_access_count = work.metrics.indexed_access_count.saturating_add(1);
        work.metrics.index_candidate_count = work
            .metrics
            .index_candidate_count
            .saturating_add(metrics.candidates_visited as usize);
        record_access_metrics(work, metrics, 0);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn eval_row_expression(
        &mut self,
        expression: impl IntoExpressionId,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let expression = expression.into_expression_id();
        self.eval_expression_entry(
            ExpressionEntry::Row {
                expression,
                context: ExpressionContext {
                    row,
                    event,
                    output,
                    consumer,
                },
            },
            bindings,
            work,
        )
    }

    fn eval_expression_entry(
        &mut self,
        entry: ExpressionEntry<'_>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let plan = Arc::clone(&self.plan);
        let metadata = Arc::clone(&self.metadata);
        let stack_limit = plan
            .row_expressions
            .len()
            .saturating_mul(4)
            .saturating_add(metadata.derived_expression_count.saturating_mul(4))
            .saturating_add(metadata.root_computations.len().saturating_mul(4))
            .saturating_add(metadata.row_computations.len().saturating_mul(4))
            .saturating_add(metadata.list_computations.len().saturating_mul(4))
            .saturating_add(256)
            .max(256);
        let mut stack = ExpressionWorkStack::new(stack_limit);
        let mut binding_undos = Vec::<ExpressionBindingUndo>::new();
        let mut currentness_targets = Vec::<ExpressionCurrentnessTarget>::new();
        let authority_depth = work.active_value_list_authorities.len();
        match &entry {
            ExpressionEntry::Row {
                expression,
                context,
            } => stack.push_task(ExpressionTask::Evaluate {
                expression: *expression,
                context: *context,
            })?,
            ExpressionEntry::CurrentnessOp { op, row, event } => {
                stack.push_task(ExpressionTask::EvaluateCurrentnessOp {
                    op: *op,
                    row: *row,
                    event: *event,
                })?
            }
            ExpressionEntry::ValueRef { value_ref, context } => {
                stack.push_task(ExpressionTask::ValueRef {
                    value_ref: ExpressionValueRef::Derived(value_ref),
                    context: *context,
                })?
            }
        }

        let result = (|| {
            while let Some(task) = stack.tasks.pop() {
                match task {
                    ExpressionTask::Evaluate {
                        expression,
                        context,
                    } => {
                        work.consume(1)?;
                        let node = plan
                            .row_expressions
                            .node(expression)
                            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                        match node {
                            PlanRowExpressionNode::Intrinsic { intrinsic } => stack
                                .push_value(EvalValue::Value(self.eval_intrinsic(*intrinsic)))?,
                            PlanRowExpressionNode::Field {
                                input: ValueRef::DistributedImport(import_id),
                            } if self.metadata.row_owned_call_results.contains_key(import_id) => {
                                stack.push_task(ExpressionTask::BeginRowOwnedCall {
                                    import_id: *import_id,
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::Field { .. } => {
                                stack.push_task(ExpressionTask::ValueRef {
                                    value_ref: ExpressionValueRef::Arena(expression),
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::Constant { constant_id } => {
                                let value = self
                                    .metadata
                                    .constants
                                    .get(constant_id)
                                    .cloned()
                                    .map(EvalValue::Value)
                                    .ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "missing constant {}",
                                            constant_id.0
                                        ))
                                    })?;
                                stack.push_value(value)?;
                            }
                            PlanRowExpressionNode::ListRef { list_id } => {
                                self.register_list_dependency(context.consumer, *list_id);
                                if self.derived_lists.contains_key(list_id) {
                                    stack.push_task(ExpressionTask::EnsureList {
                                        list: *list_id,
                                        event: context.event,
                                    })?;
                                } else {
                                    let rows = self.list_row_ids(*list_id);
                                    work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                                    stack.push_value(EvalValue::List(
                                        rows.into_iter().map(EvalValue::Row).collect(),
                                    ))?;
                                }
                            }
                            PlanRowExpressionNode::AuthorityListRef { list_id } => {
                                self.register_list_dependency(context.consumer, *list_id);
                                let rows = self.list_row_ids(*list_id);
                                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                                stack.push_value(EvalValue::List(
                                    rows.into_iter().map(EvalValue::Row).collect(),
                                ))?;
                            }
                            PlanRowExpressionNode::Local {
                                owner,
                                local,
                                projection,
                            } => {
                                let mut value =
                                    bindings.get(&(*owner, *local)).cloned().ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "contextual owner {} local {} is not active",
                                            owner.0, local.0
                                        ))
                                    })?;
                                for field in projection {
                                    value =
                                        self.eval_object_field(value, field, context.consumer)?;
                                }
                                stack.push_value(value)?;
                            }
                            PlanRowExpressionNode::LocalRow { owner, local } => {
                                let value =
                                    bindings.get(&(*owner, *local)).cloned().ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "contextual row owner {} local {} is not active",
                                            owner.0, local.0
                                        ))
                                    })?;
                                stack.push_value(value)?;
                            }
                            PlanRowExpressionNode::EventRow { source, list_id } => {
                                let event = context.event.ok_or_else(|| {
                                    Error::InvalidPlan(format!(
                                        "event row for source {} was evaluated without an active source event",
                                        source.0
                                    ))
                                })?;
                                if event.source != *source {
                                    return Err(Error::InvalidPlan(format!(
                                        "event row expects source {}, received {}",
                                        source.0, event.source.0
                                    )));
                                }
                                let row = context.row.or(event.target).ok_or_else(|| {
                                    Error::InvalidPlan(format!(
                                        "event row for source {} has no exact row target",
                                        source.0
                                    ))
                                })?;
                                if row.list != *list_id || event.target != Some(row) {
                                    return Err(Error::InvalidPlan(format!(
                                        "event row for source {} targets {:?}, expected exact ListId {} row {:?}",
                                        source.0, event.target, list_id.0, row
                                    )));
                                }
                                let leaf = event.route.owner.leaf().ok_or_else(|| {
                                    Error::InvalidPlan(format!(
                                        "event row for source {} has no owner-instance leaf",
                                        source.0
                                    ))
                                })?;
                                if leaf.list != row.list
                                    || leaf.key != row.key
                                    || leaf.generation != row.generation
                                {
                                    return Err(Error::InvalidPlan(format!(
                                        "event row for source {} does not match its owner-instance route",
                                        source.0
                                    )));
                                }
                                stack.push_value(EvalValue::Row(row))?;
                            }
                            PlanRowExpressionNode::ContextualCollection {
                                owner,
                                operation,
                                source,
                                row_local,
                                body,
                                indexed_access: None,
                                ..
                            } => {
                                stack.push_task(
                                    ExpressionTask::ContextualCollectionAfterSource {
                                        expression,
                                        owner: *owner,
                                        operation: *operation,
                                        row_local: *row_local,
                                        body: *body,
                                        context,
                                    },
                                )?;
                                stack.push_task(ExpressionTask::Evaluate {
                                    expression: *source,
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::ListGetField {
                                list_id,
                                index,
                                field,
                            } => {
                                stack.push_task(ExpressionTask::ListGetFieldAfterIndex {
                                    list: *list_id,
                                    field: *field,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::Evaluate {
                                    expression: *index,
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::ListRowField {
                                row: row_expression,
                                list_id,
                                field,
                            } => {
                                stack.push_task(ExpressionTask::ListRowFieldAfterRow {
                                    list: *list_id,
                                    field: *field,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::Evaluate {
                                    expression: *row_expression,
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::ContextualCollection {
                                owner,
                                operation,
                                row_local,
                                body,
                                captures,
                                indexed_access: Some(access),
                                ..
                            } => {
                                if !captures.is_empty() {
                                    return Err(Error::InvalidPlan(
                                        "indexed contextual access cannot carry row captures"
                                            .to_owned(),
                                    ));
                                }
                                let candidates = self.prepare_indexed_contextual_candidates(
                                    access, context, bindings, work,
                                )?;
                                let capacity = candidates.len();
                                stack.push_task(ExpressionTask::IndexedContextualNext {
                                    state: Box::new(IndexedContextualState {
                                        operation: *operation,
                                        local: (*owner, *row_local),
                                        body: *body,
                                        remaining: candidates.into_iter(),
                                        retained: Vec::with_capacity(capacity),
                                    }),
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::ContextualOrder {
                                owner,
                                operation,
                                source,
                                row_local,
                                key,
                                direction,
                            } => {
                                stack.push_task(ExpressionTask::ContextualOrderAfterSource {
                                    operation: *operation,
                                    owner: *owner,
                                    row_local: *row_local,
                                    key: *key,
                                    direction: *direction,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::Evaluate {
                                    expression: *source,
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::ListAccess { access } => {
                                let value = self.evaluate_list_access(
                                    access,
                                    context.row,
                                    context.event,
                                    context.output,
                                    context.consumer,
                                    bindings,
                                    work,
                                )?;
                                stack.push_value(value)?;
                            }
                            PlanRowExpressionNode::ListPage { page } => {
                                let value = self.evaluate_list_page(
                                    page,
                                    context.row,
                                    context.event,
                                    context.output,
                                    context.consumer,
                                    bindings,
                                    work,
                                )?;
                                stack.push_value(value)?;
                            }
                            PlanRowExpressionNode::BoundedListPage { page } => {
                                let value = self.evaluate_bounded_list_page(
                                    page,
                                    context.row,
                                    context.event,
                                    context.output,
                                    context.consumer,
                                    bindings,
                                    work,
                                )?;
                                stack.push_value(value)?;
                            }
                            PlanRowExpressionNode::BuiltinCall {
                                function,
                                input,
                                args,
                            } if matches!(
                                function,
                                PlanRowBuiltin::BoolAnd | PlanRowBuiltin::BoolOr
                            ) =>
                            {
                                let left = (*input).ok_or_else(|| {
                                    Error::InvalidPlan(format!(
                                        "{} has no compiled input",
                                        function.function_name()
                                    ))
                                })?;
                                let right = args
                                    .iter()
                                    .find(|argument| argument.name == "right")
                                    .map(|argument| argument.value)
                                    .ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "{} has no compiled right operand",
                                            function.function_name()
                                        ))
                                    })?;
                                stack.push_task(ExpressionTask::BuiltinBoolAfterLeft {
                                    function: *function,
                                    right,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::Evaluate {
                                    expression: left,
                                    context,
                                })?;
                            }
                            PlanRowExpressionNode::BuiltinCall {
                                function,
                                input,
                                args,
                            } => {
                                let value_base = stack.values.len();
                                stack.push_task(ExpressionTask::BuiltinAfterOperands {
                                    expression,
                                    value_base,
                                })?;
                                schedule_builtin_operands(
                                    &mut stack, *function, *input, args, context,
                                )?;
                            }
                            PlanRowExpressionNode::Select { input, .. } => {
                                stack.push_task(ExpressionTask::SelectAfterInput {
                                    expression,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::Evaluate {
                                    expression: *input,
                                    context,
                                })?;
                            }
                            node => {
                                let value_base = stack.values.len();
                                stack.push_task(ExpressionTask::Apply {
                                    expression,
                                    value_base,
                                    context,
                                })?;
                                schedule_apply_operands(&mut stack, node, context)?;
                            }
                        }
                    }
                    ExpressionTask::Apply {
                        expression,
                        value_base,
                        context,
                    } => {
                        let node = plan
                            .row_expressions
                            .node(expression)
                            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                        let value = self.apply_row_expression_node(
                            expression,
                            node,
                            &mut stack.values,
                            value_base,
                            context,
                            work,
                        )?;
                        stack.push_value(value)?;
                    }
                    ExpressionTask::EvaluateDerived {
                        expression,
                        context,
                    } => {
                        work.consume(1)?;
                        match expression {
                            PlanDerivedExpression::MaterializeList {
                                target_list,
                                authority_source_list,
                                fields,
                                row_field_copies,
                                value_list_authorities,
                                expression,
                            } => {
                                let owner_prefix = context
                                    .row
                                    .map(|row| self.row_owner_ancestors(row).map(<[_]>::to_vec))
                                    .transpose()?
                                    .unwrap_or_default();
                                let authority_depth = work.active_value_list_authorities.len();
                                if authority_depth.saturating_add(value_list_authorities.len())
                                    > stack.limit
                                {
                                    return Err(Error::InvalidPlan(format!(
                                        "value-list authority stack exceeded its plan-derived bound of {}",
                                        stack.limit
                                    )));
                                }
                                work.active_value_list_authorities
                                    .extend(value_list_authorities.iter().cloned());
                                stack.push_task(ExpressionTask::MaterializeListAfterValue {
                                    target_list: *target_list,
                                    authority_source_list: *authority_source_list,
                                    fields,
                                    row_field_copies,
                                    owner_prefix,
                                    authority_depth,
                                    event: context.event,
                                })?;
                                stack.push_task(ExpressionTask::EvaluateDerived {
                                    expression,
                                    context: ExpressionContext {
                                        consumer: Some(Consumer::List(*target_list)),
                                        ..context
                                    },
                                })?;
                            }
                            PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                                source_id,
                                key_field,
                                required_key,
                                state,
                                skip_empty,
                            } => {
                                let Some(event) = context.event else {
                                    stack.push_value(EvalValue::Value(Value::Null))?;
                                    continue;
                                };
                                if event.source != *source_id {
                                    stack.push_value(EvalValue::Value(Value::Null))?;
                                    continue;
                                }
                                let key = source_payload_value(&event.payload, key_field)
                                    .map(|value| value_to_text(&value))
                                    .transpose()?
                                    .unwrap_or_default();
                                if key != *required_key {
                                    stack.push_value(EvalValue::Value(Value::Null))?;
                                    continue;
                                }
                                stack.push_task(ExpressionTask::DerivedSourceKeyAfterValue {
                                    skip_empty: *skip_empty,
                                })?;
                                stack.push_task(ExpressionTask::ValueRef {
                                    value_ref: ExpressionValueRef::Derived(state),
                                    context,
                                })?;
                            }
                            PlanDerivedExpression::SourceEventTransform {
                                default, arms, ..
                            } => {
                                let selected = if let Some(ActiveTrigger {
                                    cause: TriggerCause::State(state),
                                    target,
                                    ..
                                }) = &work.active_trigger
                                {
                                    arms.iter()
                                        .find(|arm| {
                                            matches!(
                                                &arm.trigger,
                                                ValueRef::State(trigger)
                                                    if trigger == state
                                            )
                                        })
                                        .map(|arm| {
                                            (
                                                arm.value,
                                                context.row.or(*target).or_else(|| {
                                                    context.event.and_then(|event| event.target)
                                                }),
                                            )
                                        })
                                } else {
                                    None
                                }
                                .or_else(|| {
                                    context.event.and_then(|event| {
                                        arms.iter()
                                            .find(|arm| {
                                                matches!(
                                                    &arm.trigger,
                                                    ValueRef::Source(source)
                                                        if *source == event.source
                                                )
                                            })
                                            .map(|arm| (arm.value, context.row.or(event.target)))
                                    })
                                })
                                .unwrap_or((*default, context.row));
                                schedule_isolated_expression(
                                    &mut stack,
                                    &mut binding_undos,
                                    bindings,
                                    None,
                                    selected.0,
                                    ExpressionContext {
                                        row: selected.1,
                                        ..context
                                    },
                                )?;
                            }
                            PlanDerivedExpression::BoolNot { input } => {
                                stack.push_task(ExpressionTask::DerivedBoolNotAfterValue)?;
                                stack.push_task(ExpressionTask::ValueRef {
                                    value_ref: ExpressionValueRef::Derived(input),
                                    context,
                                })?;
                            }
                            PlanDerivedExpression::NumberCompareConst { left, op, right } => {
                                stack.push_task(
                                    ExpressionTask::DerivedNumberCompareAfterValue {
                                        op: *op,
                                        right: *right,
                                    },
                                )?;
                                stack.push_task(ExpressionTask::ValueRef {
                                    value_ref: ExpressionValueRef::Derived(left),
                                    context,
                                })?;
                            }
                            PlanDerivedExpression::ValueCompare { left, op, right } => {
                                stack.push_task(ExpressionTask::DerivedValueCompareAfterLeft {
                                    op: *op,
                                    right,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::ValueRef {
                                    value_ref: ExpressionValueRef::Derived(left),
                                    context,
                                })?;
                            }
                            PlanDerivedExpression::BoolAnd { left, right } => {
                                stack.push_task(ExpressionTask::DerivedBoolAndAfterLeft {
                                    right,
                                    context,
                                })?;
                                stack.push_task(ExpressionTask::EvaluateDerived {
                                    expression: left,
                                    context,
                                })?;
                            }
                            PlanDerivedExpression::BoolNotExpression { input } => {
                                stack.push_task(
                                    ExpressionTask::DerivedBoolNotExpressionAfterValue,
                                )?;
                                stack.push_task(ExpressionTask::EvaluateDerived {
                                    expression: input,
                                    context,
                                })?;
                            }
                            PlanDerivedExpression::RowExpression { expression } => {
                                schedule_isolated_expression(
                                    &mut stack,
                                    &mut binding_undos,
                                    bindings,
                                    None,
                                    *expression,
                                    ExpressionContext {
                                        row: expression_row(context.row),
                                        ..context
                                    },
                                )?;
                            }
                            PlanDerivedExpression::MaterializedRowField { local, expression } => {
                                let row = context.row.ok_or_else(|| {
                                    Error::InvalidPlan(
                                        "materialized row field was evaluated without an exact row"
                                            .to_owned(),
                                    )
                                })?;
                                let binding = local.map(|local| {
                                    (EvalValue::Row(row), (local.owner, local.row_local))
                                });
                                schedule_isolated_expression(
                                    &mut stack,
                                    &mut binding_undos,
                                    bindings,
                                    binding,
                                    *expression,
                                    ExpressionContext {
                                        row: Some(row),
                                        ..context
                                    },
                                )?;
                            }
                        }
                    }
                    ExpressionTask::MaterializeListAfterValue {
                        target_list,
                        authority_source_list,
                        fields,
                        row_field_copies,
                        owner_prefix,
                        authority_depth,
                        event,
                    } => {
                        work.active_value_list_authorities.truncate(authority_depth);
                        let value = match stack.pop_value()? {
                            EvalValue::Value(Value::Null) => EvalValue::List(Vec::new()),
                            EvalValue::Value(Value::Text(value)) if value == "SKIP" => {
                                EvalValue::List(Vec::new())
                            }
                            value @ (EvalValue::Record(_)
                            | EvalValue::MappedRow { .. }
                            | EvalValue::Row(_)
                            | EvalValue::Value(Value::Record(_))
                            | EvalValue::Value(Value::MappedRow { .. })
                            | EvalValue::Value(Value::Row { .. })) => EvalValue::List(vec![value]),
                            value => value,
                        };
                        let consumer = Some(Consumer::List(target_list));
                        let value = self.reconcile_materialized_list(
                            target_list,
                            authority_source_list,
                            fields,
                            row_field_copies,
                            value,
                            &owner_prefix,
                            event,
                            consumer,
                            work,
                        )?;
                        stack.push_value(value)?;
                    }
                    ExpressionTask::DerivedSourceKeyAfterValue { skip_empty } => {
                        let text = eval_to_text(&stack.pop_value()?)?.trim().to_owned();
                        let value = if skip_empty && text.is_empty() {
                            Value::Null
                        } else {
                            Value::Text(text)
                        };
                        stack.push_value(EvalValue::Value(value))?;
                    }
                    ExpressionTask::DerivedBoolNotAfterValue => {
                        let value = !eval_to_bool(&stack.pop_value()?)?;
                        stack.push_value(EvalValue::Value(Value::Bool(value)))?;
                    }
                    ExpressionTask::DerivedNumberCompareAfterValue { op, right } => {
                        let left = eval_to_numeric(&stack.pop_value()?)?;
                        let value = numeric_compare(left, op, right)?;
                        stack.push_value(EvalValue::Value(Value::Bool(value)))?;
                    }
                    ExpressionTask::DerivedValueCompareAfterLeft { op, right, context } => {
                        let left = stack.pop_value()?;
                        stack.push_task(ExpressionTask::DerivedValueCompareAfterRight {
                            op,
                            left,
                        })?;
                        stack.push_task(ExpressionTask::ValueRef {
                            value_ref: ExpressionValueRef::Derived(right),
                            context,
                        })?;
                    }
                    ExpressionTask::DerivedValueCompareAfterRight { op, left } => {
                        let right = stack.pop_value()?;
                        let EvalValue::Value(left) = left else {
                            return Err(Error::Evaluation(
                                "left comparison operand is not a scalar value".to_owned(),
                            ));
                        };
                        let EvalValue::Value(right) = right else {
                            return Err(Error::Evaluation(
                                "right comparison operand is not a scalar value".to_owned(),
                            ));
                        };
                        let value = compare_update_values(&left, op, &right)?;
                        stack.push_value(EvalValue::Value(Value::Bool(value)))?;
                    }
                    ExpressionTask::DerivedBoolAndAfterLeft { right, context } => {
                        if !eval_to_bool(&stack.pop_value()?)? {
                            stack.push_value(EvalValue::Value(Value::Bool(false)))?;
                        } else {
                            stack.push_task(ExpressionTask::DerivedBoolAndAfterRight)?;
                            stack.push_task(ExpressionTask::EvaluateDerived {
                                expression: right,
                                context,
                            })?;
                        }
                    }
                    ExpressionTask::DerivedBoolAndAfterRight => {
                        let value = eval_to_bool(&stack.pop_value()?)?;
                        stack.push_value(EvalValue::Value(Value::Bool(value)))?;
                    }
                    ExpressionTask::DerivedBoolNotExpressionAfterValue => {
                        let value = !eval_to_bool(&stack.pop_value()?)?;
                        stack.push_value(EvalValue::Value(Value::Bool(value)))?;
                    }
                    ExpressionTask::BeginRowOwnedCall { import_id, context } => {
                        let call =
                            metadata
                                .row_owned_call_results
                                .get(&import_id)
                                .ok_or_else(|| {
                                    Error::InvalidPlan(
                                        "distributed import is not a row-owned call result"
                                            .to_owned(),
                                    )
                                })?;
                        let consumer = context.consumer.ok_or_else(|| {
                            Error::InvalidPlan(
                                "current call was evaluated without a retained currentness consumer"
                                    .to_owned(),
                            )
                        })?;
                        let mut instance_rows = Vec::with_capacity(call.row_bindings.len());
                        for binding in &call.row_bindings {
                            let active_row = match bindings.get(&(binding.owner, binding.local)) {
                                Some(EvalValue::Row(row)) => *row,
                                Some(_) => {
                                    return Err(Error::InvalidPlan(
                                        "row-owned call binding is not a row".to_owned(),
                                    ));
                                }
                                None => {
                                    return Err(Error::InvalidPlan(
                                        "row-owned call is missing a required row binding"
                                            .to_owned(),
                                    ));
                                }
                            };
                            if active_row.list != binding.list {
                                return Err(Error::InvalidPlan(
                                    "row-owned call binding resolved to the wrong list".to_owned(),
                                ));
                            }
                            instance_rows.push(DistributedCallInstanceRow {
                                owner: binding.owner,
                                local: binding.local,
                                row: OwnerInstanceRow {
                                    list: active_row.list,
                                    key: active_row.key,
                                    generation: active_row.generation,
                                },
                            });
                        }
                        let call_instance_id = self
                            .distributed_call_instance_id(call.call_site_id, &instance_rows)
                            .map_err(|error| {
                                Error::InvalidPlan(format!(
                                    "current call has invalid instance identity: {error}"
                                ))
                            })?;
                        stack.push_task(ExpressionTask::RowOwnedCallNext {
                            state: RowOwnedCallState {
                                import_id,
                                call,
                                call_instance_id,
                                consumer,
                                context,
                                next_argument: 0,
                                arguments: BTreeMap::new(),
                            },
                        })?;
                    }
                    ExpressionTask::RowOwnedCallNext { state } => {
                        let Some(argument) = state.call.arguments.get(state.next_argument) else {
                            let result_arguments = state.arguments.clone();
                            self.register_distributed_current_call_demand(
                                state.consumer,
                                state.call.call_site_id,
                                state.call_instance_id,
                                state.arguments,
                            )?;
                            self.register_distributed_call_result_dependency(
                                state.consumer,
                                state.import_id,
                                state.call_instance_id,
                            );
                            let value = self
                                .row_owned_call_results
                                .get(&(state.import_id, state.call_instance_id))
                                .filter(|result| result.arguments == result_arguments)
                                .map(|result| result.value.clone())
                                .unwrap_or_else(|| Value::Error {
                                    code: "remote_not_current".to_owned(),
                                });
                            stack.push_value(EvalValue::Value(value))?;
                            continue;
                        };
                        let expression = argument.value;
                        let context = ExpressionContext {
                            consumer: Some(state.consumer),
                            ..state.context
                        };
                        stack.push_task(ExpressionTask::RowOwnedCallAfterArgument { state })?;
                        stack.push_task(ExpressionTask::Evaluate {
                            expression,
                            context,
                        })?;
                    }
                    ExpressionTask::RowOwnedCallAfterArgument { mut state } => {
                        let argument =
                            state
                                .call
                                .arguments
                                .get(state.next_argument)
                                .ok_or_else(|| {
                                    Error::InvalidPlan(
                                        "row-owned call argument continuation is out of range"
                                            .to_owned(),
                                    )
                                })?;
                        let value = self.materialize_eval(stack.pop_value()?)?;
                        validate_distributed_boundary_value(
                            &value,
                            &argument.data_type,
                            &format!("remote call argument `{}`", argument.name),
                        )?;
                        if state
                            .arguments
                            .insert(argument.argument_id, value)
                            .is_some()
                        {
                            return Err(Error::InvalidPlan(
                                "row-owned call has a duplicate boundary argument".to_owned(),
                            ));
                        }
                        state.next_argument = state.next_argument.saturating_add(1);
                        stack.push_task(ExpressionTask::RowOwnedCallNext { state })?;
                    }
                    ExpressionTask::ValueRef { value_ref, context } => {
                        work.consume(1)?;
                        if let ExpressionValueRef::List(list) = value_ref {
                            stack.push_task(ExpressionTask::ListValue { list, context })?;
                            continue;
                        }
                        let (expression, value_ref, derived_value_ref) = match value_ref {
                            ExpressionValueRef::Arena(expression) => {
                                let node = plan
                                    .row_expressions
                                    .node(expression)
                                    .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                                let PlanRowExpressionNode::Field { input } = node else {
                                    return Err(Error::InvalidPlan(format!(
                                        "row expression {} is not a value reference",
                                        expression.0
                                    )));
                                };
                                (Some(expression), input, false)
                            }
                            ExpressionValueRef::List(_) => unreachable!(),
                            ExpressionValueRef::Derived(value_ref) => (None, value_ref, true),
                        };
                        match value_ref {
                            ValueRef::Source(source) => {
                                stack.push_value(EvalValue::Value(Value::Bool(
                                    context.event.is_some_and(|event| event.source == *source),
                                )))?
                            }
                            ValueRef::SourcePayload { source_id, field } => {
                                let value = context
                                    .event
                                    .filter(|event| event.source == *source_id)
                                    .and_then(|event| source_payload_value(&event.payload, field))
                                    .unwrap_or(Value::Null);
                                stack.push_value(EvalValue::Value(value))?;
                            }
                            ValueRef::Constant(constant) => {
                                let value = self
                                    .metadata
                                    .constants
                                    .get(constant)
                                    .cloned()
                                    .map(EvalValue::Value)
                                    .ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "missing constant {}",
                                            constant.0
                                        ))
                                    })?;
                                stack.push_value(value)?;
                            }
                            ValueRef::DistributedImport(import_id) => {
                                if self.metadata.row_owned_call_results.contains_key(import_id) {
                                    stack.push_task(ExpressionTask::BeginRowOwnedCall {
                                        import_id: *import_id,
                                        context,
                                    })?;
                                } else {
                                    self.register_distributed_import_dependency(
                                        context.consumer,
                                        *import_id,
                                    );
                                    let value = self
                                        .distributed_imports
                                        .get(import_id)
                                        .cloned()
                                        .map(EvalValue::Value)
                                        .ok_or_else(|| {
                                            Error::InvalidPlan(
                                                "value ref uses an undeclared distributed import"
                                                    .to_owned(),
                                            )
                                        })?;
                                    stack.push_value(value)?;
                                }
                            }
                            ValueRef::List(list) => {
                                stack.push_task(ExpressionTask::ListValue {
                                    list: *list,
                                    context,
                                })?;
                            }
                            ValueRef::State(state) => {
                                stack.push_task(ExpressionTask::StateValue {
                                    state: *state,
                                    context,
                                })?;
                            }
                            ValueRef::StateProjection {
                                state_id,
                                field_path,
                            } => {
                                if let Some(expression) = expression {
                                    stack.push_task(ExpressionTask::StateProjectionAfterValue {
                                        expression,
                                        state: *state_id,
                                    })?;
                                } else if derived_value_ref {
                                    stack.push_task(
                                        ExpressionTask::DerivedStateProjectionAfterValue {
                                            field_path,
                                            state: *state_id,
                                        },
                                    )?;
                                } else {
                                    return Err(Error::InvalidPlan(
                                        "synthetic state reference cannot carry a projection"
                                            .to_owned(),
                                    ));
                                }
                                stack.push_task(ExpressionTask::StateValue {
                                    state: *state_id,
                                    context,
                                })?;
                            }
                            ValueRef::Field(field) => {
                                if let Some(owner) =
                                    self.metadata.row_field_owner.get(field).copied()
                                {
                                    let row = context.row.ok_or_else(|| {
                                        Error::Evaluation(format!(
                                            "row field {} requires a row context",
                                            field.0
                                        ))
                                    })?;
                                    if row.list != owner {
                                        return Err(Error::Evaluation(format!(
                                            "field {} belongs to list {}, not {}",
                                            field.0, owner.0, row.list.0
                                        )));
                                    }
                                    if context.output == Some(*field)
                                        && self
                                            .metadata
                                            .row_computations
                                            .get(field)
                                            .is_some_and(|op| source_event_transform_op(op))
                                    {
                                        stack.push_value(EvalValue::Value(
                                            self.row_value(row, *field)?,
                                        ))?;
                                        continue;
                                    }
                                    self.register_row_dependency(context.consumer, row, *field);
                                    if self.row_field_availability(row, *field)
                                        == RowFieldAvailability::Missing
                                    {
                                        stack.push_value(EvalValue::Value(Value::Null))?;
                                    } else {
                                        stack.push_task(ExpressionTask::EnsureRow {
                                            row,
                                            field: *field,
                                            event: context.event,
                                        })?;
                                    }
                                } else {
                                    if context.output == Some(*field)
                                        && self
                                            .metadata
                                            .root_computations
                                            .get(field)
                                            .is_some_and(|op| source_event_transform_op(op))
                                        && let Some(value) = self
                                            .root_fields
                                            .get(field)
                                            .and_then(|cell| cell.value.clone())
                                    {
                                        stack.push_value(EvalValue::Value(value))?;
                                        continue;
                                    }
                                    self.register_root_field_dependency(context.consumer, *field);
                                    stack.push_task(ExpressionTask::EnsureRoot {
                                        field: *field,
                                        event: context.event,
                                    })?;
                                }
                            }
                        }
                    }
                    ExpressionTask::ListValue { list, context } => {
                        self.register_list_dependency(context.consumer, list);
                        if self.metadata.list_computations.contains_key(&list) {
                            stack.push_task(ExpressionTask::EnsureList {
                                list,
                                event: context.event,
                            })?;
                        } else {
                            let rows = self.list_row_ids(list);
                            work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                            stack.push_value(EvalValue::List(
                                rows.into_iter().map(EvalValue::Row).collect(),
                            ))?;
                        }
                    }
                    ExpressionTask::StateValue { state, context } => {
                        if let Some(owner) = self.metadata.indexed_state_owner.get(&state).copied()
                        {
                            let row = context.row.ok_or_else(|| {
                                Error::Evaluation(format!(
                                    "indexed state {} requires a row context",
                                    state.0
                                ))
                            })?;
                            if row.list != owner {
                                return Err(Error::Evaluation(format!(
                                    "indexed state {} belongs to list {}, not {}",
                                    state.0, owner.0, row.list.0
                                )));
                            }
                            let field = *self.metadata.indexed_state_field.get(&state).ok_or_else(
                                || {
                                    Error::InvalidPlan(format!(
                                        "indexed state {} has no field",
                                        state.0
                                    ))
                                },
                            )?;
                            self.register_row_dependency(context.consumer, row, field);
                            stack.push_value(EvalValue::Value(self.row_value(row, field)?))?;
                        } else {
                            self.register_root_state_dependency(context.consumer, state);
                            let value = self
                                .root_states
                                .get(&state)
                                .cloned()
                                .map(EvalValue::Value)
                                .ok_or_else(|| {
                                    Error::Evaluation(format!(
                                        "root state {} has no value",
                                        state.0
                                    ))
                                })?;
                            stack.push_value(value)?;
                        }
                    }
                    ExpressionTask::StateProjectionAfterValue { expression, state } => {
                        let node = plan
                            .row_expressions
                            .node(expression)
                            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                        let PlanRowExpressionNode::Field {
                            input: ValueRef::StateProjection { field_path, .. },
                        } = node
                        else {
                            return Err(Error::InvalidPlan(format!(
                                "row expression {} lost its state projection",
                                expression.0
                            )));
                        };
                        let EvalValue::Value(value) = stack.pop_value()? else {
                            return Err(Error::Evaluation(format!(
                                "state {} projection does not reference a scalar value",
                                state.0
                            )));
                        };
                        let value = project_value(&value, field_path)
                            .cloned()
                            .map(EvalValue::Value)
                            .ok_or_else(|| {
                                Error::Evaluation(format!(
                                    "state {} has no projection `{}`",
                                    state.0,
                                    field_path.join(".")
                                ))
                            })?;
                        stack.push_value(value)?;
                    }
                    ExpressionTask::DerivedStateProjectionAfterValue { field_path, state } => {
                        let EvalValue::Value(value) = stack.pop_value()? else {
                            return Err(Error::Evaluation(format!(
                                "state {} projection does not reference a scalar value",
                                state.0
                            )));
                        };
                        let value = project_value(&value, field_path)
                            .cloned()
                            .map(EvalValue::Value)
                            .ok_or_else(|| {
                                Error::Evaluation(format!(
                                    "state {} has no projection `{}`",
                                    state.0,
                                    field_path.join(".")
                                ))
                            })?;
                        stack.push_value(value)?;
                    }
                    ExpressionTask::EnsureRoot { field, event } => {
                        self.flush_list_access_dependencies(work)?;
                        let currentness = self
                            .root_fields
                            .get(&field)
                            .map(|cell| cell.currentness)
                            .ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "field {} has no root computation",
                                    field.0
                                ))
                            })?;
                        match currentness {
                            Currentness::Current => {
                                let value = self
                                    .root_fields
                                    .get(&field)
                                    .and_then(|cell| cell.value.clone())
                                    .map(EvalValue::Value)
                                    .ok_or_else(|| {
                                        Error::Evaluation(format!(
                                            "current root field {} has no value",
                                            field.0
                                        ))
                                    })?;
                                stack.push_value(value)?;
                            }
                            Currentness::Evaluating => {
                                return Err(Error::Cycle { field, row: None });
                            }
                            Currentness::Dirty => {
                                if currentness_targets.len() >= stack.limit {
                                    return Err(Error::InvalidPlan(format!(
                                        "expression currentness stack exceeded its plan-derived bound of {}",
                                        stack.limit
                                    )));
                                }
                                work.consume(1)?;
                                self.root_fields
                                    .get_mut(&field)
                                    .expect("root cell checked above")
                                    .currentness = Currentness::Evaluating;
                                currentness_targets.push(ExpressionCurrentnessTarget::Root(field));
                                let consumer = Consumer::Root(field);
                                self.clear_consumer_dependencies(consumer);
                                let op = metadata
                                    .root_computations
                                    .get(&field)
                                    .map(|op| op.id)
                                    .ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "root field {} has no plan op",
                                            field.0
                                        ))
                                    })?;
                                stack.push_task(ExpressionTask::FinishRoot { field })?;
                                stack.push_task(ExpressionTask::EvaluateCurrentnessOp {
                                    op,
                                    row: None,
                                    event,
                                })?;
                            }
                        }
                    }
                    ExpressionTask::FinishRoot { field } => {
                        let value = self.materialize_eval(stack.pop_value()?)?;
                        if stack.values.len() >= stack.limit {
                            return Err(Error::InvalidPlan(format!(
                                "expression value stack exceeded its plan-derived bound of {}",
                                stack.limit
                            )));
                        }
                        let old = self
                            .root_fields
                            .get(&field)
                            .and_then(|cell| cell.value.clone());
                        {
                            let cell = self.root_fields.get_mut(&field).ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "field {} has no root computation",
                                    field.0
                                ))
                            })?;
                            cell.value = Some(value.clone());
                            cell.currentness = Currentness::Current;
                        }
                        finish_expression_currentness(
                            &mut currentness_targets,
                            ExpressionCurrentnessTarget::Root(field),
                        )?;
                        work.metrics.recomputed_field_count += 1;
                        work.recomputed_targets.insert(ValueTarget::Field(field));
                        if old.as_ref() != Some(&value) {
                            self.invalidate_root_field(field, work);
                            if work.emit {
                                work.deltas.push(Delta::SetValue {
                                    target: ValueTarget::Field(field),
                                    value: value.clone(),
                                });
                            }
                        }
                        stack.push_value(EvalValue::Value(value))?;
                    }
                    ExpressionTask::EnsureRow { row, field, event } => {
                        self.flush_list_access_dependencies(work)?;
                        let field = self.resolve_row_field_alias(row, field);
                        if self.touched_row_fields.contains(&(row, field))
                            && self
                                .lists
                                .get(&row.list)
                                .and_then(|list| list.rows.get(&row))
                                .is_some_and(|row| row.default_fields.contains(&field))
                        {
                            stack.push_value(EvalValue::Value(self.row_value(row, field)?))?;
                            continue;
                        }
                        let currentness = self
                            .lists
                            .get(&row.list)
                            .and_then(|list| list.rows.get(&row))
                            .and_then(|row| row.derived.get(&field))
                            .copied();
                        let Some(currentness) = currentness else {
                            stack.push_value(EvalValue::Value(self.row_value(row, field)?))?;
                            continue;
                        };
                        match currentness {
                            Currentness::Current => {
                                stack.push_value(EvalValue::Value(self.row_value(row, field)?))?;
                            }
                            Currentness::Evaluating => {
                                return Err(Error::Cycle {
                                    field,
                                    row: Some(row),
                                });
                            }
                            Currentness::Dirty => {
                                if currentness_targets.len() >= stack.limit {
                                    return Err(Error::InvalidPlan(format!(
                                        "expression currentness stack exceeded its plan-derived bound of {}",
                                        stack.limit
                                    )));
                                }
                                work.consume(1)?;
                                self.set_row_currentness(row, field, Currentness::Evaluating)?;
                                currentness_targets
                                    .push(ExpressionCurrentnessTarget::Row(row, field));
                                let consumer = Consumer::Row(row, field);
                                self.clear_consumer_dependencies(consumer);
                                let op = metadata.row_computations.get(&field).map(|op| op.id);
                                let default = op
                                    .is_none()
                                    .then(|| self.row_default_expression(row, field))
                                    .flatten();
                                stack.push_task(ExpressionTask::FinishRow { row, field })?;
                                if let Some(op) = op {
                                    stack.push_task(ExpressionTask::EvaluateCurrentnessOp {
                                        op,
                                        row: Some(row),
                                        event,
                                    })?;
                                } else if let Some(expression) = default {
                                    schedule_isolated_expression(
                                        &mut stack,
                                        &mut binding_undos,
                                        bindings,
                                        None,
                                        expression,
                                        ExpressionContext {
                                            row: Some(row),
                                            event,
                                            output: Some(field),
                                            consumer: Some(consumer),
                                        },
                                    )?;
                                } else {
                                    return Err(Error::InvalidPlan(format!(
                                        "row field {} has no plan op or row default",
                                        field.0
                                    )));
                                }
                            }
                        }
                    }
                    ExpressionTask::FinishRow { row, field } => {
                        let value = self.materialize_eval(stack.pop_value()?)?;
                        if stack.values.len() >= stack.limit {
                            return Err(Error::InvalidPlan(format!(
                                "expression value stack exceeded its plan-derived bound of {}",
                                stack.limit
                            )));
                        }
                        self.set_row_field(row, field, value.clone(), work)?;
                        self.set_row_currentness(row, field, Currentness::Current)?;
                        finish_expression_currentness(
                            &mut currentness_targets,
                            ExpressionCurrentnessTarget::Row(row, field),
                        )?;
                        work.metrics.recomputed_field_count += 1;
                        work.recomputed_targets
                            .insert(ValueTarget::RowField { row, field });
                        stack.push_value(EvalValue::Value(value))?;
                    }
                    ExpressionTask::EnsureList { list, event } => {
                        self.flush_list_access_dependencies(work)?;
                        let currentness = self
                            .derived_lists
                            .get(&list)
                            .map(|cell| cell.currentness)
                            .ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "list {} has no derived computation",
                                    list.0
                                ))
                            })?;
                        match currentness {
                            Currentness::Current
                                if self
                                    .derived_lists
                                    .get(&list)
                                    .is_some_and(|cell| cell.items.is_some()) =>
                            {
                                let items = self
                                    .derived_lists
                                    .get(&list)
                                    .and_then(|cell| cell.items.clone())
                                    .ok_or_else(|| {
                                        Error::Evaluation(format!(
                                            "current derived list {} has no items",
                                            list.0
                                        ))
                                    })?;
                                stack.push_value(EvalValue::List(items))?;
                            }
                            Currentness::Evaluating => {
                                return Err(Error::ListCycle { list });
                            }
                            Currentness::Current | Currentness::Dirty => {
                                if currentness_targets.len() >= stack.limit {
                                    return Err(Error::InvalidPlan(format!(
                                        "expression currentness stack exceeded its plan-derived bound of {}",
                                        stack.limit
                                    )));
                                }
                                work.consume(1)?;
                                let virtual_rows = {
                                    let cell = self
                                        .derived_lists
                                        .get_mut(&list)
                                        .expect("derived list checked above");
                                    cell.currentness = Currentness::Evaluating;
                                    cell.items = None;
                                    cell.window
                                        .take()
                                        .map(|window| {
                                            window.rows_by_index.into_values().collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default()
                                };
                                currentness_targets.push(ExpressionCurrentnessTarget::List(list));
                                for row in virtual_rows.into_iter().rev() {
                                    if self.row_exists(row) {
                                        self.remove_row(row, work)?;
                                    }
                                }
                                let consumer = Consumer::List(list);
                                self.clear_consumer_dependencies(consumer);
                                let op = metadata
                                    .list_computations
                                    .get(&list)
                                    .map(|op| op.id)
                                    .ok_or_else(|| {
                                        Error::InvalidPlan(format!(
                                            "derived list {} has no plan op",
                                            list.0
                                        ))
                                    })?;
                                stack.push_task(ExpressionTask::FinishList { list, op })?;
                                stack.push_task(ExpressionTask::EvaluateCurrentnessOp {
                                    op,
                                    row: None,
                                    event,
                                })?;
                            }
                        }
                    }
                    ExpressionTask::FinishList { list, op } => {
                        let evaluated = stack.pop_value()?;
                        let EvalValue::List(items) = evaluated else {
                            return Err(Error::InvalidPlan(format!(
                                "list computation {} did not produce a list",
                                op.0
                            )));
                        };
                        if stack.values.len() >= stack.limit {
                            return Err(Error::InvalidPlan(format!(
                                "expression value stack exceeded its plan-derived bound of {}",
                                stack.limit
                            )));
                        }
                        let old = self
                            .derived_lists
                            .get(&list)
                            .and_then(|cell| cell.items.clone());
                        {
                            let cell = self.derived_lists.get_mut(&list).ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "list {} has no derived computation",
                                    list.0
                                ))
                            })?;
                            cell.items = Some(items.clone());
                            cell.window = None;
                            cell.currentness = Currentness::Current;
                        }
                        finish_expression_currentness(
                            &mut currentness_targets,
                            ExpressionCurrentnessTarget::List(list),
                        )?;
                        work.metrics.recomputed_list_count += 1;
                        if old.as_ref() != Some(&items) {
                            self.invalidate_list_structure(list, work);
                        }
                        stack.push_value(EvalValue::List(items))?;
                    }
                    ExpressionTask::EvaluateCurrentnessOp { op, row, event } => {
                        let op = metadata.currentness_ops.get(&op).ok_or_else(|| {
                            Error::InvalidPlan(format!(
                                "currentness op {} has no immutable metadata",
                                op.0
                            ))
                        })?;
                        match &op.kind {
                            PlanOpKind::DerivedValue {
                                derived_kind,
                                expression,
                                ..
                            } => {
                                let Some(expression) = expression else {
                                    let output_label = match op.output.as_ref() {
                                        Some(ValueRef::Field(field)) => debug_label(
                                            &self.plan.debug_map.fields,
                                            "field:",
                                            field.0,
                                        ),
                                        _ => None,
                                    };
                                    return Err(Error::Unsupported {
                                        op: op.id,
                                        detail: format!(
                                            "{derived_kind:?} derived value has no typed expression; output={:?}, path={}",
                                            op.output,
                                            output_label.unwrap_or("<unknown>")
                                        ),
                                    });
                                };
                                let consumer = (!source_event_transform_op(op))
                                    .then(|| {
                                        op.output.as_ref().and_then(|output| match output {
                                            ValueRef::Field(field) => Some(match row {
                                                Some(row) => Consumer::Row(row, *field),
                                                None => Consumer::Root(*field),
                                            }),
                                            ValueRef::List(list) => Some(Consumer::List(*list)),
                                            _ => None,
                                        })
                                    })
                                    .flatten();
                                let output = op.output.as_ref().and_then(|output| match output {
                                    ValueRef::Field(field) => Some(*field),
                                    _ => None,
                                });
                                stack.push_task(ExpressionTask::EvaluateDerived {
                                    expression,
                                    context: ExpressionContext {
                                        row,
                                        event,
                                        output,
                                        consumer,
                                    },
                                })?;
                            }
                            PlanOpKind::ListProjection { projection } => {
                                let (consumer, output, target) = match op.output.as_ref() {
                                    Some(ValueRef::Field(field)) => (
                                        Some(Consumer::Root(*field)),
                                        Some(*field),
                                        ExpressionProjectionTarget::Value,
                                    ),
                                    Some(ValueRef::List(list)) => (
                                        Some(Consumer::List(*list)),
                                        None,
                                        ExpressionProjectionTarget::List(*list),
                                    ),
                                    output => {
                                        return Err(Error::InvalidPlan(format!(
                                            "list projection {} has unsupported output {output:?}",
                                            op.id.0
                                        )));
                                    }
                                };
                                let context = ExpressionContext {
                                    row: None,
                                    event,
                                    output,
                                    consumer,
                                };
                                match projection {
                                    PlanListProjection::Chunk { source_list, size } => {
                                        if *size == 0 {
                                            return Err(Error::InvalidPlan(format!(
                                                "chunk projection {} has size zero",
                                                op.id.0
                                            )));
                                        }
                                        stack.push_task(ExpressionTask::ProjectionAfterSource {
                                            op: op.id,
                                            size: *size,
                                            target,
                                        })?;
                                        stack.push_task(ExpressionTask::ValueRef {
                                            value_ref: ExpressionValueRef::List(*source_list),
                                            context,
                                        })?;
                                    }
                                    PlanListProjection::ChunkValue { source, size } => {
                                        if *size == 0 {
                                            return Err(Error::InvalidPlan(format!(
                                                "chunk-value projection {} has size zero",
                                                op.id.0
                                            )));
                                        }
                                        stack.push_task(ExpressionTask::ProjectionAfterSource {
                                            op: op.id,
                                            size: *size,
                                            target,
                                        })?;
                                        stack.push_task(ExpressionTask::ValueRef {
                                            value_ref: ExpressionValueRef::Derived(source),
                                            context,
                                        })?;
                                    }
                                    PlanListProjection::Unknown { summary } => {
                                        return Err(Error::Unsupported {
                                            op: op.id,
                                            detail: format!("unknown list projection: {summary}"),
                                        });
                                    }
                                }
                            }
                            _ => {
                                return Err(Error::Unsupported {
                                    op: op.id,
                                    detail: "operation cannot produce a derived current value"
                                        .to_owned(),
                                });
                            }
                        }
                    }
                    ExpressionTask::ProjectionAfterSource { op, size, target } => {
                        let rows = eval_to_list(stack.pop_value()?)?;
                        work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                        let mut chunks = Vec::new();
                        for (index, chunk) in rows.chunks(size).enumerate() {
                            chunks.push(EvalValue::Record(BTreeMap::from([
                                (
                                    "label".to_owned(),
                                    EvalValue::Value(Value::Text(index.to_string())),
                                ),
                                ("items".to_owned(), EvalValue::List(chunk.to_vec())),
                            ])));
                        }
                        let projected = EvalValue::List(chunks);
                        let value = match target {
                            ExpressionProjectionTarget::Value => projected,
                            ExpressionProjectionTarget::List(list) => {
                                self.reconcile_positional_list_projection(list, projected, work)?
                            }
                        };
                        if !matches!(value, EvalValue::List(_)) {
                            return Err(Error::InvalidPlan(format!(
                                "list projection {} did not produce a list",
                                op.0
                            )));
                        }
                        stack.push_value(value)?;
                    }
                    ExpressionTask::ListGetFieldAfterIndex {
                        list,
                        field,
                        context,
                    } => {
                        let index =
                            nonnegative_usize(eval_to_integer(&stack.pop_value()?)?, "list index")?;
                        let target = self
                            .lists
                            .get(&list)
                            .and_then(|list| list.order.get(index))
                            .copied()
                            .ok_or_else(|| {
                                Error::Evaluation(format!(
                                    "list {} index {index} is out of range",
                                    list.0
                                ))
                            })?;
                        self.register_row_dependency(context.consumer, target, field);
                        stack.push_task(ExpressionTask::EnsureRow {
                            row: target,
                            field,
                            event: context.event,
                        })?;
                    }
                    ExpressionTask::ListRowFieldAfterRow {
                        list,
                        field,
                        context,
                    } => {
                        let value = stack.pop_value()?;
                        let row = match value {
                            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => {
                                row
                            }
                            other => {
                                return Err(Error::Evaluation(format!(
                                    "value {other:?} is not a typed list row"
                                )));
                            }
                        };
                        if row.list != list {
                            return Err(Error::InvalidPlan(format!(
                                "typed row field {} belongs to list {}, but expression produced list {}",
                                field.0, list.0, row.list.0
                            )));
                        }
                        if self.metadata.row_field_owner.get(&field) != Some(&list) {
                            return Err(Error::InvalidPlan(format!(
                                "typed row field {} (`{}`) does not belong to list {}",
                                field.0,
                                self.metadata
                                    .field_labels
                                    .get(&field)
                                    .map(String::as_str)
                                    .unwrap_or("<unlabeled>"),
                                list.0
                            )));
                        }
                        self.register_row_dependency(context.consumer, row, field);
                        if self.row_field_availability(row, field) == RowFieldAvailability::Missing
                        {
                            stack.push_value(EvalValue::Value(Value::Null))?;
                        } else {
                            stack.push_task(ExpressionTask::EnsureRow {
                                row,
                                field,
                                event: context.event,
                            })?;
                        }
                    }
                    ExpressionTask::SelectAfterInput {
                        expression,
                        context,
                    } => {
                        let node = plan
                            .row_expressions
                            .node(expression)
                            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                        let PlanRowExpressionNode::Select { arms, .. } = node else {
                            return Err(Error::InvalidPlan(format!(
                                "row expression {} lost its select continuation",
                                expression.0
                            )));
                        };
                        let input = self.materialize_eval(stack.pop_value()?)?;
                        let expression = arms
                            .iter()
                            .find_map(|arm| {
                                select_pattern_matches(&arm.pattern, &input).then_some(arm.value)
                            })
                            .ok_or_else(|| {
                                Error::Evaluation(format!(
                                    "select has no matching arm for {input:?}"
                                ))
                            })?;
                        stack.push_task(ExpressionTask::Evaluate {
                            expression,
                            context,
                        })?;
                    }
                    ExpressionTask::BuiltinAfterOperands {
                        expression,
                        value_base,
                    } => {
                        let node = plan
                            .row_expressions
                            .node(expression)
                            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                        let PlanRowExpressionNode::BuiltinCall {
                            function,
                            input: compiled_input,
                            args: compiled_args,
                        } = node
                        else {
                            return Err(Error::InvalidPlan(format!(
                                "row expression {} lost its builtin continuation",
                                expression.0
                            )));
                        };
                        let function = *function;
                        if value_base > stack.values.len() {
                            return Err(Error::InvalidPlan(format!(
                                "{} has an invalid value-stack boundary",
                                function.function_name()
                            )));
                        }
                        let mut next_value = || {
                            if value_base >= stack.values.len() {
                                return Err(Error::InvalidPlan(format!(
                                    "{} produced too few evaluated operands",
                                    function.function_name()
                                )));
                            }
                            Ok(stack.values.remove(value_base))
                        };
                        let mut input = None;
                        let mut args = EvaluatedBuiltinArgs::new();
                        if function == PlanRowBuiltin::ListTake {
                            args.insert("count", next_value()?)?;
                            input = Some(next_value()?);
                        } else {
                            for parameter in function.signature().parameters {
                                if parameter.receiver {
                                    if compiled_input.is_some() {
                                        input = Some(next_value()?);
                                    }
                                } else if compiled_args
                                    .iter()
                                    .any(|argument| argument.name == parameter.name)
                                {
                                    args.insert(parameter.name, next_value()?)?;
                                }
                            }
                        }
                        drop(next_value);
                        if stack.values.len() != value_base {
                            return Err(Error::InvalidPlan(format!(
                                "{} produced extra evaluated operands",
                                function.function_name()
                            )));
                        }
                        let value = self.eval_builtin_values(function, input, args, work)?;
                        stack.push_value(value)?;
                    }
                    ExpressionTask::BuiltinBoolAfterLeft {
                        function,
                        right,
                        context,
                    } => {
                        let left = eval_to_bool(&stack.pop_value()?)?;
                        let short_circuit = match function {
                            PlanRowBuiltin::BoolAnd => !left,
                            PlanRowBuiltin::BoolOr => left,
                            _ => {
                                return Err(Error::InvalidPlan(
                                    "non-boolean builtin used a boolean continuation".to_owned(),
                                ));
                            }
                        };
                        if short_circuit {
                            stack.push_value(EvalValue::Value(Value::Bool(left)))?;
                        } else {
                            stack.push_task(ExpressionTask::BuiltinBoolAfterRight)?;
                            stack.push_task(ExpressionTask::Evaluate {
                                expression: right,
                                context,
                            })?;
                        }
                    }
                    ExpressionTask::BuiltinBoolAfterRight => {
                        let right = eval_to_bool(&stack.pop_value()?)?;
                        stack.push_value(EvalValue::Value(Value::Bool(right)))?;
                    }
                    ExpressionTask::ContextualCollectionAfterSource {
                        expression,
                        owner,
                        operation,
                        row_local,
                        body,
                        context,
                    } => {
                        let input = stack.pop_value()?;
                        let input = match work
                            .active_value_list_authorities
                            .iter()
                            .rev()
                            .find(|authority| authority.owner == owner)
                            .cloned()
                        {
                            Some(authority) => self.reconcile_value_list_authority(
                                &authority,
                                input,
                                context.row,
                                context.event,
                                context.consumer,
                                work,
                            )?,
                            None => input,
                        };
                        let (items, directions) = eval_to_ordered_items(input)?;
                        let local = (owner, row_local);
                        if bindings.contains_key(&local) {
                            return Err(Error::InvalidPlan(format!(
                                "contextual owner {} local {} is already active",
                                owner.0, row_local.0
                            )));
                        }
                        if matches!(
                            operation,
                            PlanContextualOperationKind::Map
                                | PlanContextualOperationKind::Filter
                                | PlanContextualOperationKind::Retain
                                | PlanContextualOperationKind::Remove
                        ) {
                            work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                        }
                        let capacity = items.len();
                        stack.push_task(ExpressionTask::ContextualCollectionNext {
                            state: Box::new(ContextualCollectionState {
                                expression,
                                owner,
                                operation,
                                local,
                                body,
                                remaining: items.into_iter(),
                                directions,
                                output: Vec::with_capacity(capacity),
                            }),
                            context,
                        })?;
                    }
                    ExpressionTask::ContextualCollectionNext { mut state, context } => {
                        let Some(item) = state.remaining.next() else {
                            let value = match state.operation {
                                PlanContextualOperationKind::Map
                                | PlanContextualOperationKind::Filter
                                | PlanContextualOperationKind::Retain
                                | PlanContextualOperationKind::Remove => {
                                    eval_ordered_items(state.output, state.directions)
                                }
                                PlanContextualOperationKind::Every => {
                                    EvalValue::Value(Value::Bool(true))
                                }
                                PlanContextualOperationKind::Any => {
                                    EvalValue::Value(Value::Bool(false))
                                }
                                PlanContextualOperationKind::Find => {
                                    EvalValue::Record(BTreeMap::from([(
                                        "$tag".to_owned(),
                                        EvalValue::Value(Value::Text("NotFound".to_owned())),
                                    )]))
                                }
                            };
                            stack.push_value(value)?;
                            continue;
                        };
                        match state.operation {
                            PlanContextualOperationKind::Map => {
                                let origin = eval_row_id(&item.value);
                                let local = state.local;
                                let body = state.body;
                                let binding_value = item.value.clone();
                                stack.push_task(
                                    ExpressionTask::ContextualCollectionAfterMapBody {
                                        state,
                                        item: item.clone(),
                                        origin,
                                        context,
                                    },
                                )?;
                                schedule_bound_expression(
                                    &mut stack,
                                    &mut binding_undos,
                                    bindings,
                                    (binding_value, local),
                                    body,
                                    context,
                                )?;
                            }
                            PlanContextualOperationKind::Filter
                            | PlanContextualOperationKind::Retain
                            | PlanContextualOperationKind::Remove
                            | PlanContextualOperationKind::Every
                            | PlanContextualOperationKind::Any
                            | PlanContextualOperationKind::Find => {
                                if matches!(
                                    state.operation,
                                    PlanContextualOperationKind::Every
                                        | PlanContextualOperationKind::Any
                                        | PlanContextualOperationKind::Find
                                ) {
                                    work.consume(1)?;
                                }
                                if state.operation == PlanContextualOperationKind::Find {
                                    work.metrics.list_find_scan_count += 1;
                                }
                                let local = state.local;
                                let body = state.body;
                                let binding_value = item.value.clone();
                                stack.push_task(
                                    ExpressionTask::ContextualCollectionAfterPredicate {
                                        state,
                                        item: item.clone(),
                                        context,
                                    },
                                )?;
                                schedule_bound_expression(
                                    &mut stack,
                                    &mut binding_undos,
                                    bindings,
                                    (binding_value, local),
                                    body,
                                    context,
                                )?;
                            }
                        }
                    }
                    ExpressionTask::ContextualCollectionAfterPredicate {
                        mut state,
                        item,
                        context,
                    } => {
                        let matches = eval_to_bool(&stack.pop_value()?)?;
                        match state.operation {
                            PlanContextualOperationKind::Filter
                            | PlanContextualOperationKind::Retain => {
                                if matches {
                                    state.output.push(item);
                                }
                            }
                            PlanContextualOperationKind::Remove => {
                                if !matches {
                                    state.output.push(item);
                                }
                            }
                            PlanContextualOperationKind::Every if !matches => {
                                stack.push_value(EvalValue::Value(Value::Bool(false)))?;
                                continue;
                            }
                            PlanContextualOperationKind::Any if matches => {
                                stack.push_value(EvalValue::Value(Value::Bool(true)))?;
                                continue;
                            }
                            PlanContextualOperationKind::Find if matches => {
                                stack.push_value(EvalValue::Record(BTreeMap::from([
                                    (
                                        "$tag".to_owned(),
                                        EvalValue::Value(Value::Text("Found".to_owned())),
                                    ),
                                    ("value".to_owned(), item.value),
                                ])))?;
                                continue;
                            }
                            PlanContextualOperationKind::Map => {
                                return Err(Error::InvalidPlan(
                                    "map used a predicate continuation".to_owned(),
                                ));
                            }
                            _ => {}
                        }
                        stack.push_task(ExpressionTask::ContextualCollectionNext {
                            state,
                            context,
                        })?;
                    }
                    ExpressionTask::ContextualCollectionAfterMapBody {
                        state,
                        item,
                        origin,
                        context,
                    } => {
                        let mapped = stack.pop_value()?;
                        stack.push_task(ExpressionTask::ContextualMapCaptureNext {
                            state: Box::new(ContextualMapCaptureState {
                                collection: state,
                                item,
                                origin,
                                mapped,
                                next_capture: 0,
                                evaluated_captures: BTreeMap::new(),
                            }),
                            context,
                        })?;
                    }
                    ExpressionTask::ContextualMapCaptureNext { mut state, context } => {
                        let node = plan
                            .row_expressions
                            .node(state.collection.expression)
                            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
                        let PlanRowExpressionNode::ContextualCollection { captures, .. } = node
                        else {
                            return Err(Error::InvalidPlan(format!(
                                "row expression {} lost its contextual collection continuation",
                                state.collection.expression.0
                            )));
                        };
                        let Some(capture) = captures.get(state.next_capture) else {
                            state.item.value = match (
                                state.origin,
                                state.mapped,
                                state.evaluated_captures,
                            ) {
                                (Some(id), EvalValue::Record(fields), captures) => {
                                    EvalValue::MappedRow {
                                        id,
                                        fields,
                                        captures,
                                    }
                                }
                                (_, value, captures) if captures.is_empty() => value,
                                _ => {
                                    return Err(Error::InvalidPlan(format!(
                                        "contextual owner {} captures state for a map without a stored source row and typed record result",
                                        state.collection.owner.0
                                    )));
                                }
                            };
                            state.collection.output.push(state.item);
                            stack.push_task(ExpressionTask::ContextualCollectionNext {
                                state: state.collection,
                                context,
                            })?;
                            continue;
                        };
                        let capture_field = capture.field;
                        let capture_value = capture.value;
                        if !self.metadata.capture_fields.contains(&capture_field) {
                            return Err(Error::InvalidPlan(format!(
                                "contextual owner {} writes non-capture field {}",
                                state.collection.owner.0, capture_field.0
                            )));
                        }
                        state.next_capture += 1;
                        let binding_value = state.item.value.clone();
                        let local = state.collection.local;
                        stack.push_task(ExpressionTask::ContextualMapCaptureAfterValue {
                            state,
                            field: capture_field,
                            context,
                        })?;
                        schedule_bound_expression(
                            &mut stack,
                            &mut binding_undos,
                            bindings,
                            (binding_value, local),
                            capture_value,
                            context,
                        )?;
                    }
                    ExpressionTask::ContextualMapCaptureAfterValue {
                        mut state,
                        field,
                        context,
                    } => {
                        let captured = stack.pop_value()?;
                        if state.evaluated_captures.insert(field, captured).is_some() {
                            return Err(Error::InvalidPlan(format!(
                                "contextual owner {} writes capture field {} more than once",
                                state.collection.owner.0, field.0
                            )));
                        }
                        stack.push_task(ExpressionTask::ContextualMapCaptureNext {
                            state,
                            context,
                        })?;
                    }
                    ExpressionTask::ContextualOrderAfterSource {
                        operation,
                        owner,
                        row_local,
                        key,
                        direction,
                        context,
                    } => {
                        let (items, directions) = eval_to_ordered_items(stack.pop_value()?)?;
                        stack.push_task(ExpressionTask::ContextualOrderAfterDirection {
                            operation,
                            owner,
                            row_local,
                            key,
                            items,
                            directions,
                            context,
                        })?;
                        stack.push_task(ExpressionTask::Evaluate {
                            expression: direction,
                            context,
                        })?;
                    }
                    ExpressionTask::ContextualOrderAfterDirection {
                        operation,
                        owner,
                        row_local,
                        key,
                        mut items,
                        mut directions,
                        context,
                    } => {
                        if operation == PlanOrderOperationKind::ThenBy && directions.is_none() {
                            return Err(Error::InvalidPlan(
                                "List/then_by requires a compatible preceding typed order chain"
                                    .to_owned(),
                            ));
                        }
                        if operation == PlanOrderOperationKind::SortBy {
                            directions = Some(Vec::new());
                            for item in &mut items {
                                item.keys.clear();
                            }
                        }
                        let direction = eval_order_direction(&stack.pop_value()?)?;
                        let local = (owner, row_local);
                        if bindings.contains_key(&local) {
                            return Err(Error::InvalidPlan(format!(
                                "contextual owner {} local {} is already active",
                                owner.0, row_local.0
                            )));
                        }
                        work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                        let mut directions = directions.unwrap_or_default();
                        directions.push(direction);
                        let capacity = items.len();
                        stack.push_task(ExpressionTask::ContextualOrderNext {
                            state: Box::new(ContextualOrderState {
                                local,
                                key,
                                remaining: items.into_iter(),
                                ordered: Vec::with_capacity(capacity),
                                directions,
                            }),
                            context,
                        })?;
                    }
                    ExpressionTask::ContextualOrderNext { mut state, context } => {
                        let Some(item) = state.remaining.next() else {
                            state.ordered.sort_by(|left, right| {
                                compare_ordered_items(left, right, &state.directions)
                            });
                            stack.push_value(EvalValue::OrderedList {
                                items: state.ordered,
                                directions: state.directions,
                            })?;
                            continue;
                        };
                        let local = state.local;
                        let key = state.key;
                        let binding_value = item.value.clone();
                        stack.push_task(ExpressionTask::ContextualOrderAfterKey {
                            state,
                            item: item.clone(),
                            context,
                        })?;
                        schedule_bound_expression(
                            &mut stack,
                            &mut binding_undos,
                            bindings,
                            (binding_value, local),
                            key,
                            context,
                        )?;
                    }
                    ExpressionTask::ContextualOrderAfterKey {
                        mut state,
                        mut item,
                        context,
                    } => {
                        let evaluated = self.materialize_eval(stack.pop_value()?)?;
                        item.keys.push(eval_order_key(evaluated)?);
                        state.ordered.push(item);
                        stack.push_task(ExpressionTask::ContextualOrderNext { state, context })?;
                    }
                    ExpressionTask::IndexedContextualNext { mut state, context } => {
                        let Some(candidate) = state.remaining.next() else {
                            let (value, result_count) = match state.operation {
                                PlanContextualOperationKind::Filter
                                | PlanContextualOperationKind::Retain => {
                                    let result_count = state.retained.len();
                                    (EvalValue::List(state.retained), result_count)
                                }
                                PlanContextualOperationKind::Any => {
                                    (EvalValue::Value(Value::Bool(false)), 0)
                                }
                                PlanContextualOperationKind::Find => (
                                    EvalValue::Record(BTreeMap::from([(
                                        "$tag".to_owned(),
                                        EvalValue::Value(Value::Text("NotFound".to_owned())),
                                    )])),
                                    0,
                                ),
                                operation => {
                                    return Err(Error::InvalidPlan(format!(
                                        "typed contextual access is not valid for {operation:?}"
                                    )));
                                }
                            };
                            work.metrics.access_result_count = work
                                .metrics
                                .access_result_count
                                .saturating_add(u64::try_from(result_count).unwrap_or(u64::MAX));
                            stack.push_value(value)?;
                            continue;
                        };
                        let local = state.local;
                        let body = state.body;
                        stack.push_task(ExpressionTask::IndexedContextualAfterPredicate {
                            state,
                            candidate,
                            context,
                        })?;
                        schedule_bound_expression(
                            &mut stack,
                            &mut binding_undos,
                            bindings,
                            (EvalValue::Row(candidate), local),
                            body,
                            context,
                        )?;
                    }
                    ExpressionTask::IndexedContextualAfterPredicate {
                        mut state,
                        candidate,
                        context,
                    } => {
                        let include = eval_to_bool(&stack.pop_value()?)?;
                        if include {
                            match state.operation {
                                PlanContextualOperationKind::Filter
                                | PlanContextualOperationKind::Retain => {
                                    state.retained.push(EvalValue::Row(candidate));
                                }
                                PlanContextualOperationKind::Any => {
                                    work.metrics.access_result_count =
                                        work.metrics.access_result_count.saturating_add(1);
                                    stack.push_value(EvalValue::Value(Value::Bool(true)))?;
                                    continue;
                                }
                                PlanContextualOperationKind::Find => {
                                    work.metrics.access_result_count =
                                        work.metrics.access_result_count.saturating_add(1);
                                    stack.push_value(EvalValue::Record(BTreeMap::from([
                                        (
                                            "$tag".to_owned(),
                                            EvalValue::Value(Value::Text("Found".to_owned())),
                                        ),
                                        ("value".to_owned(), EvalValue::Row(candidate)),
                                    ])))?;
                                    continue;
                                }
                                operation => {
                                    return Err(Error::InvalidPlan(format!(
                                        "typed contextual access is not valid for {operation:?}"
                                    )));
                                }
                            }
                        }
                        stack
                            .push_task(ExpressionTask::IndexedContextualNext { state, context })?;
                    }
                    ExpressionTask::RestoreBinding { undo } => {
                        if undo + 1 != binding_undos.len() {
                            return Err(Error::InvalidPlan(
                                "contextual binding continuation order is invalid".to_owned(),
                            ));
                        }
                        restore_expression_binding(
                            bindings,
                            binding_undos.pop().expect("binding undo checked above"),
                        );
                    }
                }
            }
            if stack.values.len() != 1 {
                return Err(Error::InvalidPlan(format!(
                    "expression evaluation completed with {} values",
                    stack.values.len()
                )));
            }
            stack.pop_value()
        })();

        while let Some(undo) = binding_undos.pop() {
            restore_expression_binding(bindings, undo);
        }
        while let Some(target) = currentness_targets.pop() {
            self.rollback_expression_currentness(target);
        }
        work.active_value_list_authorities.truncate(authority_depth);
        result
    }

    fn apply_row_expression_node(
        &mut self,
        expression: PlanRowExpressionId,
        node: &PlanRowExpressionNode,
        operands: &mut Vec<EvalValue>,
        value_base: usize,
        context: ExpressionContext<'_>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        if value_base > operands.len() {
            return Err(Error::InvalidPlan(format!(
                "row expression {} has an invalid value-stack boundary",
                expression.0
            )));
        }
        let mut next = || {
            if value_base >= operands.len() {
                return Err(Error::InvalidPlan(format!(
                    "row expression {} is missing an evaluated operand",
                    expression.0
                )));
            }
            Ok(operands.remove(value_base))
        };
        let value = match node {
            PlanRowExpressionNode::TextTrim { .. } => {
                EvalValue::Value(Value::Text(eval_to_text(&next()?)?.trim().to_owned()))
            }
            PlanRowExpressionNode::TextIsEmpty { .. } => {
                EvalValue::Value(Value::Bool(eval_to_text(&next()?)?.is_empty()))
            }
            PlanRowExpressionNode::TextStartsWith { .. } => {
                let input = next()?;
                let prefix = next()?;
                EvalValue::Value(Value::Bool(
                    eval_to_text(&input)?.starts_with(&eval_to_text(&prefix)?),
                ))
            }
            PlanRowExpressionNode::TextLength { .. } => EvalValue::Value(Value::integer(
                eval_to_text(&next()?)?.chars().count() as i64,
            )?),
            PlanRowExpressionNode::TextToNumber { .. } => {
                let input = next()?;
                if matches!(input, EvalValue::Value(Value::Error { .. })) {
                    input
                } else {
                    let text = eval_to_text(&input)?;
                    EvalValue::Value(match text.trim().parse::<FiniteReal>() {
                        Ok(value) => Value::Number(value),
                        Err(_) => Value::Text("NaN".to_owned()),
                    })
                }
            }
            PlanRowExpressionNode::TextSubstring { .. } => {
                let input = next()?;
                let start = nonnegative_usize(eval_to_integer(&next()?)?, "text substring start")?;
                let length =
                    nonnegative_usize(eval_to_integer(&next()?)?, "text substring length")?;
                let value = eval_to_text(&input)?
                    .chars()
                    .skip(start)
                    .take(length)
                    .collect::<String>();
                EvalValue::Value(Value::Text(value))
            }
            PlanRowExpressionNode::TextToBytes { encoding, .. } => {
                let encoding = encoding
                    .is_some()
                    .then(|| next().and_then(|value| eval_to_text(&value)))
                    .transpose()?;
                validate_encoding(encoding.as_deref())?;
                EvalValue::Value(Value::Bytes(eval_to_text(&next()?)?.into_bytes().into()))
            }
            PlanRowExpressionNode::BytesToText { encoding, .. } => {
                let encoding = encoding
                    .is_some()
                    .then(|| next().and_then(|value| eval_to_text(&value)))
                    .transpose()?;
                validate_encoding(encoding.as_deref())?;
                let bytes = eval_to_bytes(&next()?)?;
                let text = String::from_utf8(bytes.to_vec())
                    .map_err(|error| Error::Evaluation(format!("invalid UTF-8: {error}")))?;
                EvalValue::Value(Value::Text(text))
            }
            PlanRowExpressionNode::BytesToHex { .. } => {
                EvalValue::Value(Value::Text(encode_hex(&eval_to_bytes(&next()?)?)))
            }
            PlanRowExpressionNode::BytesToBase64 { .. } => {
                EvalValue::Value(Value::Text(encode_base64(&eval_to_bytes(&next()?)?)))
            }
            PlanRowExpressionNode::BytesFromHex { .. } => {
                EvalValue::Value(Value::Bytes(decode_hex(&eval_to_text(&next()?)?)?))
            }
            PlanRowExpressionNode::BytesFromBase64 { .. } => {
                EvalValue::Value(Value::Bytes(decode_base64(&eval_to_text(&next()?)?)?))
            }
            PlanRowExpressionNode::BytesIsEmpty { .. } => {
                EvalValue::Value(Value::Bool(eval_to_bytes(&next()?)?.is_empty()))
            }
            PlanRowExpressionNode::BytesLength { .. } => {
                EvalValue::Value(Value::integer(eval_to_bytes(&next()?)?.len() as i64)?)
            }
            PlanRowExpressionNode::BytesGet { .. } => {
                let bytes = eval_to_bytes(&next()?)?;
                let index = nonnegative_usize(eval_to_integer(&next()?)?, "byte index")?;
                let value = bytes.get(index).copied().ok_or_else(|| {
                    Error::Evaluation(format!("byte index {index} is out of range"))
                })?;
                EvalValue::Value(Value::Bytes(Bytes::copy_from_slice(&[value])))
            }
            PlanRowExpressionNode::BytesSlice { .. } => {
                let bytes = eval_to_bytes(&next()?)?;
                let offset = nonnegative_usize(eval_to_integer(&next()?)?, "byte offset")?;
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                EvalValue::Value(Value::Bytes(checked_slice(&bytes, offset, count)?))
            }
            PlanRowExpressionNode::BytesTake { .. } => {
                let bytes = eval_to_bytes(&next()?)?;
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                EvalValue::Value(Value::Bytes(bytes.slice(..count.min(bytes.len()))))
            }
            PlanRowExpressionNode::BytesDrop { .. } => {
                let bytes = eval_to_bytes(&next()?)?;
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                EvalValue::Value(Value::Bytes(bytes.slice(count.min(bytes.len())..)))
            }
            PlanRowExpressionNode::BytesZeros { .. } => {
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                EvalValue::Value(Value::Bytes(vec![0; count].into()))
            }
            PlanRowExpressionNode::BytesReadUnsigned { .. } => {
                let bytes = eval_to_bytes(&next()?)?;
                let offset = nonnegative_usize(eval_to_integer(&next()?)?, "byte offset")?;
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                let endian = eval_to_text(&next()?)?;
                EvalValue::Value(Value::integer(read_integer(
                    &bytes, offset, count, &endian, false,
                )?)?)
            }
            PlanRowExpressionNode::BytesReadSigned { .. } => {
                let bytes = eval_to_bytes(&next()?)?;
                let offset = nonnegative_usize(eval_to_integer(&next()?)?, "byte offset")?;
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                let endian = eval_to_text(&next()?)?;
                EvalValue::Value(Value::integer(read_integer(
                    &bytes, offset, count, &endian, true,
                )?)?)
            }
            PlanRowExpressionNode::BytesSet { .. } => {
                let mut bytes = eval_to_bytes(&next()?)?.to_vec();
                let index = nonnegative_usize(eval_to_integer(&next()?)?, "byte index")?;
                let value = eval_to_bytes(&next()?)?;
                let [value] = value.as_ref() else {
                    return Err(Error::Evaluation(format!(
                        "Bytes/set value must be BYTES[1], found {} byte(s)",
                        value.len()
                    )));
                };
                let target = bytes.get_mut(index).ok_or_else(|| {
                    Error::Evaluation(format!("byte index {index} is out of range"))
                })?;
                *target = *value;
                EvalValue::Value(Value::Bytes(bytes.into()))
            }
            PlanRowExpressionNode::BytesWriteUnsigned { .. }
            | PlanRowExpressionNode::BytesWriteSigned { .. } => {
                let mut bytes = eval_to_bytes(&next()?)?.to_vec();
                let offset = nonnegative_usize(eval_to_integer(&next()?)?, "byte offset")?;
                let count = nonnegative_usize(eval_to_integer(&next()?)?, "byte count")?;
                let endian = eval_to_text(&next()?)?;
                let value = eval_to_integer(&next()?)?;
                write_integer(&mut bytes, offset, count, &endian, value)?;
                EvalValue::Value(Value::Bytes(bytes.into()))
            }
            PlanRowExpressionNode::BytesFind { .. } => {
                let input = eval_to_bytes(&next()?)?;
                let needle = eval_to_bytes(&next()?)?;
                EvalValue::Value(match find_bytes(&input, &needle) {
                    Some(index) => Value::integer(index as i64)?,
                    None => Value::Text("NaN".to_owned()),
                })
            }
            PlanRowExpressionNode::BytesStartsWith { .. } => {
                let input = eval_to_bytes(&next()?)?;
                let prefix = eval_to_bytes(&next()?)?;
                EvalValue::Value(Value::Bool(input.starts_with(&prefix)))
            }
            PlanRowExpressionNode::BytesEndsWith { .. } => {
                let input = eval_to_bytes(&next()?)?;
                let suffix = eval_to_bytes(&next()?)?;
                EvalValue::Value(Value::Bool(input.ends_with(&suffix)))
            }
            PlanRowExpressionNode::BytesConcat { .. } => {
                let left = eval_to_bytes(&next()?)?;
                let right = eval_to_bytes(&next()?)?;
                let mut joined = Vec::with_capacity(left.len().saturating_add(right.len()));
                joined.extend_from_slice(&left);
                joined.extend_from_slice(&right);
                EvalValue::Value(Value::Bytes(joined.into()))
            }
            PlanRowExpressionNode::BytesEqual { .. } => EvalValue::Value(Value::Bool(
                eval_to_bytes(&next()?)? == eval_to_bytes(&next()?)?,
            )),
            PlanRowExpressionNode::NumberInfix { op, .. } => {
                let left = next()?;
                let right = next()?;
                EvalValue::Value(eval_number_infix(*op, &left, &right)?)
            }
            PlanRowExpressionNode::TextConcat { parts } => {
                let mut text = String::new();
                for _ in 0..parts.len() {
                    let value = next()?;
                    text.push_str(&eval_to_text(&value)?);
                }
                EvalValue::Value(Value::Text(text))
            }
            PlanRowExpressionNode::ListRange { .. } => {
                let from = eval_to_integer(&next()?)?;
                let to = eval_to_integer(&next()?)?;
                let values = if from <= to {
                    let length = to
                        .checked_sub(from)
                        .and_then(|span| span.checked_add(1))
                        .and_then(|length| u64::try_from(length).ok())
                        .unwrap_or(u64::MAX);
                    work.consume(length)?;
                    (from..=to)
                        .map(|value| Value::integer(value).map(EvalValue::Value))
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    Vec::new()
                };
                EvalValue::List(values)
            }
            PlanRowExpressionNode::ListLiteral { items } => {
                let mut values = Vec::with_capacity(items.len());
                for _ in 0..items.len() {
                    values.push(next()?);
                }
                EvalValue::List(values)
            }
            PlanRowExpressionNode::ListSum { .. } => {
                let items = eval_to_list(next()?)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                let mut total = 0.0_f64;
                for item in items {
                    if let Ok(value) = eval_to_number(&item) {
                        total += value.get();
                    }
                }
                EvalValue::Value(Value::Number(
                    FiniteReal::new(total)
                        .map_err(|_| Error::Evaluation("List/sum overflow".to_owned()))?,
                ))
            }
            PlanRowExpressionNode::Object { fields } => {
                let mut record = BTreeMap::new();
                for field in fields {
                    let value = next()?;
                    if field.spread {
                        self.extend_record_from_spread(
                            &mut record,
                            value,
                            context.event,
                            context.consumer,
                            work,
                        )?;
                    } else {
                        record.insert(field.name.clone(), value);
                    }
                }
                EvalValue::Record(record)
            }
            PlanRowExpressionNode::TaggedObject { tag, fields } => {
                let mut record = BTreeMap::from([(
                    "$tag".to_owned(),
                    EvalValue::Value(Value::Text(tag.clone())),
                )]);
                for field in fields {
                    let value = next()?;
                    if field.spread {
                        self.extend_record_from_spread(
                            &mut record,
                            value,
                            context.event,
                            context.consumer,
                            work,
                        )?;
                    } else {
                        record.insert(field.name.clone(), value);
                    }
                }
                EvalValue::Record(record)
            }
            PlanRowExpressionNode::ObjectField { field, .. } => {
                self.eval_object_field(next()?, field, context.consumer)?
            }
            PlanRowExpressionNode::Intrinsic { .. }
            | PlanRowExpressionNode::Field { .. }
            | PlanRowExpressionNode::Constant { .. }
            | PlanRowExpressionNode::ListGetField { .. }
            | PlanRowExpressionNode::ListRef { .. }
            | PlanRowExpressionNode::AuthorityListRef { .. }
            | PlanRowExpressionNode::ContextualCollection { .. }
            | PlanRowExpressionNode::ContextualOrder { .. }
            | PlanRowExpressionNode::ListAccess { .. }
            | PlanRowExpressionNode::ListPage { .. }
            | PlanRowExpressionNode::BoundedListPage { .. }
            | PlanRowExpressionNode::Local { .. }
            | PlanRowExpressionNode::LocalRow { .. }
            | PlanRowExpressionNode::EventRow { .. }
            | PlanRowExpressionNode::ListRowField { .. }
            | PlanRowExpressionNode::BuiltinCall { .. }
            | PlanRowExpressionNode::Select { .. } => {
                return Err(Error::InvalidPlan(format!(
                    "row expression {} reached an invalid apply continuation",
                    expression.0
                )));
            }
        };
        drop(next);
        if operands.len() != value_base {
            return Err(Error::InvalidPlan(format!(
                "row expression {} left unused evaluated operands",
                expression.0
            )));
        }
        Ok(value)
    }

    fn eval_contextual_body(
        &mut self,
        local: (PlanStaticOwnerId, PlanLocalId),
        value: EvalValue,
        body: impl IntoExpressionId,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let previous = bindings.insert(local, value);
        let result = self.eval_row_expression(body, row, event, output, consumer, bindings, work);
        match previous {
            Some(previous) => {
                bindings.insert(local, previous);
            }
            None => {
                bindings.remove(&local);
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_list_access_selection(
        &mut self,
        selection: &PlanListAccessSelection,
        index_plan: &PlanListIndex,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<(EvaluatedListAccessSelection, Vec<Value>), Error> {
        let mut tasks = vec![ListAccessSelectionTask::Evaluate(selection)];
        let mut values = Vec::<(EvaluatedListAccessSelection, Vec<Value>)>::new();
        while let Some(task) = tasks.pop() {
            match task {
                ListAccessSelectionTask::Evaluate(selection) => match selection {
                    PlanListAccessSelection::OrderedStart => {
                        values.push((EvaluatedListAccessSelection::OrderedStart, Vec::new()))
                    }
                    PlanListAccessSelection::KeyPrefix {
                        values: expressions,
                    } => {
                        let mut evaluated = Vec::with_capacity(expressions.len());
                        let mut captures = Vec::with_capacity(expressions.len());
                        for (position, expression) in expressions.iter().enumerate() {
                            let key = index_plan.keys.get(position).ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "typed list key prefix component {position} exceeds index {} arity",
                                    index_plan.id.0
                                ))
                            })?;
                            let value = self.eval_row_expression(
                                expression, row, event, output, consumer, bindings, work,
                            )?;
                            let value = self.materialize_eval(value)?;
                            evaluated.push(structural_index_value(key, value.clone())?);
                            captures.push(value);
                        }
                        values.push((
                            EvaluatedListAccessSelection::KeyPrefix { values: evaluated },
                            captures,
                        ));
                    }
                    PlanListAccessSelection::TextPrefix { leading, prefix } => {
                        let mut evaluated = Vec::with_capacity(leading.len());
                        let mut captures = Vec::with_capacity(leading.len().saturating_add(1));
                        for (position, expression) in leading.iter().enumerate() {
                            let key = index_plan.keys.get(position).ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "typed list Text prefix component {position} exceeds index {} arity",
                                    index_plan.id.0
                                ))
                            })?;
                            let value = self.eval_row_expression(
                                expression, row, event, output, consumer, bindings, work,
                            )?;
                            let value = self.materialize_eval(value)?;
                            evaluated.push(structural_index_value(key, value.clone())?);
                            captures.push(value);
                        }
                        let prefix = self.eval_row_expression(
                            prefix, row, event, output, consumer, bindings, work,
                        )?;
                        let prefix = eval_to_text(&prefix)?;
                        captures.push(Value::Text(prefix.clone()));
                        values.push((
                            EvaluatedListAccessSelection::TextPrefix {
                                leading: evaluated,
                                prefix,
                            },
                            captures,
                        ));
                    }
                    PlanListAccessSelection::ComponentRange {
                        leading,
                        lower,
                        upper,
                    } => {
                        let mut evaluated = Vec::with_capacity(leading.len());
                        let mut captures = Vec::with_capacity(
                            leading
                                .len()
                                .saturating_add(usize::from(lower.is_some()))
                                .saturating_add(usize::from(upper.is_some())),
                        );
                        for (position, expression) in leading.iter().enumerate() {
                            let key = index_plan.keys.get(position).ok_or_else(|| {
                                Error::InvalidPlan(format!(
                                    "typed list range component {position} exceeds index {} arity",
                                    index_plan.id.0
                                ))
                            })?;
                            let value = self.eval_row_expression(
                                expression, row, event, output, consumer, bindings, work,
                            )?;
                            let value = self.materialize_eval(value)?;
                            evaluated.push(structural_index_value(key, value.clone())?);
                            captures.push(value);
                        }
                        let target = index_plan.keys.get(leading.len()).ok_or_else(|| {
                            Error::InvalidPlan(format!(
                                "typed list range has no component {} in index {}",
                                leading.len(),
                                index_plan.id.0
                            ))
                        })?;
                        let mut evaluate_bound = |bound: &boon_plan::PlanListAccessBound| {
                            let value = self.eval_row_expression(
                                &bound.value,
                                row,
                                event,
                                output,
                                consumer,
                                bindings,
                                work,
                            )?;
                            let value = self.materialize_eval(value)?;
                            let structural = structural_index_value(target, value.clone())?;
                            captures.push(value);
                            Ok::<_, Error>((structural, bound.inclusive))
                        };
                        let lower = lower.as_ref().map(&mut evaluate_bound).transpose()?;
                        let upper = upper.as_ref().map(&mut evaluate_bound).transpose()?;
                        values.push((
                            EvaluatedListAccessSelection::ComponentRange {
                                leading: evaluated,
                                lower,
                                upper,
                            },
                            captures,
                        ));
                    }
                    PlanListAccessSelection::Union { branches }
                    | PlanListAccessSelection::Intersection { branches } => {
                        let additional = branches.len().saturating_add(1);
                        if tasks.len().saturating_add(additional)
                            > MAX_LIST_ACCESS_SELECTION_CONTINUATIONS
                        {
                            return Err(Error::InvalidPlan(format!(
                                "list access selection continuation stack exceeded its checked bound of {}",
                                MAX_LIST_ACCESS_SELECTION_CONTINUATIONS
                            )));
                        }
                        let kind = match selection {
                            PlanListAccessSelection::Union { .. } => {
                                ListAccessSelectionBranchKind::Union
                            }
                            PlanListAccessSelection::Intersection { .. } => {
                                ListAccessSelectionBranchKind::Intersection
                            }
                            _ => unreachable!(),
                        };
                        tasks.push(ListAccessSelectionTask::FinishBranches {
                            kind,
                            value_base: values.len(),
                            branch_count: branches.len(),
                        });
                        for branch in branches.iter().rev() {
                            tasks.push(ListAccessSelectionTask::Evaluate(branch));
                        }
                    }
                },
                ListAccessSelectionTask::FinishBranches {
                    kind,
                    value_base,
                    branch_count,
                } => {
                    if values.len() != value_base.saturating_add(branch_count) {
                        return Err(Error::InvalidPlan(
                            "list access selection branch continuation produced an invalid value count"
                                .to_owned(),
                        ));
                    }
                    let mut branches = Vec::with_capacity(branch_count);
                    let mut captures = Vec::new();
                    for (branch, branch_captures) in values.drain(value_base..) {
                        branches.push(branch);
                        captures.extend(branch_captures);
                    }
                    let selection = match kind {
                        ListAccessSelectionBranchKind::Union => {
                            EvaluatedListAccessSelection::Union { branches }
                        }
                        ListAccessSelectionBranchKind::Intersection => {
                            EvaluatedListAccessSelection::Intersection { branches }
                        }
                    };
                    values.push((selection, captures));
                }
            }
        }
        if values.len() != 1 {
            return Err(Error::InvalidPlan(format!(
                "list access selection evaluation completed with {} values",
                values.len()
            )));
        }
        values.pop().ok_or_else(|| {
            Error::InvalidPlan("list access selection evaluation produced no root value".to_owned())
        })
    }
    fn evaluate_bounded_list_page(
        &mut self,
        page: &PlanBoundedListPage,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        match self
            .evaluate_bounded_list_page_inner(page, row, event, output, consumer, bindings, work)
        {
            Err(Error::WorkBudgetExceeded { .. }) => {
                work.metrics.access_work_limit_failure_count = work
                    .metrics
                    .access_work_limit_failure_count
                    .saturating_add(1);
                Ok(page_terminal_variant("PageWorkLimitExceeded"))
            }
            result => result,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_bounded_list_page_inner(
        &mut self,
        page: &PlanBoundedListPage,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let size =
            self.eval_row_expression(&page.size, row, event, output, consumer, bindings, work)?;
        let Ok(size) = eval_to_integer(&size) else {
            return Ok(page_terminal_variant("InvalidPageSize"));
        };
        if !(1..=10_000).contains(&size) {
            return Ok(page_terminal_variant("InvalidPageSize"));
        }
        let size = usize::try_from(size).expect("positive page size fits usize");

        let view =
            self.eval_row_expression(&page.view, row, event, output, consumer, bindings, work)?;
        let evaluated = eval_to_list(view)?;
        if evaluated.len() > page.max_items as usize {
            work.metrics.access_work_limit_failure_count = work
                .metrics
                .access_work_limit_failure_count
                .saturating_add(1);
            return Ok(page_terminal_variant("PageWorkLimitExceeded"));
        }
        work.metrics.bounded_page_scan_count =
            work.metrics.bounded_page_scan_count.saturating_add(1);
        work.metrics.bounded_page_candidate_count = work
            .metrics
            .bounded_page_candidate_count
            .saturating_add(u64::try_from(evaluated.len()).unwrap_or(u64::MAX));
        work.consume(evaluated.len().try_into().unwrap_or(u64::MAX))?;
        let items = evaluated
            .into_iter()
            .map(|value| self.materialize_page_value(value, event, work))
            .collect::<Result<Vec<_>, _>>()?;

        let authority_revision = self.bounded_page_authority_revision(page.view)?;
        let mut captures =
            self.evaluate_bounded_page_captures(page.view, row, event, output, consumer, work)?;
        captures.push(Value::List(items.clone()));
        let owner_scope = row
            .map(|row| {
                self.row_owner_ancestors(row).map(|owners| {
                    owners
                        .iter()
                        .map(|owner| RowId {
                            list: owner.list,
                            key: owner.key,
                            generation: owner.generation,
                        })
                        .collect::<Vec<_>>()
                })
            })
            .transpose()?
            .unwrap_or_default();
        let principal_scope = self.eval_intrinsic(PlanIntrinsic::SessionInfoPrincipal);
        let capture_fingerprint = capture_fingerprint(
            page.view_fingerprint,
            self.cursor_ephemeral_launch_epoch,
            self.options.cursor_scope_fingerprint.as_ref(),
            &owner_scope,
            &principal_scope,
            captures.iter(),
            self,
        )
        .map_err(|_| {
            Error::InvalidPlan(
                "bounded page cursor capture has no canonical memory identity".to_owned(),
            )
        })?;

        let after =
            self.eval_row_expression(&page.after, row, event, output, consumer, bindings, work)?;
        let after = self.materialize_eval(after)?;
        let accepted_offset = match page_position_bytes(&after) {
            Ok(None) => 0,
            Ok(Some(bytes)) => {
                let cursor = match open_cursor(&self.cursor_sealing_key, &bytes) {
                    Ok(cursor) => cursor,
                    Err(_) => return Ok(page_terminal_variant("InvalidPageCursor")),
                };
                if cursor.view_fingerprint != page.view_fingerprint
                    || cursor.capture_fingerprint != capture_fingerprint
                {
                    return Ok(page_terminal_variant("InvalidPageCursor"));
                }
                if cursor.authority_revision != authority_revision {
                    return Ok(page_terminal_variant("PageExpired"));
                }
                if !cursor.semantic_key.parts().is_empty()
                    || cursor.accepted_offset == 0
                    || cursor.accepted_offset > items.len() as u64
                {
                    return Ok(page_terminal_variant("InvalidPageCursor"));
                }
                let previous = usize::try_from(cursor.accepted_offset - 1)
                    .map_err(|_| Error::Evaluation("page cursor offset overflow".to_owned()))?;
                let (source_order, row_id) =
                    bounded_page_position(cursor.accepted_offset, &items[previous], self).map_err(
                        |_| {
                            Error::InvalidPlan(
                                "bounded page continuation has no canonical memory identity"
                                    .to_owned(),
                            )
                        },
                    )?;
                if cursor.source_order != source_order || cursor.row_id != row_id {
                    return Ok(page_terminal_variant("InvalidPageCursor"));
                }
                cursor.accepted_offset
            }
            Err(()) => return Ok(page_terminal_variant("InvalidPageCursor")),
        };
        let offset = usize::try_from(accepted_offset)
            .map_err(|_| Error::Evaluation("page cursor offset overflow".to_owned()))?;
        let end = offset.saturating_add(size).min(items.len());
        let page_items = items[offset..end].to_vec();
        let next = if end < items.len() {
            let accepted_offset = u64::try_from(end).unwrap_or(u64::MAX);
            let (source_order, row_id) =
                bounded_page_position(accepted_offset, &items[end - 1], self).map_err(|_| {
                    Error::InvalidPlan(
                        "bounded page result has no canonical memory identity".to_owned(),
                    )
                })?;
            let cursor = PageCursor {
                view_fingerprint: page.view_fingerprint,
                authority_revision,
                capture_fingerprint,
                accepted_offset,
                semantic_key: StructuralKey::new(Vec::new())
                    .expect("empty bounded page key is valid"),
                source_order,
                row_id,
            };
            match seal_cursor(&self.cursor_sealing_key, &cursor) {
                Ok(cursor) => Some(cursor),
                Err(CursorError::TooLarge) => {
                    return Ok(page_terminal_variant("PageWorkLimitExceeded"));
                }
                Err(CursorError::Invalid | CursorError::Randomness) => {
                    return Err(Error::Evaluation(
                        "failed to seal bounded typed list page cursor".to_owned(),
                    ));
                }
            }
        } else {
            None
        };
        work.metrics.access_result_count = work
            .metrics
            .access_result_count
            .saturating_add(u64::try_from(page_items.len()).unwrap_or(u64::MAX));
        Ok(page_result(page_items, next))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_bounded_page_captures(
        &mut self,
        view: PlanRowExpressionId,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<Vec<Value>, Error> {
        let mut inputs = BTreeSet::new();
        self.plan
            .row_expressions
            .visit_value_refs(view, &mut |input| {
                let row_field = matches!(
                    input,
                    ValueRef::Field(field)
                        if self.metadata.row_field_owner.contains_key(field)
                );
                if !row_field && !matches!(input, ValueRef::Constant(_) | ValueRef::List(_)) {
                    inputs.insert(input.clone());
                }
            })
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        let mut captures = Vec::with_capacity(inputs.len());
        for input in inputs {
            let value = self.eval_value_ref(&input, row, event, output, consumer, work)?;
            captures.push(self.materialize_eval(value)?);
        }
        Ok(captures)
    }

    fn bounded_page_authority_revision(&self, view: PlanRowExpressionId) -> Result<u64, Error> {
        let mut lists = BTreeSet::new();
        self.plan
            .row_expressions
            .visit_value_refs(view, &mut |input| {
                if let ValueRef::List(list) = input {
                    lists.insert(*list);
                }
            })
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        let mut indexes = BTreeSet::new();
        collect_list_access_indexes(&self.plan.row_expressions, view, &mut indexes)?;
        for index in indexes {
            let source = self
                .metadata
                .list_indexes
                .get(&index)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "bounded typed list page references missing index {}",
                        index.0
                    ))
                })?
                .source_list;
            lists.insert(source);
        }
        let mut authorities = Vec::with_capacity(lists.len());
        for list in lists {
            let revision = self
                .lists
                .get(&list)
                .map(|list| list.revision)
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "bounded typed list page source list {} is missing",
                        list.0
                    ))
                })?;
            let (memory_id, type_fingerprint) = self
                .metadata
                .semantic_list_identities
                .get(&list)
                .copied()
                .ok_or_else(|| {
                    Error::InvalidPlan(format!(
                        "bounded typed list page source list {} has no canonical memory identity",
                        list.0
                    ))
                })?;
            authorities.push((memory_id, type_fingerprint, revision));
        }
        authorities.sort();
        let mut hasher = Sha256::new();
        hasher.update(b"boon.bounded-page-authority.v2\0");
        hasher.update((authorities.len() as u64).to_be_bytes());
        for (memory_id, type_fingerprint, revision) in authorities {
            hasher.update(memory_id);
            hasher.update(type_fingerprint);
            hasher.update(revision.to_be_bytes());
        }
        let digest: [u8; 32] = hasher.finalize().into();
        Ok(u64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 prefix has eight bytes"),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_list_page(
        &mut self,
        page: &PlanListPage,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        match self.evaluate_list_page_inner(page, row, event, output, consumer, bindings, work) {
            Err(Error::WorkBudgetExceeded { .. }) => {
                work.metrics.access_work_limit_failure_count = work
                    .metrics
                    .access_work_limit_failure_count
                    .saturating_add(1);
                Ok(page_terminal_variant("PageWorkLimitExceeded"))
            }
            result => result,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_list_page_inner(
        &mut self,
        page: &PlanListPage,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let metadata = Arc::clone(&self.metadata);
        let index_plan = metadata
            .list_indexes
            .get(&page.access.index)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list page references missing index {}",
                    page.access.index.0
                ))
            })?;

        let size = self.eval_row_expression(
            &page.access.limit,
            row,
            event,
            output,
            consumer,
            bindings,
            work,
        )?;
        let Ok(size) = eval_to_integer(&size) else {
            return Ok(page_terminal_variant("InvalidPageSize"));
        };
        if !(1..=10_000).contains(&size) {
            return Ok(page_terminal_variant("InvalidPageSize"));
        }
        let size = usize::try_from(size).expect("positive page size fits usize");

        let (view_limit, view_limit_capture) = match &page.view_limit {
            Some(limit) => {
                let value =
                    self.eval_row_expression(limit, row, event, output, consumer, bindings, work)?;
                let count = u64::try_from(eval_to_integer(&value)?).map_err(|_| {
                    Error::Evaluation(
                        "List/take count before List/page must be a non-negative integer"
                            .to_owned(),
                    )
                })?;
                (count, Some(self.materialize_eval(value)?))
            }
            None => (u64::MAX, None),
        };

        let guard_matches = match &page.access.guard {
            Some(guard) => {
                let value =
                    self.eval_row_expression(guard, row, event, output, consumer, bindings, work)?;
                eval_to_bool(&value)?
            }
            None => true,
        };
        let (selection, selection_captures) = self.evaluate_list_access_selection(
            &page.access.selection,
            index_plan,
            row,
            event,
            output,
            consumer,
            bindings,
            work,
        )?;

        self.register_list_dependency(consumer, index_plan.source_list);
        self.register_list_access_dependency(consumer, page.access.index, &selection);
        let mut selection_expressions = Vec::new();
        page.access
            .selection
            .visit_expressions(&mut |expression| selection_expressions.push(expression));
        if selection_expressions.len() != selection_captures.len() {
            return Err(Error::InvalidPlan(format!(
                "typed list page selection produced {} captures for {} expressions",
                selection_captures.len(),
                selection_expressions.len()
            )));
        }
        let mut captured_filter_expressions = Vec::new();
        let mut captures = Vec::new();
        for (expression, value) in selection_expressions.into_iter().zip(selection_captures) {
            if page
                .access
                .filters
                .iter()
                .try_fold(false, |found, filter| {
                    if found {
                        Ok(true)
                    } else {
                        row_expression_contains(
                            &self.plan.row_expressions,
                            filter.predicate,
                            expression,
                        )
                    }
                })?
            {
                captured_filter_expressions.push(expression);
                captures.push(value);
            }
        }
        captures.extend(self.evaluate_page_captures(
            page,
            index_plan,
            &captured_filter_expressions,
            row,
            event,
            output,
            consumer,
            bindings,
            work,
        )?);
        if let Some(view_limit) = view_limit_capture {
            captures.push(view_limit);
        }
        let owner_scope = row
            .map(|row| {
                self.row_owner_ancestors(row).map(|owners| {
                    owners
                        .iter()
                        .map(|owner| RowId {
                            list: owner.list,
                            key: owner.key,
                            generation: owner.generation,
                        })
                        .collect::<Vec<_>>()
                })
            })
            .transpose()?
            .unwrap_or_default();
        let principal_scope = self.eval_intrinsic(PlanIntrinsic::SessionInfoPrincipal);
        let capture_fingerprint = capture_fingerprint(
            page.view_fingerprint,
            self.cursor_ephemeral_launch_epoch,
            self.options.cursor_scope_fingerprint.as_ref(),
            &owner_scope,
            &principal_scope,
            captures.iter(),
            self,
        )
        .map_err(|_| {
            Error::InvalidPlan(
                "typed list page cursor capture has no canonical memory identity".to_owned(),
            )
        })?;
        let authority_revision = self
            .lists
            .get(&index_plan.source_list)
            .map(|list| list.revision)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list page source list {} is missing",
                    index_plan.source_list.0
                ))
            })?;

        let after =
            self.eval_row_expression(&page.after, row, event, output, consumer, bindings, work)?;
        let after = self.materialize_eval(after)?;
        let (cursor_payload, accepted_offset) = match page_position_bytes(&after) {
            Ok(None) => (None, 0),
            Ok(Some(bytes)) => {
                let cursor = match open_cursor(&self.cursor_sealing_key, &bytes) {
                    Ok(cursor) => cursor,
                    Err(_) => return Ok(page_terminal_variant("InvalidPageCursor")),
                };
                if cursor.view_fingerprint != page.view_fingerprint
                    || cursor.capture_fingerprint != capture_fingerprint
                {
                    return Ok(page_terminal_variant("InvalidPageCursor"));
                }
                if cursor.authority_revision != authority_revision {
                    return Ok(page_terminal_variant("PageExpired"));
                }
                let accepted_offset = cursor.accepted_offset;
                (Some(cursor), accepted_offset)
            }
            Err(()) => return Ok(page_terminal_variant("InvalidPageCursor")),
        };
        if accepted_offset > view_limit {
            return Ok(page_terminal_variant("InvalidPageCursor"));
        }
        if !guard_matches || accepted_offset == view_limit {
            return Ok(page_result(Vec::new(), None));
        }

        let remaining = view_limit.saturating_sub(accepted_offset);
        let requested = u64::try_from(size)
            .unwrap_or(u64::MAX)
            .saturating_add(1)
            .min(remaining);
        let requested = usize::try_from(requested).unwrap_or(usize::MAX);
        self.ensure_ordered_index_current(page.access.index, consumer, work)?;
        let after_cursor = match cursor_payload {
            Some(payload) => {
                let Some(index) = self.ordered_indexes.get(&page.access.index) else {
                    return Ok(page_terminal_variant("InvalidPageCursor"));
                };
                let semantic_components = page.access.semantic_order.len();
                let matching_current_key =
                    index
                        .cursor_keys_for(payload.row_id)
                        .into_iter()
                        .any(|cursor| {
                            cursor.source_order() == payload.source_order
                                && semantic_page_cursor_key(&cursor, semantic_components)
                                    .is_ok_and(|key| key == payload.semantic_key)
                        });
                if !matching_current_key {
                    return Ok(page_terminal_variant("InvalidPageCursor"));
                }
                Some(AccessCursorKey::new(
                    payload.semantic_key,
                    payload.source_order,
                    payload.row_id,
                ))
            }
            None => None,
        };
        let index = self
            .ordered_indexes
            .remove(&page.access.index)
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list page index {} is not current",
                    page.access.index.0
                ))
            })?;
        let limits = self.options.list_access_work_limits;
        let mut tracker = AccessWorkTracker::new(AccessWorkLimits::new(
            limits.max_index_seeks,
            limits.max_key_ranges,
            limits.max_keys_visited,
            limits.max_candidates_visited,
            limits.max_rows_returned,
            limits.max_branch_polls,
            0,
        ));
        let traversal = (|| {
            let mut stream = open_evaluated_list_access(
                &index,
                &selection,
                after_cursor.as_ref(),
                page.access.semantic_order.len(),
            )
            .map_err(PageTraversalFailure::Access)?;
            let mut accepted = Vec::with_capacity(requested.min(size.saturating_add(1)));
            while accepted.len() < requested {
                let candidate = match stream.next(&mut tracker) {
                    Ok(Some(candidate)) => candidate,
                    Ok(None) => break,
                    Err(AccessError::WorkLimitExceeded(_)) => {
                        return Err(PageTraversalFailure::WorkLimit);
                    }
                    Err(error) => return Err(PageTraversalFailure::Access(error)),
                };
                work.consume(1).map_err(PageTraversalFailure::Runtime)?;
                let candidate_row = runtime_row_id(index_plan.source_list, candidate.row_id());
                if !self.row_exists(candidate_row) {
                    return Err(PageTraversalFailure::Runtime(Error::InvalidPlan(format!(
                        "typed list page on index {} returned stale row {}:{}:{}",
                        page.access.index.0,
                        candidate_row.list.0,
                        candidate_row.key,
                        candidate_row.generation
                    ))));
                }
                let mut matches_all = true;
                for filter in &page.access.filters {
                    let matches = self
                        .eval_contextual_body(
                            (filter.owner, filter.row_local),
                            EvalValue::Row(candidate_row),
                            &filter.predicate,
                            Some(candidate_row),
                            event,
                            output,
                            consumer,
                            bindings,
                            work,
                        )
                        .map_err(PageTraversalFailure::Runtime)?;
                    if !eval_to_bool(&matches).map_err(PageTraversalFailure::Runtime)? {
                        matches_all = false;
                        break;
                    }
                }
                if !matches_all {
                    continue;
                }
                accepted.push((candidate_row, candidate.cursor_key()));
            }
            Ok(accepted)
        })();
        let metrics = tracker.metrics();
        self.ordered_indexes.insert(page.access.index, index);
        let mut accepted = match traversal {
            Ok(accepted) => accepted,
            Err(PageTraversalFailure::WorkLimit) => {
                record_access_metrics(work, metrics, 0);
                return Ok(page_terminal_variant("PageWorkLimitExceeded"));
            }
            Err(PageTraversalFailure::Runtime(error)) => return Err(error),
            Err(PageTraversalFailure::Access(error)) => {
                return Err(Error::Evaluation(error.to_string()));
            }
        };
        let has_more = accepted.len() > size;
        if has_more {
            accepted.truncate(size);
        }
        record_access_metrics(work, metrics, accepted.len());
        let next = if has_more {
            let (_, position) = accepted.last().ok_or_else(|| {
                Error::InvalidPlan("non-empty page lost its continuation position".to_owned())
            })?;
            let cursor = PageCursor {
                view_fingerprint: page.view_fingerprint,
                authority_revision,
                capture_fingerprint,
                accepted_offset: accepted_offset
                    .saturating_add(u64::try_from(accepted.len()).unwrap_or(u64::MAX)),
                semantic_key: semantic_page_cursor_key(position, page.access.semantic_order.len())?,
                source_order: position.source_order(),
                row_id: position.row_id(),
            };
            match seal_cursor(&self.cursor_sealing_key, &cursor) {
                Ok(cursor) => Some(cursor),
                Err(CursorError::TooLarge) => {
                    return Ok(page_terminal_variant("PageWorkLimitExceeded"));
                }
                Err(CursorError::Invalid | CursorError::Randomness) => {
                    return Err(Error::Evaluation(
                        "failed to seal typed list page cursor".to_owned(),
                    ));
                }
            }
        } else {
            None
        };
        let mut items = Vec::with_capacity(accepted.len());
        for (row, _) in accepted {
            let value = if page.access.maps.is_empty() {
                EvalValue::Row(row)
            } else {
                self.evaluate_list_maps(
                    &page.access.maps,
                    EvalValue::Row(row),
                    row,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?
            };
            items.push(self.materialize_page_value(value, event, work)?);
        }
        Ok(page_result(items, next))
    }

    fn materialize_page_value(
        &mut self,
        value: EvalValue,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let value = self.materialize_eval(value)?;
        self.normalize_page_value(value, event, work)
    }

    fn normalize_page_value(
        &mut self,
        value: Value,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let mut tasks = vec![PageValueTask::Evaluate(value)];
        let mut values = Vec::<Value>::new();
        while let Some(task) = tasks.pop() {
            match task {
                PageValueTask::Evaluate(value) => match value {
                    Value::Row { id, .. } => {
                        values.push(self.materialize_page_row(id.list, id, event, work)?);
                    }
                    Value::MappedRow { fields, .. } | Value::Record(fields) => {
                        ensure_value_continuation_capacity(&tasks, 1, "page value normalization")?;
                        tasks.push(PageValueTask::Continue(PageValueCollection::Record {
                            remaining: fields.into_iter(),
                            output: BTreeMap::new(),
                        }));
                    }
                    Value::List(items) => {
                        let capacity = items.len();
                        ensure_value_continuation_capacity(&tasks, 1, "page value normalization")?;
                        tasks.push(PageValueTask::Continue(PageValueCollection::List {
                            remaining: items.into_iter(),
                            output: Vec::with_capacity(capacity),
                        }));
                    }
                    value => values.push(value),
                },
                PageValueTask::Continue(mut collection) => {
                    let next = match &mut collection {
                        PageValueCollection::List { remaining, .. } => remaining
                            .next()
                            .map(|value| (EvalMaterializationSlot::Item, value)),
                        PageValueCollection::Record { remaining, .. } => remaining
                            .next()
                            .map(|(name, value)| (EvalMaterializationSlot::Field(name), value)),
                    };
                    if let Some((slot, value)) = next {
                        ensure_value_continuation_capacity(&tasks, 2, "page value normalization")?;
                        tasks.push(PageValueTask::Append { collection, slot });
                        tasks.push(PageValueTask::Evaluate(value));
                    } else {
                        let value = match collection {
                            PageValueCollection::List { output, .. } => Value::List(output),
                            PageValueCollection::Record { output, .. } => Value::Record(output),
                        };
                        values.push(value);
                    }
                }
                PageValueTask::Append {
                    mut collection,
                    slot,
                } => {
                    let value = values.pop().ok_or_else(|| {
                        Error::InvalidPlan(
                            "page value normalization produced no child value".to_owned(),
                        )
                    })?;
                    match (&mut collection, slot) {
                        (
                            PageValueCollection::List { output, .. },
                            EvalMaterializationSlot::Item,
                        ) => output.push(value),
                        (
                            PageValueCollection::Record { output, .. },
                            EvalMaterializationSlot::Field(name),
                        ) => {
                            output.insert(name, value);
                        }
                        _ => {
                            return Err(Error::InvalidPlan(
                                "page value normalization continuation type mismatch".to_owned(),
                            ));
                        }
                    }
                    ensure_value_continuation_capacity(&tasks, 1, "page value normalization")?;
                    tasks.push(PageValueTask::Continue(collection));
                }
            }
        }
        if values.len() != 1 {
            return Err(Error::InvalidPlan(format!(
                "page value normalization completed with {} values",
                values.len()
            )));
        }
        values.pop().ok_or_else(|| {
            Error::InvalidPlan("page value normalization produced no root value".to_owned())
        })
    }

    fn materialize_page_row(
        &mut self,
        list: ListId,
        row: RowId,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Value, Error> {
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list page source list {} has no storage slot",
                    list.0
                ))
            })?;
        let item_type = self
            .plan
            .persistence
            .lists
            .iter()
            .find(|memory| memory.runtime_slot == slot.id)
            .and_then(|memory| match &memory.data_type {
                boon_plan::DataTypePlan::List { item } => Some(item.as_ref().clone()),
                _ => None,
            })
            .ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "typed list page source list {} has no declared item type",
                    list.0
                ))
            })?;
        let output_fields = slot
            .row_fields
            .iter()
            .map(|field| OutputListFieldRef {
                list_id: list,
                name: field.name.clone(),
                field_id: field.field_id,
            })
            .collect::<Vec<_>>();
        if matches!(
            item_type,
            boon_plan::DataTypePlan::Record { open: false, .. }
        ) {
            return self.materialize_typed_list_item(
                EvalValue::Row(row),
                &item_type,
                &output_fields,
                event,
                None,
                work,
            );
        }
        let value_field = output_list_field(&output_fields, list, "value")?;
        let value = if self.metadata.row_computations.contains_key(&value_field) {
            self.ensure_row_field(row, value_field, event, work)?
        } else {
            self.row_value(row, value_field)?
        };
        normalize_scalar_list_item(value, &item_type)
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_page_captures(
        &mut self,
        page: &PlanListPage,
        index_plan: &PlanListIndex,
        captured_filter_expressions: &[PlanRowExpressionId],
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<Vec<Value>, Error> {
        let mut inputs = BTreeSet::new();
        let mut intrinsics = BTreeSet::new();
        for filter in &page.access.filters {
            collect_uncaptured_page_dependencies(
                &self.plan.row_expressions,
                filter.predicate,
                captured_filter_expressions,
                index_plan.source_list,
                &mut inputs,
                &mut intrinsics,
            )?;
        }
        for map in &page.access.maps {
            collect_uncaptured_page_dependencies(
                &self.plan.row_expressions,
                map.body,
                captured_filter_expressions,
                index_plan.source_list,
                &mut inputs,
                &mut intrinsics,
            )?;
            for capture in &map.captures {
                collect_uncaptured_page_dependencies(
                    &self.plan.row_expressions,
                    capture.value,
                    captured_filter_expressions,
                    index_plan.source_list,
                    &mut inputs,
                    &mut intrinsics,
                )?;
            }
        }

        let mut captures = Vec::with_capacity(inputs.len() + bindings.len());
        for input in inputs {
            let value = self.eval_value_ref(&input, row, event, output, consumer, work)?;
            captures.push(self.materialize_eval(value)?);
        }
        for intrinsic in intrinsics {
            captures.push(self.eval_intrinsic(intrinsic));
        }
        for ((owner, local), value) in bindings.iter() {
            let referenced = page
                .access
                .filters
                .iter()
                .map(|filter| filter.predicate)
                .chain(page.access.maps.iter().flat_map(|map| {
                    std::iter::once(map.body)
                        .chain(map.captures.iter().map(|capture| capture.value))
                }))
                .try_fold(false, |found, expression| {
                    if found {
                        Ok(true)
                    } else {
                        uncaptured_expression_references_local(
                            &self.plan.row_expressions,
                            expression,
                            captured_filter_expressions,
                            *owner,
                            *local,
                        )
                    }
                })?;
            if referenced {
                captures.push(self.materialize_eval(value.clone())?);
            }
        }
        Ok(captures)
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_list_maps(
        &mut self,
        maps: &[PlanListMap],
        mut value: EvalValue,
        source_row: RowId,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        for map in maps {
            let local = (map.owner, map.row_local);
            let origin = eval_row_id(&value);
            let mapped = self.eval_contextual_body(
                local,
                value.clone(),
                &map.body,
                Some(source_row),
                event,
                output,
                consumer,
                bindings,
                work,
            )?;
            let mut evaluated_captures = BTreeMap::new();
            for capture in &map.captures {
                if !self.metadata.capture_fields.contains(&capture.field) {
                    return Err(Error::InvalidPlan(format!(
                        "typed list map owner {} writes non-capture field {}",
                        map.owner.0, capture.field.0
                    )));
                }
                let captured = self.eval_contextual_body(
                    local,
                    value.clone(),
                    &capture.value,
                    Some(source_row),
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?;
                if evaluated_captures.insert(capture.field, captured).is_some() {
                    return Err(Error::InvalidPlan(format!(
                        "typed list map owner {} writes capture field {} more than once",
                        map.owner.0, capture.field.0
                    )));
                }
            }
            value = match (origin, mapped, evaluated_captures) {
                (Some(id), EvalValue::Record(fields), captures) => EvalValue::MappedRow {
                    id,
                    fields,
                    captures,
                },
                (_, value, captures) if captures.is_empty() => value,
                _ => {
                    return Err(Error::InvalidPlan(format!(
                        "typed list map owner {} captures state without a typed source row and record result",
                        map.owner.0
                    )));
                }
            };
        }
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_list_access(
        &mut self,
        access: &PlanListAccess,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        output: Option<FieldId>,
        consumer: Option<Consumer>,
        bindings: &mut PlanLocalBindings,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let metadata = Arc::clone(&self.metadata);
        let index_plan = metadata.list_indexes.get(&access.index).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "typed list access references missing index {}",
                access.index.0
            ))
        })?;
        if let Some(guard) = &access.guard {
            let guard =
                self.eval_row_expression(guard, row, event, output, consumer, bindings, work)?;
            if !eval_to_bool(&guard)? {
                return Ok(EvalValue::List(Vec::new()));
            }
        }
        let exhaustive_candidate_limit = access
            .exhaustive_candidate_limit
            .map(|limit| limit as usize);
        let limit = if let Some(limit) = exhaustive_candidate_limit {
            limit.saturating_add(1)
        } else {
            let limit = self.eval_row_expression(
                &access.limit,
                row,
                event,
                output,
                consumer,
                bindings,
                work,
            )?;
            usize::try_from(eval_to_integer(&limit)?).map_err(|_| {
                Error::Evaluation("List/take count must be a non-negative integer".to_owned())
            })?
        };
        if limit == 0 {
            return Ok(EvalValue::List(Vec::new()));
        }
        let (selection, _) = self.evaluate_list_access_selection(
            &access.selection,
            index_plan,
            row,
            event,
            output,
            consumer,
            bindings,
            work,
        )?;
        self.register_list_access_dependency(consumer, access.index, &selection);
        self.ensure_ordered_index_current(access.index, consumer, work)?;
        let index = self.ordered_indexes.remove(&access.index).ok_or_else(|| {
            Error::InvalidPlan(format!(
                "typed list access index {} is not current",
                access.index.0
            ))
        })?;
        let limits = self.options.list_access_work_limits;
        let mut tracker = AccessWorkTracker::new(AccessWorkLimits::new(
            limits.max_index_seeks,
            limits.max_key_ranges,
            limits.max_keys_visited,
            limits.max_candidates_visited,
            limits.max_rows_returned,
            limits.max_branch_polls,
            0,
        ));
        let result = (|| {
            let mut stream =
                open_evaluated_list_access(&index, &selection, None, access.semantic_order.len())
                    .map_err(|error| Error::Evaluation(error.to_string()))?;
            let mut result = Vec::with_capacity(limit);
            while result.len() < limit {
                let Some(candidate) = stream
                    .next(&mut tracker)
                    .map_err(|error| Error::Evaluation(error.to_string()))?
                else {
                    break;
                };
                work.consume(1)?;
                let candidate = runtime_row_id(index_plan.source_list, candidate.row_id());
                if !self.row_exists(candidate) {
                    return Err(Error::InvalidPlan(format!(
                        "typed list access on index {} returned stale row {}:{}:{}",
                        access.index.0, candidate.list.0, candidate.key, candidate.generation
                    )));
                }
                let mut matches_all = true;
                for filter in &access.filters {
                    let matches = self.eval_contextual_body(
                        (filter.owner, filter.row_local),
                        EvalValue::Row(candidate),
                        &filter.predicate,
                        Some(candidate),
                        event,
                        output,
                        consumer,
                        bindings,
                        work,
                    )?;
                    if !eval_to_bool(&matches)? {
                        matches_all = false;
                        break;
                    }
                }
                if !matches_all {
                    continue;
                }
                result.push(self.evaluate_list_maps(
                    &access.maps,
                    EvalValue::Row(candidate),
                    candidate,
                    event,
                    output,
                    consumer,
                    bindings,
                    work,
                )?);
            }
            if exhaustive_candidate_limit.is_some_and(|limit| result.len() > limit) {
                return Err(Error::WorkBudgetExceeded {
                    limit: exhaustive_candidate_limit.unwrap_or_default() as u64,
                    attempted: result.len() as u64,
                });
            }
            Ok(EvalValue::List(result))
        })();
        let metrics = tracker.metrics();
        self.ordered_indexes.insert(access.index, index);
        let result_count = match &result {
            Ok(EvalValue::List(items)) => items.len(),
            _ => 0,
        };
        record_access_metrics(work, metrics, result_count);
        result
    }

    fn eval_intrinsic(&self, intrinsic: PlanIntrinsic) -> Value {
        let SessionContext::Available { status, principal } = &self.options.session_context else {
            return Value::Error {
                code: "session_scope_unavailable".to_owned(),
            };
        };
        match intrinsic {
            PlanIntrinsic::SessionInfoStatus => match status {
                SessionConnectionStatus::Connecting => Value::Text("Connecting".to_owned()),
                SessionConnectionStatus::Current => Value::Text("Current".to_owned()),
                SessionConnectionStatus::Stale => Value::Text("Stale".to_owned()),
                SessionConnectionStatus::Failed { code } => Value::Record(BTreeMap::from([
                    ("$tag".to_owned(), Value::Text("Failed".to_owned())),
                    ("code".to_owned(), Value::Text(code.clone())),
                ])),
            },
            PlanIntrinsic::SessionInfoPrincipal => match principal {
                SessionPrincipal::Authenticated { subject, roles } => {
                    Value::Record(BTreeMap::from([
                        ("$tag".to_owned(), Value::Text("Authenticated".to_owned())),
                        ("subject".to_owned(), Value::Text(subject.clone())),
                        (
                            "roles".to_owned(),
                            Value::List(roles.iter().cloned().map(Value::Text).collect()),
                        ),
                    ]))
                }
                SessionPrincipal::Anonymous => Value::Text("Anonymous".to_owned()),
            },
        }
    }

    fn eval_object_field(
        &mut self,
        object: EvalValue,
        field: &str,
        consumer: Option<Consumer>,
    ) -> Result<EvalValue, Error> {
        let missing = |keys: Vec<String>| {
            Error::Evaluation(format!(
                "record has no field `{field}` for consumer {consumer:?}; available fields: {}",
                keys.join(", ")
            ))
        };
        match object {
            EvalValue::Record(mut record) => {
                let keys = record.keys().cloned().collect();
                record.remove(field).ok_or_else(|| missing(keys))
            }
            EvalValue::MappedRow { mut fields, .. } => {
                let keys = fields.keys().cloned().collect();
                fields.remove(field).ok_or_else(|| missing(keys))
            }
            EvalValue::Value(Value::Record(mut record)) => {
                let keys = record.keys().cloned().collect();
                record
                    .remove(field)
                    .map(EvalValue::Value)
                    .ok_or_else(|| missing(keys))
            }
            EvalValue::Value(Value::MappedRow { mut fields, .. }) => {
                let keys = fields.keys().cloned().collect();
                fields
                    .remove(field)
                    .map(EvalValue::Value)
                    .ok_or_else(|| missing(keys))
            }
            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => {
                Err(Error::InvalidPlan(format!(
                    "typed row member `{field}` on ListId {} reached ObjectField without a compiler-owned FieldId",
                    row.list.0
                )))
            }
            other => Err(Error::Evaluation(format!(
                "value {other:?} is not an object"
            ))),
        }
    }

    fn extend_record_from_spread(
        &mut self,
        record: &mut BTreeMap<String, EvalValue>,
        value: EvalValue,
        event: Option<&SourceEvent>,
        consumer: Option<Consumer>,
        work: &mut Work,
    ) -> Result<(), Error> {
        match value {
            EvalValue::Record(fields) | EvalValue::MappedRow { fields, .. } => {
                record.extend(fields);
            }
            EvalValue::Value(Value::Record(fields))
            | EvalValue::Value(Value::MappedRow { fields, .. }) => {
                record.extend(
                    fields
                        .into_iter()
                        .map(|(name, value)| (name, EvalValue::Value(value))),
                );
            }
            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => {
                let fields = self
                    .metadata
                    .row_field_names
                    .iter()
                    .filter(|((list, _), _)| *list == row.list)
                    .map(|((_, field), name)| (*field, name.clone()))
                    .collect::<Vec<_>>();
                for (field, name) in fields {
                    self.register_row_dependency(consumer, row, field);
                    let value = self.ensure_row_field(row, field, event, work)?;
                    record.insert(name, EvalValue::Value(value));
                }
            }
            other => {
                return Err(Error::Evaluation(format!(
                    "record spread requires a record or typed row, found {other:?}"
                )));
            }
        }
        Ok(())
    }

    fn eval_builtin_values(
        &mut self,
        function: PlanRowBuiltin,
        input: Option<EvalValue>,
        mut args: EvaluatedBuiltinArgs,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let require_input = |input: Option<EvalValue>| {
            input.ok_or_else(|| {
                Error::InvalidPlan(format!(
                    "{} has no evaluated input",
                    function.function_name()
                ))
            })
        };
        let value = match function {
            PlanRowBuiltin::BoolNot => {
                EvalValue::Value(Value::Bool(!eval_to_bool(&require_input(input)?)?))
            }
            PlanRowBuiltin::BoolAnd | PlanRowBuiltin::BoolOr => {
                return Err(Error::InvalidPlan(format!(
                    "{} bypassed its short-circuit continuation",
                    function.function_name()
                )));
            }
            PlanRowBuiltin::BoolToggle => {
                let value = eval_to_bool(&require_input(input)?)?;
                let when = take_optional_builtin_arg(&mut args, "when")
                    .map(|when| eval_to_bool(&when))
                    .transpose()?
                    .unwrap_or(true);
                EvalValue::Value(Value::Bool(if when { !value } else { value }))
            }
            PlanRowBuiltin::TextEmpty => EvalValue::Value(Value::Text(String::new())),
            PlanRowBuiltin::TextToLowercase => EvalValue::Value(Value::Text(
                eval_to_text(&require_input(input)?)?.to_lowercase(),
            )),
            PlanRowBuiltin::TextToUppercase => EvalValue::Value(Value::Text(
                eval_to_text(&require_input(input)?)?.to_uppercase(),
            )),
            PlanRowBuiltin::TextContains => {
                let input = require_input(input)?;
                let needle = take_required_builtin_arg(&mut args, "needle", function)?;
                EvalValue::Value(Value::Bool(
                    eval_to_text(&input)?.contains(&eval_to_text(&needle)?),
                ))
            }
            PlanRowBuiltin::TextIsNotEmpty => EvalValue::Value(Value::Bool(
                !eval_to_text(&require_input(input)?)?.is_empty(),
            )),
            PlanRowBuiltin::TextAllCharsIn => {
                let input = eval_to_text(&require_input(input)?)?;
                let allowed =
                    eval_to_text(&take_required_builtin_arg(&mut args, "chars", function)?)?;
                EvalValue::Value(Value::Bool(
                    input.chars().all(|character| allowed.contains(character)),
                ))
            }
            PlanRowBuiltin::NumberToText => {
                let number = eval_to_number(&require_input(input)?)?;
                let radix = take_optional_builtin_arg(&mut args, "radix")
                    .map(|value| eval_to_integer(&value))
                    .transpose()?
                    .unwrap_or(10);
                let radix = u32::try_from(radix).unwrap_or_default();
                let min_width = take_optional_builtin_arg(&mut args, "min_width")
                    .map(|value| eval_to_integer(&value))
                    .transpose()?
                    .map(|value| usize::try_from(value).unwrap_or(usize::MAX))
                    .unwrap_or_default();
                let signed_width = take_optional_builtin_arg(&mut args, "signed_width")
                    .map(|value| eval_to_integer(&value))
                    .transpose()?
                    .map(|value| u32::try_from(value).unwrap_or_default());
                let group_size = take_optional_builtin_arg(&mut args, "group_size")
                    .map(|value| eval_to_integer(&value))
                    .transpose()?
                    .map(|value| usize::try_from(value).unwrap_or_default());
                let prefix = take_optional_builtin_arg(&mut args, "prefix")
                    .map(|value| eval_to_bool(&value))
                    .transpose()?
                    .unwrap_or(false);
                EvalValue::Value(Value::Text(
                    format_number_text(
                        number,
                        NumberTextFormat {
                            radix,
                            min_width,
                            signed_width,
                            group_size,
                            prefix,
                        },
                    )
                    .map_err(|error| Error::Evaluation(error.to_string()))?,
                ))
            }
            PlanRowBuiltin::NumberToAsciiText => {
                let value = eval_to_number(&require_input(input)?)?;
                let width = take_optional_builtin_arg(&mut args, "width")
                    .map(|width| eval_to_number(&width))
                    .transpose()?;
                EvalValue::Value(Value::Text(format_number_ascii_text(value, width)))
            }
            PlanRowBuiltin::ErrorNew => {
                let code = take_optional_builtin_arg(&mut args, "code")
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .unwrap_or_else(|| "error".to_owned());
                EvalValue::Value(Value::Error { code })
            }
            PlanRowBuiltin::ErrorText => {
                let code = match require_input(input)? {
                    EvalValue::Value(Value::Error { code }) => code,
                    _ => String::new(),
                };
                EvalValue::Value(Value::Text(code))
            }
            PlanRowBuiltin::NumberCeil
            | PlanRowBuiltin::NumberFloor
            | PlanRowBuiltin::NumberRound
            | PlanRowBuiltin::NumberTruncate => {
                let value = eval_to_number(&require_input(input)?)?.get();
                let rounded = match function {
                    PlanRowBuiltin::NumberCeil => value.ceil(),
                    PlanRowBuiltin::NumberFloor => value.floor(),
                    PlanRowBuiltin::NumberRound => value.round(),
                    PlanRowBuiltin::NumberTruncate => value.trunc(),
                    _ => unreachable!(),
                };
                EvalValue::Value(Value::Number(finite_number_result(
                    rounded,
                    function.function_name(),
                )?))
            }
            PlanRowBuiltin::NumberBitWidth => {
                let width = number_bit_width(eval_to_number(&require_input(input)?)?)
                    .map_err(|error| Error::Evaluation(error.to_string()))?;
                EvalValue::Value(Value::Number(width))
            }
            PlanRowBuiltin::NumberMin | PlanRowBuiltin::NumberMax => {
                let left = eval_to_number(&require_input(input)?)?;
                let right =
                    eval_to_number(&take_required_builtin_arg(&mut args, "right", function)?)?;
                EvalValue::Value(Value::Number(if function == PlanRowBuiltin::NumberMin {
                    left.min(right)
                } else {
                    left.max(right)
                }))
            }
            PlanRowBuiltin::NumberInterpolate => {
                let start = take_required_builtin_number(&mut args, "start", function)?;
                let end = take_required_builtin_number(&mut args, "end", function)?;
                let numerator = take_required_builtin_number(&mut args, "numerator", function)?;
                let denominator = take_required_builtin_number(&mut args, "denominator", function)?;
                let fallback = take_required_builtin_number(&mut args, "fallback", function)?;
                let value = if denominator.get() == 0.0 {
                    fallback
                } else {
                    finite_number_result(
                        start.get()
                            + ((end.get() - start.get()) * numerator.get() / denominator.get()),
                        "Number/interpolate",
                    )?
                };
                EvalValue::Value(Value::Number(value))
            }
            PlanRowBuiltin::NumberProjectOffset => {
                let time = take_required_builtin_number(&mut args, "time", function)?;
                let start = take_required_builtin_number(&mut args, "viewport_start", function)?;
                let end = take_required_builtin_number(&mut args, "viewport_end", function)?;
                let width = take_required_builtin_number(&mut args, "canvas_width", function)?;
                let fallback = take_required_builtin_number(&mut args, "fallback", function)?;
                let _zoom = take_optional_builtin_arg(&mut args, "zoom");
                let span = end.get() - start.get();
                let value = if span <= 0.0 || width.get() <= 0.0 {
                    fallback
                } else {
                    finite_number_result(
                        ((time.get() - start.get()) * width.get() / span).clamp(0.0, width.get()),
                        "Number/project_offset",
                    )?
                };
                EvalValue::Value(Value::Number(value))
            }
            PlanRowBuiltin::NumberProjectTime => {
                let x = take_required_builtin_number(&mut args, "pointer_x", function)?;
                let width = take_required_builtin_number(&mut args, "pointer_width", function)?;
                let start = take_required_builtin_number(&mut args, "viewport_start", function)?;
                let end = take_required_builtin_number(&mut args, "viewport_end", function)?;
                let fallback = take_required_builtin_number(&mut args, "fallback", function)?;
                let value = if width.get() <= 0.0 {
                    fallback
                } else {
                    finite_number_result(
                        (x.get() * (end.get() - start.get()) / width.get() + start.get())
                            .clamp(start.min(end).get(), start.max(end).get()),
                        "Number/project_time",
                    )?
                };
                EvalValue::Value(Value::Number(value))
            }
            PlanRowBuiltin::NumberProjectWidth => {
                let segment_start =
                    take_required_builtin_number(&mut args, "start_time", function)?;
                let segment_end = take_required_builtin_number(&mut args, "end_time", function)?;
                let viewport_start =
                    take_required_builtin_number(&mut args, "viewport_start", function)?;
                let viewport_end =
                    take_required_builtin_number(&mut args, "viewport_end", function)?;
                let canvas_width =
                    take_required_builtin_number(&mut args, "canvas_width", function)?;
                let fallback = take_required_builtin_number(&mut args, "fallback", function)?;
                let _zoom = take_optional_builtin_arg(&mut args, "zoom");
                let viewport_span = viewport_end.get() - viewport_start.get();
                let segment_span = segment_end.get() - segment_start.get();
                let value =
                    if viewport_span <= 0.0 || segment_span <= 0.0 || canvas_width.get() <= 0.0 {
                        fallback
                    } else {
                        finite_number_result(
                            (segment_span * canvas_width.get() / viewport_span)
                                .clamp(0.0, canvas_width.get()),
                            "Number/project_width",
                        )?
                    };
                EvalValue::Value(Value::Number(value))
            }
            PlanRowBuiltin::ListGet => {
                let input = require_input(input)?;
                let index = usize::try_from(eval_to_integer(&take_required_builtin_arg(
                    &mut args, "index", function,
                )?)?)
                .map_err(|_| {
                    Error::Evaluation("List/get index must be a non-negative integer".to_owned())
                })?;
                work.consume(1)?;
                eval_to_list(input)?.into_iter().nth(index).ok_or_else(|| {
                    Error::Evaluation(format!("List/get index {index} is out of bounds"))
                })?
            }
            PlanRowBuiltin::ListLatest => {
                let mut values = eval_to_list(require_input(input)?)?;
                work.consume(1)?;
                values.pop().unwrap_or(EvalValue::Value(Value::Null))
            }
            PlanRowBuiltin::ListCount | PlanRowBuiltin::ListLength => EvalValue::Value(
                Value::integer(eval_to_list(require_input(input)?)?.len() as i64)?,
            ),
            PlanRowBuiltin::ListIsNotEmpty => EvalValue::Value(Value::Bool(
                !eval_to_list(require_input(input)?)?.is_empty(),
            )),
            PlanRowBuiltin::ListTake => {
                let count = usize::try_from(eval_to_integer(&take_required_builtin_arg(
                    &mut args, "count", function,
                )?)?)
                .map_err(|_| {
                    Error::Evaluation("List/take count must be a non-negative integer".to_owned())
                })?;
                let mut value = require_input(input)?;
                match &mut value {
                    EvalValue::List(items) => items.truncate(count),
                    EvalValue::OrderedList { items, .. } => items.truncate(count),
                    EvalValue::Value(Value::List(items)) => items.truncate(count),
                    other => {
                        return Err(Error::Evaluation(format!(
                            "List/take input {other:?} is not a list"
                        )));
                    }
                }
                work.consume(count.try_into().unwrap_or(u64::MAX))?;
                value
            }
            PlanRowBuiltin::TextJoin => {
                let items = eval_to_list(require_input(input)?)?;
                let separator = take_optional_builtin_arg(&mut args, "separator")
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .unwrap_or_default();
                let empty = take_optional_builtin_arg(&mut args, "empty")
                    .map(|value| eval_to_text(&value))
                    .transpose()?
                    .unwrap_or_default();
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                if items.is_empty() {
                    EvalValue::Value(Value::Text(empty))
                } else {
                    let values = items
                        .into_iter()
                        .map(|item| eval_to_text(&item))
                        .collect::<Result<Vec<_>, _>>()?;
                    EvalValue::Value(Value::Text(values.join(&separator)))
                }
            }
            PlanRowBuiltin::TextJoinLines => {
                let items = eval_to_list(require_input(input)?)?;
                work.consume(items.len().try_into().unwrap_or(u64::MAX))?;
                let values = items
                    .into_iter()
                    .map(|item| eval_to_text(&item))
                    .collect::<Result<Vec<_>, _>>()?;
                EvalValue::Value(Value::Text(values.join("\n")))
            }
            PlanRowBuiltin::TextTimeRangeLabel => {
                let start = eval_to_text(&require_input(input)?)?;
                let end = eval_to_text(&take_required_builtin_arg(&mut args, "end", function)?)?;
                let unit = eval_to_text(&take_required_builtin_arg(&mut args, "unit", function)?)?;
                EvalValue::Value(Value::Text(format!("{start} {unit} - {end} {unit}")))
            }
        };
        if let Some(name) = args.first_name() {
            return Err(Error::InvalidPlan(format!(
                "{} left evaluated argument `{name}` unused",
                function.function_name()
            )));
        }
        Ok(value)
    }
}

impl MachineInstance {
    fn evaluate_update(
        &mut self,
        op: &PlanOp,
        row: Option<RowId>,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<Option<Value>, Error> {
        let PlanOpKind::StateUpdate {
            value: Some(expression),
            effect: None,
            ..
        } = &op.kind
        else {
            return Err(Error::InvalidPlan(format!(
                "state update op {} has no executable value",
                op.id.0
            )));
        };
        let value = self.eval_row_expression(
            expression,
            row.or_else(|| event.and_then(|event| event.target)),
            event,
            None,
            None,
            &mut PlanLocalBindings::new(),
            work,
        )?;
        self.materialize_eval(value).map(Some)
    }

    fn stage_mutation_batch(
        &mut self,
        operations: &[Arc<PlanOp>],
        event: Option<&SourceEvent>,
        trigger: &TriggerFrame<'_>,
        work: &mut Work,
    ) -> Result<(), Error> {
        let pending = operations
            .iter()
            .map(|op| self.evaluate_mutation(op, event, trigger, work))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        work.pending_list_mutations.extend(pending);
        Ok(())
    }

    fn commit_pending_list_mutations(&mut self, work: &mut Work) -> Result<(), Error> {
        let staged = std::mem::take(&mut work.pending_list_mutations);
        let mut latest = BTreeMap::<(usize, OwnerInstanceId), PendingListMutation>::new();
        for mutation in staged {
            let (site, owner) = mutation.site_owner();
            let key = (site, owner.clone());
            match latest.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(mutation);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let previous_sequence = entry.get().sequence();
                    match mutation.sequence().cmp(&previous_sequence) {
                        std::cmp::Ordering::Greater => {
                            entry.insert(mutation);
                        }
                        std::cmp::Ordering::Less => {}
                        std::cmp::Ordering::Equal => {
                            return Err(Error::Evaluation(format!(
                                "list mutation site {site} produced multiple values for owner {:?} at source sequence {previous_sequence}; use explicit PRIORITY or proven EXCLUSIVE",
                                owner
                            )));
                        }
                    }
                }
            }
        }
        let mut pending = latest.into_values().collect::<Vec<_>>();
        pending.sort_by_key(PendingListMutation::ordinal);
        for mutation in pending {
            match mutation {
                PendingListMutation::Append {
                    list,
                    fields,
                    owner,
                    ..
                } => {
                    self.append_row_with_owner_prefix(list, fields, &owner.ancestors, None, work)?;
                }
                PendingListMutation::Remove { rows, .. } => {
                    for row in rows {
                        if self
                            .lists
                            .get(&row.list)
                            .is_some_and(|list| list.rows.contains_key(&row))
                        {
                            self.remove_row(row, work)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn evaluate_mutation(
        &mut self,
        op: &PlanOp,
        event: Option<&SourceEvent>,
        trigger: &TriggerFrame<'_>,
        work: &mut Work,
    ) -> Result<Option<PendingListMutation>, Error> {
        work.consume(1)?;
        let PlanOpKind::ListMutation { mutation } = &op.kind else {
            return Err(Error::InvalidPlan(format!(
                "mutation op {} is not a list mutation",
                op.id.0
            )));
        };
        let Some(ValueRef::List(list)) = op.output else {
            return Err(Error::InvalidPlan(format!(
                "mutation op {} has no list output",
                op.id.0
            )));
        };
        match mutation {
            PlanListMutation::Append(append) => {
                if !Self::trigger_accepts(&append.trigger, &trigger.active) {
                    return Ok(None);
                }
                let row = event.and_then(|event| event.target);
                let gate = self.eval_row_expression(
                    &append.gate,
                    row,
                    event,
                    None,
                    None,
                    &mut PlanLocalBindings::new(),
                    work,
                )?;
                if !eval_value_is_present(&gate) {
                    return Ok(None);
                }
                let item = self.eval_row_expression(
                    &append.item,
                    row,
                    event,
                    None,
                    None,
                    &mut PlanLocalBindings::new(),
                    work,
                )?;
                if !eval_value_is_present(&item) {
                    return Ok(None);
                }
                let fields = self.materialize_append_item(list, append, item, event, work)?;
                let owner = instantiate_plan_owner(&append.owner, &trigger.active)?;
                Ok(Some(PendingListMutation::Append {
                    site: append.site,
                    ordinal: append.ordinal,
                    sequence: trigger.active.sequence,
                    owner,
                    list,
                    fields,
                }))
            }
            PlanListMutation::Remove(remove) => {
                if !Self::trigger_accepts(&remove.trigger, &trigger.active) {
                    return Ok(None);
                }
                let owner = instantiate_plan_owner(&remove.owner, &trigger.active)?;
                let candidates = self
                    .list_row_ids(list)
                    .into_iter()
                    .filter(|row| {
                        self.row_owner_ancestors(*row).is_ok_and(|ancestors| {
                            ancestors == owner.ancestors.as_slice()
                                || ancestors
                                    .split_last()
                                    .is_some_and(|(_, parent)| parent == owner.ancestors.as_slice())
                        })
                    })
                    .collect::<Vec<_>>();
                work.consume(candidates.len().try_into().unwrap_or(u64::MAX))?;
                let mut removed = Vec::new();
                for row in candidates {
                    let local = (remove.local_owner, remove.row_local);
                    let gate = self.eval_contextual_body(
                        local,
                        EvalValue::Row(row),
                        &remove.gate,
                        Some(row),
                        event,
                        None,
                        None,
                        &mut PlanLocalBindings::new(),
                        work,
                    )?;
                    if !eval_value_is_present(&gate) {
                        continue;
                    }
                    let predicate = self.eval_contextual_body(
                        local,
                        EvalValue::Row(row),
                        &remove.predicate,
                        Some(row),
                        event,
                        None,
                        None,
                        &mut PlanLocalBindings::new(),
                        work,
                    )?;
                    if !eval_value_is_present(&predicate) {
                        continue;
                    }
                    if eval_to_bool(&predicate)? == remove.remove_when {
                        removed.push(row);
                    }
                }
                Ok(
                    (!removed.is_empty()).then_some(PendingListMutation::Remove {
                        site: remove.site,
                        ordinal: remove.ordinal,
                        sequence: trigger.active.sequence,
                        owner,
                        rows: removed,
                    }),
                )
            }
        }
    }

    fn materialize_append_item(
        &mut self,
        list: ListId,
        append: &boon_plan::PlanListAppend,
        item: EvalValue,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<BTreeMap<FieldId, Value>, Error> {
        let fields_by_name = append
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.field_id))
            .collect::<BTreeMap<_, _>>();
        let unique_field_ids = append
            .fields
            .iter()
            .map(|field| field.field_id)
            .collect::<BTreeSet<_>>();
        if fields_by_name.len() != append.fields.len()
            || unique_field_ids.len() != append.fields.len()
        {
            return Err(Error::InvalidPlan(format!(
                "append into list {} has duplicate authority fields",
                list.0
            )));
        }

        let record = match item {
            EvalValue::Record(fields) | EvalValue::MappedRow { fields, .. } => Some(fields),
            EvalValue::Value(Value::Record(fields))
            | EvalValue::Value(Value::MappedRow { fields, .. }) => Some(
                fields
                    .into_iter()
                    .map(|(name, value)| (name, EvalValue::Value(value)))
                    .collect(),
            ),
            EvalValue::Row(row) | EvalValue::Value(Value::Row { id: row, .. }) => {
                let copies = append
                    .row_field_copies
                    .iter()
                    .filter(|copy| copy.source_list == row.list)
                    .copied()
                    .collect::<Vec<_>>();
                if copies.is_empty() {
                    return Err(Error::InvalidPlan(format!(
                        "append into list {} received a row from ListId {} without compiler-owned field copies",
                        list.0, row.list.0
                    )));
                }
                let mut values = BTreeMap::new();
                for copy in copies {
                    self.register_row_dependency(
                        Some(Consumer::List(list)),
                        row,
                        copy.source_field,
                    );
                    values.insert(
                        copy.target_field,
                        self.ensure_row_field(row, copy.source_field, event, work)?,
                    );
                }
                return Ok(values);
            }
            EvalValue::Value(value) if fields_by_name.len() == 1 => {
                let field = *fields_by_name.values().next().expect("one append field");
                return Ok(BTreeMap::from([(field, value)]));
            }
            other => {
                return Err(Error::Evaluation(format!(
                    "append into list {} produced non-record item {other:?}",
                    list.0
                )));
            }
        }
        .expect("record append item");

        if record.keys().any(|name| !fields_by_name.contains_key(name)) {
            return Err(Error::Evaluation(format!(
                "append into list {} produced unknown fields {:?}; authority fields are {:?}",
                list.0,
                record.keys().collect::<Vec<_>>(),
                fields_by_name.keys().collect::<Vec<_>>()
            )));
        }
        record
            .into_iter()
            .map(|(name, value)| {
                let field = fields_by_name[&name];
                Ok((field, self.materialize_eval(value)?))
            })
            .collect()
    }

    fn trigger_accepts(trigger: &ValueRef, active: &ActiveTrigger) -> bool {
        match (trigger, active.cause) {
            (ValueRef::Source(trigger), TriggerCause::Source(source)) => *trigger == source,
            (ValueRef::State(trigger), TriggerCause::State(state)) => *trigger == state,
            (
                ValueRef::SourcePayload { .. }
                | ValueRef::StateProjection { .. }
                | ValueRef::Field(_)
                | ValueRef::Constant(_)
                | ValueRef::List(_)
                | ValueRef::DistributedImport(_),
                _,
            ) => false,
            (ValueRef::Source(_) | ValueRef::State(_), _) => false,
        }
    }

    fn materialize_virtual_projection_row(
        &mut self,
        list_id: ListId,
        logical_index: u64,
        fields: BTreeMap<FieldId, Value>,
        work: &mut Work,
    ) -> Result<RowId, Error> {
        let key = logical_index.checked_add(1).ok_or_else(|| {
            Error::Evaluation(format!(
                "virtual list {} logical row key overflowed",
                list_id.0
            ))
        })?;
        let row = RowId {
            list: list_id,
            key,
            generation: 1,
        };
        if self.row_exists(row) {
            return Ok(row);
        }
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list_id)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("missing list slot {}", list_id.0)))?;
        if slot
            .capacity
            .is_some_and(|capacity| self.list_row_ids(list_id).len() >= capacity)
        {
            return Err(Error::Evaluation(format!(
                "virtual list {} capacity {} is exhausted",
                list_id.0,
                slot.capacity.unwrap_or_default()
            )));
        }
        let inserted = self.insert_initial_row(&slot, key, fields, BTreeSet::new(), work)?;
        debug_assert_eq!(inserted, row);
        Ok(inserted)
    }

    fn append_row_with_owner_prefix(
        &mut self,
        list_id: ListId,
        fields: BTreeMap<FieldId, Value>,
        owner_prefix: &[OwnerInstanceRow],
        materialization_origin: Option<Vec<OwnerInstanceRow>>,
        work: &mut Work,
    ) -> Result<RowId, Error> {
        let slot = self
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list_id)
            .cloned()
            .ok_or_else(|| Error::InvalidPlan(format!("missing list slot {}", list_id.0)))?;
        if slot
            .capacity
            .is_some_and(|capacity| self.list_row_ids(list_id).len() >= capacity)
        {
            return Err(Error::Evaluation(format!(
                "list {} capacity {} is exhausted",
                list_id.0,
                slot.capacity.unwrap_or_default()
            )));
        }
        let key = self
            .lists
            .get(&list_id)
            .map(|list| list.next_key.max(1))
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        let row_id = RowId {
            list: list_id,
            key,
            generation: 1,
        };
        let mut row = Row {
            owner_ancestors: owner_prefix
                .iter()
                .copied()
                .chain(std::iter::once(OwnerInstanceRow {
                    list: row_id.list,
                    key: row_id.key,
                    generation: row_id.generation,
                }))
                .collect(),
            materialization_origin,
            fields,
            ..Row::default()
        };
        for field in self.metadata.row_computations.keys() {
            if self.metadata.row_field_owner.get(field) == Some(&list_id) {
                row.derived.insert(*field, Currentness::Dirty);
            }
        }
        let previous_next_key = self
            .lists
            .get(&list_id)
            .map(|list| list.next_key)
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        let next_key = key.checked_add(1).ok_or_else(|| {
            Error::Evaluation(format!("list {} row-key allocator is exhausted", list_id.0))
        })?;
        let prepared_order = self
            .lists
            .get(&list_id)
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?
            .prepare_push_ordered(row_id, None)?;
        let order_maintenance = prepared_order.maintenance.clone();
        let prepared_index_dirty =
            self.prepare_ordered_index_rows_dirty_for_list(list_id, [row_id]);
        charge_source_order_maintenance(work, &order_maintenance)?;
        prepared_index_dirty.charge(work)?;
        let was_structurally_touched = self.touched_lists.contains(&list_id);
        let list = self
            .lists
            .get_mut(&list_id)
            .ok_or_else(|| Error::Evaluation(format!("list {} is missing", list_id.0)))?;
        let source_order = prepared_order.commit(list);
        list.next_key = next_key;
        work.authority_undo.push(AuthorityUndo::AppendRow {
            row: row_id,
            previous_next_key,
            source_order,
            touched_list: was_structurally_touched,
        });
        list.rows.insert(row_id, row);
        list.index_owner_partition_row(row_id)?;
        record_source_order_maintenance(work, &order_maintenance);
        self.mark_list_semantic_change(list_id, work)?;
        self.touched_lists.insert(list_id);
        let fanout = prepared_index_dirty.fanout;
        self.commit_ordered_index_dirty(prepared_index_dirty);
        record_ordered_index_fanout(work, fanout);
        work.suppress_row_deltas.insert(row_id);
        self.initialize_indexed_states(row_id, work)?;
        work.suppress_row_deltas.remove(&row_id);

        let authority_fields = self.authority_fields_for_list(list_id);
        let row_authority = self
            .lists
            .get(&list_id)
            .and_then(|list| list.rows.get(&row_id))
            .ok_or_else(|| Error::Evaluation("appended row disappeared".to_owned()))?;
        let fields = authority_fields
            .iter()
            .filter_map(|field| {
                row_authority
                    .fields
                    .get(field)
                    .cloned()
                    .map(|value| (*field, value))
            })
            .collect();
        let next_key = self
            .lists
            .get(&list_id)
            .map(|list| list.next_key)
            .unwrap_or(row_id.key.saturating_add(1));
        if was_structurally_touched {
            work.authority_deltas.push(AuthorityDelta::InsertRow {
                row: RowAuthority {
                    id: row_id,
                    source_order_token: self
                        .lists
                        .get(&list_id)
                        .and_then(|list| list.order_token(row_id))
                        .ok_or_else(|| {
                            Error::InvalidPlan(
                                "appended authority row has no source-order token".to_owned(),
                            )
                        })?,
                    owner_ancestors: row_authority.owner_ancestors.clone(),
                    materialization_origin: row_authority.materialization_origin.clone(),
                    fields,
                    touched_fields: BTreeSet::new(),
                },
                index: self
                    .lists
                    .get(&list_id)
                    .map(|list| list.order.len().saturating_sub(1))
                    .unwrap_or(0)
                    .try_into()
                    .map_err(|_| {
                        Error::Evaluation("list insertion index exceeds u64".to_owned())
                    })?,
                next_key,
            });
        } else {
            work.authority_deltas.push(AuthorityDelta::ReplaceList {
                list_id,
                authority: self.list_authority(list_id)?,
            });
        }

        self.bind_row_sources(row_id, slot.scope_id)?;
        work.changed_rows.insert(row_id);
        if work.emit {
            let row = self
                .lists
                .get(&list_id)
                .and_then(|list| list.rows.get(&row_id))
                .expect("appended row exists");
            work.deltas.push(Delta::InsertRow {
                row: RowSnapshot {
                    id: row_id,
                    fields: row.fields.clone(),
                },
            });
            for (source, binding_id) in &row.bindings {
                work.deltas.push(Delta::BindSource {
                    row: row_id,
                    source: *source,
                    binding_id: *binding_id,
                });
            }
        }
        self.invalidate_list_structure(list_id, work);
        Ok(row_id)
    }

    fn bind_row_sources(&mut self, row: RowId, scope: Option<ScopeId>) -> Result<(), Error> {
        let sources = self
            .metadata
            .routes
            .values()
            .filter(|route| route.scope_id == scope && route.scoped)
            .map(|route| route.source_id)
            .collect::<Vec<_>>();
        let row_state = self
            .lists
            .get_mut(&row.list)
            .and_then(|list| list.rows.get_mut(&row))
            .ok_or_else(|| {
                Error::Evaluation(format!("cannot bind sources to missing row {row:?}"))
            })?;
        for source in sources {
            let binding_id = self.next_binding_id;
            self.next_binding_id = self.next_binding_id.saturating_add(1);
            row_state.bindings.insert(source, binding_id);
        }
        Ok(())
    }

    fn remove_row(&mut self, row: RowId, work: &mut Work) -> Result<(), Error> {
        let owner = self.row_owner_ancestors(row)?.to_vec();
        let mut descendants = self
            .lists
            .values()
            .flat_map(|list| list.rows.iter())
            .filter_map(|(candidate, state)| {
                (*candidate != row && state.owner_ancestors.starts_with(&owner))
                    .then_some((state.owner_ancestors.len(), *candidate))
            })
            .collect::<Vec<_>>();
        descendants.sort_by(|(left_depth, left), (right_depth, right)| {
            right_depth.cmp(left_depth).then_with(|| left.cmp(right))
        });
        for (_, descendant) in descendants {
            if self
                .lists
                .get(&descendant.list)
                .is_some_and(|list| list.rows.contains_key(&descendant))
            {
                self.remove_row_exact(descendant, work)?;
            }
        }
        self.remove_row_exact(row, work)
    }

    fn remove_row_exact(&mut self, row: RowId, work: &mut Work) -> Result<(), Error> {
        let effect_consumers = self
            .effect_activations
            .keys()
            .filter(|consumer| consumer.row == Some(row))
            .copied()
            .collect::<Vec<_>>();
        for consumer in effect_consumers {
            self.record_effect_activation_undo(consumer, work);
            self.effect_activations.remove(&consumer);
            self.clear_consumer_dependencies(Consumer::Effect(consumer));
            work.pending_effect_reconciliations.remove(&consumer);
        }
        self.cancel_pending_transient_effects_for_row(row, work);
        let (removed_value, previous_next_key) = self
            .lists
            .get(&row.list)
            .and_then(|list| Some((list.rows.get(&row)?.clone(), list.next_key)))
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot remove missing row {}:{}:{}",
                    row.list.0, row.key, row.generation
                ))
            })?;
        let touched_fields = self
            .touched_row_fields
            .iter()
            .filter_map(|(candidate, field)| (*candidate == row).then_some(*field))
            .collect();
        let was_structurally_touched = self.touched_lists.contains(&row.list);
        let prepared_order = self
            .lists
            .get(&row.list)
            .ok_or_else(|| Error::Evaluation(format!("cannot remove missing list {}", row.list.0)))?
            .prepare_remove_ordered(row, None)?
            .ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot remove missing row {}:{}:{} from source order",
                    row.list.0, row.key, row.generation
                ))
            })?;
        let order_maintenance = prepared_order.maintenance.clone();
        let prepared_index_dirty = self.prepare_ordered_index_rows_dirty_for_list(row.list, [row]);
        charge_source_order_maintenance(work, &order_maintenance)?;
        prepared_index_dirty.charge(work)?;
        self.mark_list_semantic_change(row.list, work)?;
        let fanout = prepared_index_dirty.fanout;
        let removed = {
            let list = self.lists.get_mut(&row.list).ok_or_else(|| {
                Error::Evaluation(format!("cannot remove missing list {}", row.list.0))
            })?;
            let source_order = prepared_order.commit(list);
            work.authority_undo.push(AuthorityUndo::RemoveRow {
                row,
                value: removed_value,
                source_order,
                previous_next_key,
                touched_list: was_structurally_touched,
                touched_fields,
            });
            let removed = list.rows.remove(&row).ok_or_else(|| {
                Error::Evaluation(format!(
                    "cannot remove missing row {}:{}:{}",
                    row.list.0, row.key, row.generation
                ))
            })?;
            list.remove_owner_partition_row(row, &removed);
            removed
        };
        self.commit_ordered_index_dirty(prepared_index_dirty);
        record_ordered_index_fanout(work, fanout);
        record_source_order_maintenance(work, &order_maintenance);
        self.touched_lists.insert(row.list);
        self.touched_row_fields
            .retain(|(candidate, _)| *candidate != row);
        if was_structurally_touched {
            work.authority_deltas.push(AuthorityDelta::RemoveRow {
                row,
                next_key: previous_next_key,
            });
        } else {
            work.authority_deltas.push(AuthorityDelta::ReplaceList {
                list_id: row.list,
                authority: self.list_authority(row.list)?,
            });
        }
        for field in removed.fields.keys() {
            self.invalidate_row_field(row, *field, work);
        }
        for field in removed.derived.keys() {
            self.clear_consumer_dependencies(Consumer::Row(row, *field));
        }
        work.changed_rows.insert(row);
        if work.emit {
            for (source, binding_id) in removed.bindings {
                work.deltas.push(Delta::UnbindSource {
                    row,
                    source,
                    binding_id,
                });
            }
            work.deltas.push(Delta::RemoveRow { row });
        }
        self.invalidate_list_structure(row.list, work);
        Ok(())
    }

    fn evaluate_projection(
        &mut self,
        output: ValueRef,
        op: PlanOpId,
        projection: &PlanListProjection,
        event: Option<&SourceEvent>,
        work: &mut Work,
    ) -> Result<EvalValue, Error> {
        let (consumer, output_field) = match output {
            ValueRef::Field(field) => (Some(Consumer::Root(field)), Some(field)),
            ValueRef::List(list) => (Some(Consumer::List(list)), None),
            _ => {
                return Err(Error::InvalidPlan(format!(
                    "list projection {} has a non-field/list output",
                    op.0
                )));
            }
        };
        match projection {
            PlanListProjection::Chunk { source_list, size } => {
                if *size == 0 {
                    return Err(Error::InvalidPlan(format!(
                        "chunk projection {} has size zero",
                        op.0
                    )));
                }
                let rows = self.eval_value_ref(
                    &ValueRef::List(*source_list),
                    None,
                    event,
                    output_field,
                    consumer,
                    work,
                )?;
                let rows = eval_to_list(rows)?;
                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                let mut chunks = Vec::new();
                for (index, chunk) in rows.chunks(*size).enumerate() {
                    chunks.push(EvalValue::Record(BTreeMap::from([
                        (
                            "label".to_owned(),
                            EvalValue::Value(Value::Text(index.to_string())),
                        ),
                        ("items".to_owned(), EvalValue::List(chunk.to_vec())),
                    ])));
                }
                Ok(EvalValue::List(chunks))
            }
            PlanListProjection::ChunkValue { source, size } => {
                if *size == 0 {
                    return Err(Error::InvalidPlan(format!(
                        "chunk-value projection {} has size zero",
                        op.0
                    )));
                }
                let source =
                    self.eval_value_ref(source, None, event, output_field, consumer, work)?;
                let rows = eval_to_list(source)?;
                work.consume(rows.len().try_into().unwrap_or(u64::MAX))?;
                let chunks = rows
                    .chunks(*size)
                    .enumerate()
                    .map(|(index, chunk)| {
                        EvalValue::Record(BTreeMap::from([
                            (
                                "label".to_owned(),
                                EvalValue::Value(Value::Text(index.to_string())),
                            ),
                            ("items".to_owned(), EvalValue::List(chunk.to_vec())),
                        ]))
                    })
                    .collect();
                Ok(EvalValue::List(chunks))
            }
            PlanListProjection::Unknown { summary } => Err(Error::Unsupported {
                op,
                detail: format!("unknown list projection: {summary}"),
            }),
        }
    }
}

fn normalize_scalar_list_item(
    value: Value,
    item_type: &boon_plan::DataTypePlan,
) -> Result<Value, Error> {
    match (value, item_type) {
        (Value::Text(value), boon_plan::DataTypePlan::Number) => value
            .parse::<i64>()
            .map_err(|_| Error::Evaluation(format!("`{value}` is not an exact list NUMBER")))
            .and_then(Value::integer),
        (value, _) => normalize_host_output_value(value),
    }
}

fn eval_value_is_present(value: &EvalValue) -> bool {
    match value {
        EvalValue::Value(Value::Null) => false,
        EvalValue::Value(Value::Text(value)) if value == "SKIP" => false,
        _ => true,
    }
}

fn expression_row(row: Option<RowId>) -> Option<RowId> {
    row
}

fn row_identity_value(row: RowId) -> Value {
    Value::Row {
        id: row,
        fields: BTreeMap::new(),
    }
}

pub(crate) fn normalize_host_output_value(value: Value) -> Result<Value, Error> {
    match value {
        Value::List(values) => values
            .into_iter()
            .map(normalize_host_output_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Value::Record(fields) => fields
            .into_iter()
            .map(|(name, value)| Ok((name, normalize_host_output_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        Value::MappedRow { fields, .. } => fields
            .into_iter()
            .map(|(name, value)| Ok((name, normalize_host_output_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        Value::Row { .. } => Err(Error::Evaluation(
            "host list output exposes an unprojected runtime row; map it to explicit data fields"
                .to_owned(),
        )),
        Value::HostBound { .. } => Err(Error::Evaluation(
            "host outputs cannot expose process-local host bindings".to_owned(),
        )),
        Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::Text(_)
        | Value::Bytes(_)
        | Value::Error { .. } => Ok(value),
    }
}

fn normalize_effect_intent_value(value: Value) -> Result<Value, Error> {
    match value {
        Value::List(values) => values
            .into_iter()
            .map(normalize_effect_intent_value)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Value::Record(fields) => fields
            .into_iter()
            .map(|(name, value)| Ok((name, normalize_effect_intent_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        Value::MappedRow { fields, .. } => fields
            .into_iter()
            .map(|(name, value)| Ok((name, normalize_effect_intent_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, Error>>()
            .map(Value::Record),
        Value::Row { .. } => Err(Error::Evaluation(
            "effect intent exposes an unprojected runtime row; map it to explicit data fields"
                .to_owned(),
        )),
        value @ Value::HostBound { .. }
        | value @ Value::Null
        | value @ Value::Bool(_)
        | value @ Value::Number(_)
        | value @ Value::Text(_)
        | value @ Value::Bytes(_)
        | value @ Value::Error { .. } => Ok(value),
    }
}

fn eval_row_id(value: &EvalValue) -> Option<RowId> {
    match value {
        EvalValue::Row(id) | EvalValue::MappedRow { id, .. } => Some(*id),
        EvalValue::Value(Value::Row { id, .. }) | EvalValue::Value(Value::MappedRow { id, .. }) => {
            Some(*id)
        }
        EvalValue::Value(_)
        | EvalValue::List(_)
        | EvalValue::Record(_)
        | EvalValue::OrderedList { .. } => None,
    }
}

fn eval_to_list(value: EvalValue) -> Result<Vec<EvalValue>, Error> {
    match value {
        EvalValue::List(values) => Ok(values),
        EvalValue::OrderedList { items, .. } => {
            Ok(items.into_iter().map(|item| item.value).collect())
        }
        EvalValue::Value(Value::List(values)) => {
            Ok(values.into_iter().map(EvalValue::Value).collect())
        }
        other => Err(Error::Evaluation(format!("value {other:?} is not a list"))),
    }
}

fn take_optional_builtin_arg(args: &mut EvaluatedBuiltinArgs, name: &str) -> Option<EvalValue> {
    args.remove(name)
}

fn take_required_builtin_arg(
    args: &mut EvaluatedBuiltinArgs,
    name: &str,
    function: PlanRowBuiltin,
) -> Result<EvalValue, Error> {
    args.remove(name).ok_or_else(|| {
        Error::InvalidPlan(format!(
            "{} has no evaluated `{name}` argument",
            function.function_name()
        ))
    })
}

fn take_required_builtin_number(
    args: &mut EvaluatedBuiltinArgs,
    name: &str,
    function: PlanRowBuiltin,
) -> Result<FiniteReal, Error> {
    eval_to_number(&take_required_builtin_arg(args, name, function)?)
}

fn collect_list_access_indexes(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    indexes: &mut BTreeSet<PlanListIndexId>,
) -> Result<(), Error> {
    let mut pending = vec![expression];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let node = arena
            .node(expression)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        match node {
            PlanRowExpressionNode::ListAccess { access } => {
                indexes.insert(access.index);
            }
            PlanRowExpressionNode::ListPage { page } => {
                indexes.insert(page.access.index);
            }
            _ => {}
        }
        node.visit_children(&mut |child| pending.push(child));
    }
    Ok(())
}

fn eval_to_ordered_items(
    value: EvalValue,
) -> Result<(Vec<OrderedEvalItem>, Option<Vec<EvalOrderDirection>>), Error> {
    match value {
        EvalValue::OrderedList { items, directions } => Ok((items, Some(directions))),
        other => Ok((
            eval_to_list(other)?
                .into_iter()
                .enumerate()
                .map(|(source_order, value)| OrderedEvalItem {
                    value,
                    keys: Vec::new(),
                    source_order,
                })
                .collect(),
            None,
        )),
    }
}

fn eval_ordered_items(
    items: Vec<OrderedEvalItem>,
    directions: Option<Vec<EvalOrderDirection>>,
) -> EvalValue {
    match directions {
        Some(directions) => EvalValue::OrderedList { items, directions },
        None => EvalValue::List(items.into_iter().map(|item| item.value).collect()),
    }
}

fn eval_order_direction(value: &EvalValue) -> Result<EvalOrderDirection, Error> {
    match value {
        EvalValue::Value(Value::Text(value)) if value == "Ascending" => {
            Ok(EvalOrderDirection::Ascending)
        }
        EvalValue::Value(Value::Text(value)) if value == "Descending" => {
            Ok(EvalOrderDirection::Descending)
        }
        other => Err(Error::Evaluation(format!(
            "list order direction must be Ascending or Descending, found {other:?}"
        ))),
    }
}

fn eval_order_key(value: Value) -> Result<Value, Error> {
    match value {
        value @ (Value::Bool(_) | Value::Number(_) | Value::Text(_)) => Ok(value),
        other => Err(Error::Evaluation(format!(
            "list order key must be Bool, Number, Text, or a fieldless tag, found {other:?}"
        ))),
    }
}

fn compare_ordered_items(
    left: &OrderedEvalItem,
    right: &OrderedEvalItem,
    directions: &[EvalOrderDirection],
) -> std::cmp::Ordering {
    for ((left_key, right_key), direction) in left
        .keys
        .iter()
        .zip(&right.keys)
        .zip(directions.iter().copied())
    {
        let ordering = left_key.cmp(right_key);
        let ordering = match direction {
            EvalOrderDirection::Ascending => ordering,
            EvalOrderDirection::Descending => ordering.reverse(),
        };
        if !ordering.is_eq() {
            return ordering;
        }
    }
    left.source_order.cmp(&right.source_order)
}

fn eval_to_text(value: &EvalValue) -> Result<String, Error> {
    match value {
        EvalValue::Value(value) => value_to_text(value),
        other => Err(Error::Evaluation(format!(
            "value {other:?} is not text-like"
        ))),
    }
}

fn page_position_bytes(value: &Value) -> Result<Option<Bytes>, ()> {
    match value {
        Value::Text(tag) if tag == "Start" => Ok(None),
        Value::Record(fields)
            if fields.len() == 2
                && matches!(fields.get("$tag"), Some(Value::Text(tag)) if tag == "Cursor") =>
        {
            match fields.get("value") {
                Some(Value::Bytes(value)) => Ok(Some(value.clone())),
                _ => Err(()),
            }
        }
        Value::HostBound { visible, .. } => page_position_bytes(visible),
        _ => Err(()),
    }
}

fn open_evaluated_list_access<'a>(
    index: &'a OrderedIndex,
    selection: &EvaluatedListAccessSelection,
    after: Option<&AccessCursorKey>,
    semantic_component_count: usize,
) -> Result<AccessStream<'a>, AccessError> {
    let physical_prefix_count = index
        .schema()
        .components()
        .len()
        .checked_sub(semantic_component_count)
        .ok_or(AccessError::InvalidProjectionPrefix {
            actual: semantic_component_count,
            maximum: index.schema().components().len(),
        })?;
    let physical_after =
        |leading: &[StructuralValue]| -> Result<Option<AccessCursorKey>, AccessError> {
            let Some(after) = after else {
                return Ok(None);
            };
            if leading.len() < physical_prefix_count {
                return Err(AccessError::InvalidProjectionPrefix {
                    actual: physical_prefix_count,
                    maximum: leading.len(),
                });
            }
            let mut parts = leading[..physical_prefix_count].to_vec();
            parts.extend_from_slice(after.key().parts());
            Ok(Some(AccessCursorKey::new(
                StructuralKey::new(parts)?,
                after.source_order(),
                after.row_id(),
            )))
        };
    let project = |stream: AccessStream<'a>| stream.project_key_prefix(physical_prefix_count);
    match selection {
        EvaluatedListAccessSelection::OrderedStart => {
            let after = physical_after(&[])?;
            project(index.ordered_start(after.as_ref())?)
        }
        EvaluatedListAccessSelection::KeyPrefix { values } => {
            let after = physical_after(values)?;
            project(index.key_prefix(values, after.as_ref())?)
        }
        EvaluatedListAccessSelection::TextPrefix { leading, prefix } => {
            let after = physical_after(leading)?;
            project(index.text_prefix(leading, prefix, after.as_ref())?)
        }
        EvaluatedListAccessSelection::ComponentRange {
            leading,
            lower,
            upper,
        } => {
            let lower = match lower {
                Some((value, true)) => Bound::Included(value),
                Some((value, false)) => Bound::Excluded(value),
                None => Bound::Unbounded,
            };
            let upper = match upper {
                Some((value, true)) => Bound::Included(value),
                Some((value, false)) => Bound::Excluded(value),
                None => Bound::Unbounded,
            };
            let after = physical_after(leading)?;
            project(index.component_range(leading, lower, upper, after.as_ref())?)
        }
        EvaluatedListAccessSelection::Union { branches } => branches
            .iter()
            .map(|branch| {
                open_evaluated_list_access(index, branch, after, semantic_component_count)
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(AccessStream::union),
        EvaluatedListAccessSelection::Intersection { branches } => branches
            .iter()
            .map(|branch| {
                open_evaluated_list_access(index, branch, after, semantic_component_count)
            })
            .collect::<Result<Vec<_>, _>>()
            .and_then(AccessStream::intersection),
    }
}

fn semantic_page_cursor_key(
    cursor: &AccessCursorKey,
    semantic_component_count: usize,
) -> Result<StructuralKey, Error> {
    let components = cursor.key().parts();
    let start = components
        .len()
        .checked_sub(semantic_component_count)
        .ok_or_else(|| {
            Error::InvalidPlan(format!(
                "typed page semantic order has {semantic_component_count} components but its physical cursor has {}",
                components.len()
            ))
        })?;
    StructuralKey::new(components[start..].to_vec())
        .map_err(|error| Error::InvalidPlan(error.to_string()))
}

fn row_expression_contains(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    candidate: PlanRowExpressionId,
) -> Result<bool, Error> {
    let mut pending = vec![expression];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if expression == candidate {
            return Ok(true);
        }
        if !visited.insert(expression) {
            continue;
        }
        arena
            .node(expression)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?
            .visit_children(&mut |child| pending.push(child));
    }
    Ok(false)
}

fn collect_uncaptured_page_dependencies(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    captured: &[PlanRowExpressionId],
    source_list: ListId,
    inputs: &mut BTreeSet<ValueRef>,
    intrinsics: &mut BTreeSet<PlanIntrinsic>,
) -> Result<(), Error> {
    let captured = captured.iter().copied().collect::<BTreeSet<_>>();
    let mut pending = vec![expression];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if captured.contains(&expression) || !visited.insert(expression) {
            continue;
        }
        let node = arena
            .node(expression)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        let mut insert_input = |input: ValueRef| {
            if !matches!(input, ValueRef::List(list) if list == source_list) {
                inputs.insert(input);
            }
        };
        match node {
            PlanRowExpressionNode::Intrinsic { intrinsic } => {
                intrinsics.insert(*intrinsic);
            }
            PlanRowExpressionNode::Field { input } => insert_input(input.clone()),
            PlanRowExpressionNode::Constant { constant_id } => {
                insert_input(ValueRef::Constant(*constant_id));
            }
            PlanRowExpressionNode::ListGetField { list_id, .. }
            | PlanRowExpressionNode::ListRef { list_id }
            | PlanRowExpressionNode::AuthorityListRef { list_id }
            | PlanRowExpressionNode::ListRowField { list_id, .. } => {
                insert_input(ValueRef::List(*list_id));
            }
            PlanRowExpressionNode::EventRow { source, .. } => {
                insert_input(ValueRef::Source(*source));
            }
            _ => {}
        }
        node.visit_children(&mut |child| pending.push(child));
    }
    Ok(())
}

fn uncaptured_expression_references_local(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    captured: &[PlanRowExpressionId],
    owner: PlanStaticOwnerId,
    local: PlanLocalId,
) -> Result<bool, Error> {
    let captured = captured.iter().copied().collect::<BTreeSet<_>>();
    let mut pending = vec![expression];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if captured.contains(&expression) || !visited.insert(expression) {
            continue;
        }
        let node = arena
            .node(expression)
            .map_err(|error| Error::InvalidPlan(error.to_string()))?;
        if matches!(
            node,
            PlanRowExpressionNode::Local {
                owner: candidate_owner,
                local: candidate_local,
                ..
            } | PlanRowExpressionNode::LocalRow {
                owner: candidate_owner,
                local: candidate_local,
            } if *candidate_owner == owner && *candidate_local == local
        ) {
            return Ok(true);
        }
        node.visit_children(&mut |child| pending.push(child));
    }
    Ok(false)
}

fn page_terminal_variant(tag: &str) -> EvalValue {
    EvalValue::Value(Value::Text(tag.to_owned()))
}

fn page_result(items: Vec<Value>, next: Option<Vec<u8>>) -> EvalValue {
    let next = match next {
        Some(value) => Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("Cursor".to_owned())),
            ("value".to_owned(), Value::Bytes(Bytes::from(value))),
        ])),
        None => Value::Text("End".to_owned()),
    };
    EvalValue::Value(Value::Record(BTreeMap::from([
        ("$tag".to_owned(), Value::Text("Page".to_owned())),
        ("items".to_owned(), Value::List(items)),
        ("next".to_owned(), next),
    ])))
}

fn evaluated_list_access_selection_matches_key(
    index: &OrderedIndex,
    selection: &EvaluatedListAccessSelection,
    key: &StructuralKey,
) -> Result<bool, AccessError> {
    match selection {
        EvaluatedListAccessSelection::OrderedStart => Ok(true),
        EvaluatedListAccessSelection::KeyPrefix { values } => index.key_matches_prefix(key, values),
        EvaluatedListAccessSelection::TextPrefix { leading, prefix } => {
            index.key_matches_text_prefix(key, leading, prefix)
        }
        EvaluatedListAccessSelection::ComponentRange {
            leading,
            lower,
            upper,
        } => {
            let lower = match lower {
                Some((value, true)) => Bound::Included(value),
                Some((value, false)) => Bound::Excluded(value),
                None => Bound::Unbounded,
            };
            let upper = match upper {
                Some((value, true)) => Bound::Included(value),
                Some((value, false)) => Bound::Excluded(value),
                None => Bound::Unbounded,
            };
            index.key_matches_component_range(key, leading, lower, upper)
        }
        EvaluatedListAccessSelection::Union { branches } => {
            for branch in branches {
                if evaluated_list_access_selection_matches_key(index, branch, key)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        EvaluatedListAccessSelection::Intersection { branches } => {
            for branch in branches {
                if !evaluated_list_access_selection_matches_key(index, branch, key)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

fn record_access_metrics(work: &mut Work, metrics: AccessMetrics, result_count: usize) {
    work.metrics.access_index_seek_count = work
        .metrics
        .access_index_seek_count
        .saturating_add(metrics.index_seeks);
    work.metrics.access_cursor_seek_count = work
        .metrics
        .access_cursor_seek_count
        .saturating_add(metrics.cursor_seeks);
    work.metrics.access_key_range_count = work
        .metrics
        .access_key_range_count
        .saturating_add(metrics.key_ranges);
    work.metrics.access_key_count = work
        .metrics
        .access_key_count
        .saturating_add(metrics.keys_visited);
    work.metrics.access_candidate_count = work
        .metrics
        .access_candidate_count
        .saturating_add(metrics.candidates_visited);
    work.metrics.access_kernel_returned_count = work
        .metrics
        .access_kernel_returned_count
        .saturating_add(metrics.rows_returned);
    work.metrics.access_branch_poll_count = work
        .metrics
        .access_branch_poll_count
        .saturating_add(metrics.branch_polls);
    work.metrics.access_union_duplicate_skip_count = work
        .metrics
        .access_union_duplicate_skip_count
        .saturating_add(metrics.union_duplicates_skipped);
    work.metrics.access_intersection_candidate_skip_count = work
        .metrics
        .access_intersection_candidate_skip_count
        .saturating_add(metrics.intersection_candidates_skipped);
    work.metrics.access_full_scan_count = work
        .metrics
        .access_full_scan_count
        .saturating_add(metrics.full_scans);
    work.metrics.access_work_limit_failure_count = work
        .metrics
        .access_work_limit_failure_count
        .saturating_add(metrics.work_limit_failures);
    work.metrics.access_result_count = work
        .metrics
        .access_result_count
        .saturating_add(u64::try_from(result_count).unwrap_or(u64::MAX));
}

fn value_to_text(value: &Value) -> Result<String, Error> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(if *value { "True" } else { "False" }.to_owned()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Text(value) => Ok(value.clone()),
        Value::Bytes(bytes) => String::from_utf8(bytes.to_vec())
            .map_err(|error| Error::Evaluation(format!("invalid UTF-8: {error}"))),
        Value::Error { code } => Ok(code.clone()),
        Value::List(_) | Value::Record(_) | Value::MappedRow { .. } | Value::Row { .. } => Err(
            Error::Evaluation("list or record cannot be converted to text".to_owned()),
        ),
        Value::HostBound { .. } => Err(Error::Evaluation(
            "host-bound values cannot be converted to text".to_owned(),
        )),
    }
}

fn source_event_transform_op(op: &PlanOp) -> bool {
    matches!(
        &op.kind,
        PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            ..
        }
    )
}

fn value_to_bool(value: &Value) -> Result<bool, Error> {
    match value {
        Value::Bool(value) => Ok(*value),
        Value::Text(value) if value == "True" => Ok(true),
        Value::Text(value) if value == "False" => Ok(false),
        other => Err(Error::Evaluation(format!("value {other:?} is not boolean"))),
    }
}

fn eval_to_number(value: &EvalValue) -> Result<FiniteReal, Error> {
    match value {
        EvalValue::Value(Value::Number(value)) => Ok(*value),
        EvalValue::Value(Value::Text(value)) => value
            .parse::<FiniteReal>()
            .map_err(|_| Error::Evaluation(format!("text `{value}` is not a number"))),
        other => Err(Error::Evaluation(format!("value {other:?} is not numeric"))),
    }
}

fn eval_to_integer(value: &EvalValue) -> Result<i64, Error> {
    eval_to_number(value)?
        .to_i64_exact()
        .map_err(|error| Error::Evaluation(error.to_string()))
}

fn eval_to_bool(value: &EvalValue) -> Result<bool, Error> {
    match value {
        EvalValue::Value(Value::Bool(value)) => Ok(*value),
        EvalValue::Value(Value::Text(value)) if value == "True" => Ok(true),
        EvalValue::Value(Value::Text(value)) if value == "False" => Ok(false),
        other => Err(Error::Evaluation(format!("value {other:?} is not boolean"))),
    }
}

fn eval_to_bytes(value: &EvalValue) -> Result<Bytes, Error> {
    match value {
        EvalValue::Value(Value::Bytes(value)) => Ok(value.clone()),
        other => Err(Error::Evaluation(format!("value {other:?} is not BYTES"))),
    }
}

fn finite_number_result(value: f64, context: &str) -> Result<FiniteReal, Error> {
    FiniteReal::new(value)
        .map_err(|_| Error::Evaluation(format!("{context} produced a non-finite Number")))
}

fn eval_number_infix(op: PlanInfixOp, left: &EvalValue, right: &EvalValue) -> Result<Value, Error> {
    for value in [left, right] {
        if let EvalValue::Value(Value::Error { code }) = value {
            return Ok(Value::Error { code: code.clone() });
        }
    }
    if matches!(left, EvalValue::Value(Value::Text(value)) if value == "NaN")
        || matches!(right, EvalValue::Value(Value::Text(value)) if value == "NaN")
    {
        return Ok(Value::Text("NaN".to_owned()));
    }
    if matches!(op, PlanInfixOp::Equal | PlanInfixOp::NotEqual) {
        let equal = match (eval_to_numeric(left), eval_to_numeric(right)) {
            (Ok(left), Ok(right)) => numeric_compare(left, PlanInfixOp::Equal, right)?,
            _ => eval_data_equal(left, right)?,
        };
        return Ok(Value::Bool(if op == PlanInfixOp::Equal {
            equal
        } else {
            !equal
        }));
    }
    let left_number = eval_to_numeric(left);
    let right_number = eval_to_numeric(right);
    if op == PlanInfixOp::Add && (left_number.is_err() || right_number.is_err()) {
        return Ok(Value::Text(format!(
            "{}{}",
            eval_to_text(left)?,
            eval_to_text(right)?
        )));
    }
    numeric_infix(left_number?, op, right_number?)
}

fn eval_data_equal(left: &EvalValue, right: &EvalValue) -> Result<bool, Error> {
    match (left, right) {
        (EvalValue::Value(left), EvalValue::Value(right)) => value_data_equal(left, right),
        (EvalValue::Value(left), right) => value_eval_data_equal(left, right),
        (left, EvalValue::Value(right)) => value_eval_data_equal(right, left),
        (EvalValue::List(left), EvalValue::List(right)) => eval_list_data_equal(left, right),
        (
            EvalValue::OrderedList { items: left, .. },
            EvalValue::OrderedList { items: right, .. },
        ) => eval_ordered_list_data_equal(left, right),
        (EvalValue::List(left), EvalValue::OrderedList { items: right, .. }) => {
            eval_list_ordered_data_equal(left, right)
        }
        (EvalValue::OrderedList { items: left, .. }, EvalValue::List(right)) => {
            eval_list_ordered_data_equal(right, left)
        }
        (EvalValue::Record(left), EvalValue::Record(right)) => {
            eval_named_fields_data_equal(left, right)
        }
        (EvalValue::Record(left), EvalValue::MappedRow { fields: right, .. })
        | (EvalValue::MappedRow { fields: left, .. }, EvalValue::Record(right))
        | (EvalValue::MappedRow { fields: left, .. }, EvalValue::MappedRow { fields: right, .. }) => {
            eval_named_fields_data_equal(left, right)
        }
        (EvalValue::Row(_), _) | (_, EvalValue::Row(_)) => Err(Error::Evaluation(
            "unprojected runtime row reached Boon data equality".to_owned(),
        )),
        _ => Ok(false),
    }
}

fn value_eval_data_equal(value: &Value, eval: &EvalValue) -> Result<bool, Error> {
    match (value.visible(), eval) {
        (Value::List(values), EvalValue::List(evals)) => value_eval_list_data_equal(values, evals),
        (Value::List(values), EvalValue::OrderedList { items, .. }) => {
            value_ordered_list_data_equal(values, items)
        }
        (Value::Record(values), EvalValue::Record(evals))
        | (Value::Record(values), EvalValue::MappedRow { fields: evals, .. })
        | (Value::MappedRow { fields: values, .. }, EvalValue::Record(evals))
        | (Value::MappedRow { fields: values, .. }, EvalValue::MappedRow { fields: evals, .. }) => {
            value_eval_named_fields_data_equal(values, evals)
        }
        (Value::Row { .. }, _) | (_, EvalValue::Row(_)) => Err(Error::Evaluation(
            "unprojected runtime row reached Boon data equality".to_owned(),
        )),
        _ => Ok(false),
    }
}

fn value_data_equal(left: &Value, right: &Value) -> Result<bool, Error> {
    let left = left.visible();
    let right = right.visible();
    match (left, right) {
        (Value::List(left), Value::List(right)) => value_list_data_equal(left, right),
        (Value::Record(left), Value::Record(right))
        | (Value::Record(left), Value::MappedRow { fields: right, .. })
        | (Value::MappedRow { fields: left, .. }, Value::Record(right))
        | (Value::MappedRow { fields: left, .. }, Value::MappedRow { fields: right, .. }) => {
            value_named_fields_data_equal(left, right)
        }
        (Value::Row { .. }, _) | (_, Value::Row { .. }) => Err(Error::Evaluation(
            "unprojected runtime row reached Boon data equality".to_owned(),
        )),
        _ => Ok(left == right),
    }
}

fn eval_list_data_equal(left: &[EvalValue], right: &[EvalValue]) -> Result<bool, Error> {
    equal_length_and_all(left, right, eval_data_equal)
}

fn eval_ordered_list_data_equal(
    left: &[OrderedEvalItem],
    right: &[OrderedEvalItem],
) -> Result<bool, Error> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        if !eval_data_equal(&left.value, &right.value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn eval_list_ordered_data_equal(
    values: &[EvalValue],
    ordered: &[OrderedEvalItem],
) -> Result<bool, Error> {
    if values.len() != ordered.len() {
        return Ok(false);
    }
    for (value, ordered) in values.iter().zip(ordered) {
        if !eval_data_equal(value, &ordered.value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn value_list_data_equal(left: &[Value], right: &[Value]) -> Result<bool, Error> {
    equal_length_and_all(left, right, value_data_equal)
}

fn value_eval_list_data_equal(values: &[Value], evals: &[EvalValue]) -> Result<bool, Error> {
    equal_length_and_all(values, evals, value_eval_data_equal)
}

fn value_ordered_list_data_equal(
    values: &[Value],
    ordered: &[OrderedEvalItem],
) -> Result<bool, Error> {
    if values.len() != ordered.len() {
        return Ok(false);
    }
    for (value, ordered) in values.iter().zip(ordered) {
        if !value_eval_data_equal(value, &ordered.value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn eval_named_fields_data_equal(
    left: &BTreeMap<String, EvalValue>,
    right: &BTreeMap<String, EvalValue>,
) -> Result<bool, Error> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (name, left) in left {
        let Some(right) = right.get(name) else {
            return Ok(false);
        };
        if !eval_data_equal(left, right)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn value_named_fields_data_equal(
    left: &BTreeMap<String, Value>,
    right: &BTreeMap<String, Value>,
) -> Result<bool, Error> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (name, left) in left {
        let Some(right) = right.get(name) else {
            return Ok(false);
        };
        if !value_data_equal(left, right)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn value_eval_named_fields_data_equal(
    values: &BTreeMap<String, Value>,
    evals: &BTreeMap<String, EvalValue>,
) -> Result<bool, Error> {
    if values.len() != evals.len() {
        return Ok(false);
    }
    for (name, value) in values {
        let Some(eval) = evals.get(name) else {
            return Ok(false);
        };
        if !value_eval_data_equal(value, eval)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn equal_length_and_all<L, R>(
    left: &[L],
    right: &[R],
    equal: impl Fn(&L, &R) -> Result<bool, Error>,
) -> Result<bool, Error> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        if !equal(left, right)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn eval_to_numeric(value: &EvalValue) -> Result<FiniteReal, Error> {
    eval_to_number(value)
}

fn numeric_infix(left: FiniteReal, op: PlanInfixOp, right: FiniteReal) -> Result<Value, Error> {
    if matches!(op, PlanInfixOp::Divide | PlanInfixOp::Remainder) && right.get() == 0.0 {
        return Ok(Value::Error {
            code: if op == PlanInfixOp::Divide {
                "div_by_zero"
            } else {
                "mod_by_zero"
            }
            .to_owned(),
        });
    }
    if op.is_comparison() {
        return Ok(Value::Bool(numeric_compare(left, op, right)?));
    }
    let left = left.get();
    let right = right.get();
    let result = match op {
        PlanInfixOp::Add => left + right,
        PlanInfixOp::Subtract => left - right,
        PlanInfixOp::Multiply => left * right,
        PlanInfixOp::Divide => left / right,
        PlanInfixOp::Remainder => left % right,
        PlanInfixOp::Equal
        | PlanInfixOp::NotEqual
        | PlanInfixOp::Less
        | PlanInfixOp::LessOrEqual
        | PlanInfixOp::Greater
        | PlanInfixOp::GreaterOrEqual => unreachable!("comparisons return above"),
    };
    finite_number_result(result, "numeric operation").map(Value::Number)
}

fn numeric_compare(left: FiniteReal, op: PlanInfixOp, right: FiniteReal) -> Result<bool, Error> {
    let ordering = left.cmp(&right);
    match op {
        PlanInfixOp::Equal => Ok(ordering.is_eq()),
        PlanInfixOp::NotEqual => Ok(!ordering.is_eq()),
        PlanInfixOp::Greater => Ok(ordering.is_gt()),
        PlanInfixOp::GreaterOrEqual => Ok(ordering.is_ge()),
        PlanInfixOp::Less => Ok(ordering.is_lt()),
        PlanInfixOp::LessOrEqual => Ok(ordering.is_le()),
        PlanInfixOp::Add
        | PlanInfixOp::Subtract
        | PlanInfixOp::Multiply
        | PlanInfixOp::Divide
        | PlanInfixOp::Remainder => Err(Error::Evaluation(format!(
            "numeric operator `{op}` is not a comparison"
        ))),
    }
}

fn compare_update_values(
    left: &Value,
    operator: PlanInfixOp,
    right: &Value,
) -> Result<bool, Error> {
    match operator {
        PlanInfixOp::Equal | PlanInfixOp::NotEqual => {
            let equal = match (
                eval_to_numeric(&EvalValue::Value(left.clone())),
                eval_to_numeric(&EvalValue::Value(right.clone())),
            ) {
                (Ok(left), Ok(right)) => numeric_compare(left, PlanInfixOp::Equal, right)?,
                _ => value_data_equal(left, right)?,
            };
            Ok(if operator == PlanInfixOp::Equal {
                equal
            } else {
                !equal
            })
        }
        PlanInfixOp::Greater
        | PlanInfixOp::GreaterOrEqual
        | PlanInfixOp::Less
        | PlanInfixOp::LessOrEqual => {
            let left = eval_to_numeric(&EvalValue::Value(left.clone()))?;
            let right = eval_to_numeric(&EvalValue::Value(right.clone()))?;
            numeric_compare(left, operator, right)
        }
        PlanInfixOp::Add
        | PlanInfixOp::Subtract
        | PlanInfixOp::Multiply
        | PlanInfixOp::Divide
        | PlanInfixOp::Remainder => Err(Error::Evaluation(format!(
            "unsupported comparison `{operator}`"
        ))),
    }
}

fn select_pattern_matches(pattern: &PlanRowSelectPattern, value: &Value) -> bool {
    match pattern {
        PlanRowSelectPattern::Bool { value: expected } => value == &Value::Bool(*expected),
        PlanRowSelectPattern::Text { value: expected } => {
            value == &Value::Text(expected.clone())
                || tagged_value_label(value).is_some_and(|tag| tag == expected)
        }
        PlanRowSelectPattern::Number { value: expected } => value == &Value::Number(*expected),
        PlanRowSelectPattern::NaN => value == &Value::Text("NaN".to_owned()),
        PlanRowSelectPattern::Wildcard => true,
    }
}

fn tagged_value_label(value: &Value) -> Option<&str> {
    let value = value.visible();
    let Value::Record(fields) = value else {
        return None;
    };
    fields.get("$tag").and_then(|tag| match tag {
        Value::Text(tag) => Some(tag.as_str()),
        _ => None,
    })
}

fn effect_outcome_tag(value: &Value) -> Option<&str> {
    match value {
        Value::Text(tag) => Some(tag),
        _ => tagged_value_label(value),
    }
}

fn nonnegative_usize(value: i64, context: &str) -> Result<usize, Error> {
    usize::try_from(value)
        .map_err(|_| Error::Evaluation(format!("{context} {value} is negative or too large")))
}

fn validate_encoding(encoding: Option<&str>) -> Result<(), Error> {
    if encoding.is_none_or(|encoding| {
        matches!(
            encoding.to_ascii_lowercase().as_str(),
            "utf8" | "utf-8" | "text" | "ascii"
        )
    }) {
        Ok(())
    } else {
        Err(Error::Evaluation(format!(
            "unsupported text encoding `{}`",
            encoding.unwrap_or_default()
        )))
    }
}

fn checked_slice(bytes: &Bytes, offset: usize, count: usize) -> Result<Bytes, Error> {
    let end = offset
        .checked_add(count)
        .ok_or_else(|| Error::Evaluation("byte range overflow".to_owned()))?;
    if end > bytes.len() {
        return Err(Error::Evaluation(format!(
            "byte range {offset}..{end} exceeds length {}",
            bytes.len()
        )));
    }
    Ok(bytes.slice(offset..end))
}

fn read_integer(
    bytes: &Bytes,
    offset: usize,
    count: usize,
    endian: &str,
    signed: bool,
) -> Result<i64, Error> {
    if count == 0 || count > 8 {
        return Err(Error::Evaluation(format!(
            "integer byte count {count} is outside 1..=8"
        )));
    }
    let slice = checked_slice(bytes, offset, count)?;
    let little = parse_endian(endian)?;
    let mut value = 0u64;
    if little {
        for (shift, byte) in slice.iter().enumerate() {
            value |= u64::from(*byte) << (shift * 8);
        }
    } else {
        for byte in slice {
            value = (value << 8) | u64::from(byte);
        }
    }
    if signed && count < 8 && value & (1u64 << (count * 8 - 1)) != 0 {
        value |= u64::MAX << (count * 8);
    }
    Ok(value as i64)
}

fn write_integer(
    bytes: &mut [u8],
    offset: usize,
    count: usize,
    endian: &str,
    value: i64,
) -> Result<(), Error> {
    if count == 0 || count > 8 {
        return Err(Error::Evaluation(format!(
            "integer byte count {count} is outside 1..=8"
        )));
    }
    let end = offset
        .checked_add(count)
        .ok_or_else(|| Error::Evaluation("byte range overflow".to_owned()))?;
    let target_len = bytes.len();
    let target = bytes.get_mut(offset..end).ok_or_else(|| {
        Error::Evaluation(format!(
            "byte range {offset}..{end} exceeds length {target_len}"
        ))
    })?;
    let little = parse_endian(endian)?;
    for (index, byte) in target.iter_mut().enumerate() {
        let shift = if little { index } else { count - index - 1 } * 8;
        *byte = ((value as u64 >> shift) & 0xff) as u8;
    }
    Ok(())
}

fn parse_endian(endian: &str) -> Result<bool, Error> {
    match endian.to_ascii_lowercase().as_str() {
        "little" | "little_endian" | "le" => Ok(true),
        "big" | "big_endian" | "be" => Ok(false),
        _ => Err(Error::Evaluation(format!(
            "unsupported byte order `{endian}`"
        ))),
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(text: &str) -> Result<Bytes, Error> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return Err(Error::Evaluation(
            "hex input has an odd number of digits".to_owned(),
        ));
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Bytes::from)
}

fn hex_digit(digit: u8) -> Result<u8, Error> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(Error::Evaluation(format!(
            "invalid hex digit `{}`",
            digit as char
        ))),
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn decode_base64(text: &str) -> Result<Bytes, Error> {
    let bytes = text.trim().as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(Error::Evaluation(
            "base64 input length is not divisible by four".to_owned(),
        ));
    }
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let a = base64_digit(chunk[0])?;
        let b = base64_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            base64_digit(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            base64_digit(chunk[3])?
        };
        let value =
            (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        output.push((value >> 16) as u8);
        if chunk[2] != b'=' {
            output.push((value >> 8) as u8);
        }
        if chunk[3] != b'=' {
            output.push(value as u8);
        }
    }
    Ok(Bytes::from(output))
}

fn base64_digit(digit: u8) -> Result<u8, Error> {
    match digit {
        b'A'..=b'Z' => Ok(digit - b'A'),
        b'a'..=b'z' => Ok(digit - b'a' + 26),
        b'0'..=b'9' => Ok(digit - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::Evaluation(format!(
            "invalid base64 digit `{}`",
            digit as char
        ))),
    }
}

pub(crate) fn report_deltas(deltas: Vec<Delta>) -> Vec<Delta> {
    coalesce_deltas(deltas)
        .into_iter()
        .map(|delta| match delta {
            Delta::SetValue { target, value } => Delta::SetValue {
                target,
                value: value.into_visible_facade(),
            },
            Delta::SetDistributedImport { import_id, value } => Delta::SetDistributedImport {
                import_id,
                value: value.into_visible_facade(),
            },
            Delta::InsertRow { row } => Delta::InsertRow {
                row: report_row_snapshot(row),
            },
            delta @ (Delta::RemoveRow { .. }
            | Delta::BindSource { .. }
            | Delta::UnbindSource { .. }) => delta,
        })
        .collect()
}

fn report_authority_deltas(deltas: Vec<AuthorityDelta>) -> Vec<AuthorityDelta> {
    deltas
        .into_iter()
        .map(|delta| match delta {
            AuthorityDelta::SetRoot { state, value } => AuthorityDelta::SetRoot {
                state,
                value: value.into_visible_facade(),
            },
            AuthorityDelta::SetRowField {
                row,
                owner_ancestors,
                materialization_origin,
                field,
                value,
            } => AuthorityDelta::SetRowField {
                row,
                owner_ancestors,
                materialization_origin,
                field,
                value: value.into_visible_facade(),
            },
            AuthorityDelta::ReplaceList { list_id, authority } => AuthorityDelta::ReplaceList {
                list_id,
                authority: report_list_authority(authority),
            },
            AuthorityDelta::InsertRow {
                row,
                index,
                next_key,
            } => AuthorityDelta::InsertRow {
                row: report_row_authority(row),
                index,
                next_key,
            },
            delta @ AuthorityDelta::RemoveRow { .. } => delta,
        })
        .collect()
}

fn report_row_snapshot(row: RowSnapshot) -> RowSnapshot {
    RowSnapshot {
        id: row.id,
        fields: row
            .fields
            .into_iter()
            .map(|(field, value)| (field, value.into_visible_facade()))
            .collect(),
    }
}

fn report_row_authority(row: RowAuthority) -> RowAuthority {
    RowAuthority {
        id: row.id,
        source_order_token: row.source_order_token,
        owner_ancestors: row.owner_ancestors,
        materialization_origin: row.materialization_origin,
        fields: row
            .fields
            .into_iter()
            .map(|(field, value)| (field, value.into_visible_facade()))
            .collect(),
        touched_fields: row.touched_fields,
    }
}

fn report_list_authority(authority: ListAuthority) -> ListAuthority {
    ListAuthority {
        touched: authority.touched,
        revision: authority.revision,
        next_key: authority.next_key,
        next_order_token: authority.next_order_token,
        rows: authority
            .rows
            .into_iter()
            .map(report_row_authority)
            .collect(),
    }
}

fn coalesce_deltas(deltas: Vec<Delta>) -> Vec<Delta> {
    let mut output = Vec::with_capacity(deltas.len());
    let mut positions = BTreeMap::<ValueTarget, usize>::new();
    let mut import_positions = BTreeMap::<ImportId, usize>::new();
    for delta in deltas {
        match delta {
            Delta::SetValue { target, value } => {
                if let Some(position) = positions.get(&target).copied() {
                    output[position] = Delta::SetValue { target, value };
                } else {
                    positions.insert(target, output.len());
                    output.push(Delta::SetValue { target, value });
                }
            }
            Delta::SetDistributedImport { import_id, value } => {
                if let Some(position) = import_positions.get(&import_id).copied() {
                    output[position] = Delta::SetDistributedImport { import_id, value };
                } else {
                    import_positions.insert(import_id, output.len());
                    output.push(Delta::SetDistributedImport { import_id, value });
                }
            }
            other => output.push(other),
        }
    }
    output
}

fn source_payload_value(payload: &SourcePayload, field: &SourcePayloadField) -> Option<Value> {
    match field {
        SourcePayloadField::Address => payload.address.clone().map(Value::Text),
        SourcePayloadField::Key => payload.key.clone().map(Value::Text),
        SourcePayloadField::Text => payload.text.clone().map(Value::Text),
        SourcePayloadField::Named(name) => payload.fields.get(name).cloned(),
        SourcePayloadField::Bytes => payload
            .fields
            .get("bytes")
            .or_else(|| payload.fields.get("Bytes"))
            .cloned(),
    }
}

pub(crate) fn project_value<'a>(value: &'a Value, field_path: &[String]) -> Option<&'a Value> {
    let mut value = value;
    for field in field_path {
        value = match value {
            Value::Record(fields) | Value::MappedRow { fields, .. } => fields.get(field)?,
            _ => return None,
        };
    }
    Some(value)
}

fn effect_replay_is_transient(replay: &boon_plan::EffectReplay) -> bool {
    matches!(
        replay,
        boon_plan::EffectReplay::ReadOnly | boon_plan::EffectReplay::ProcessScoped
    )
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    use boon_plan::{ProgramRole, TargetProfile};

    #[test]
    fn transient_effect_call_id_diagnostics_are_opaque() {
        let call_id = TransientEffectCallId {
            launch_epoch: 8_675_309,
            sequence: 4_294_967_291,
        };

        for diagnostic in [format!("{call_id:?}"), format!("{call_id}")] {
            assert_eq!(diagnostic, "TransientEffectCallId(..)");
            assert!(!diagnostic.contains("8675309"));
            assert!(!diagnostic.contains("4294967291"));
        }
    }

    #[test]
    fn runtime_turn_diagnostics_do_not_expose_distributed_payloads() {
        const SENTINEL: &str = "turn-secret-82be7a";
        let invocation = DistributedInvocation {
            call_site_id: RemoteCallSiteId([0xb1; 32]),
            call_instance_id: DistributedCallInstanceId([0xb2; 32]),
            arguments: BTreeMap::from([(
                DistributedArgumentId([0xb3; 32]),
                Value::Text(SENTINEL.to_owned()),
            )]),
            result_route: SourceRouteToken::new(
                0xb4,
                OwnerInstanceId::root(),
                SourceId(0xb5),
                0xb6,
            )
            .unwrap(),
        };
        assert_eq!(format!("{invocation:?}"), "DistributedInvocation(..)");

        let turn = Turn {
            sequence: 1,
            source_sequence: Some(2),
            deltas: Vec::new(),
            authority_deltas: Vec::new(),
            durable_changes: Vec::new(),
            outbox_changes: Vec::new(),
            transient_effects: Vec::new(),
            cancelled_transient_effects: Vec::new(),
            transient_effect_credit_grants: Vec::new(),
            distributed_invocations: vec![invocation],
            metrics: TurnMetrics::default(),
        };
        let diagnostic = format!("{turn:?}");
        assert!(diagnostic.contains("distributed_invocation_count: 1"));
        for hidden in [SENTINEL, "b1b1", "b2b2", "b3b3", "181", "182"] {
            assert!(!diagnostic.contains(hidden), "leaked `{hidden}`");
        }
    }

    fn order_test_row(key: u64) -> RowId {
        RowId {
            list: ListId(91),
            key,
            generation: 1,
        }
    }

    fn hard_avl_height_bound(blocks: usize) -> u16 {
        if blocks == 0 {
            return 0;
        }
        let ceil_log2 = usize::BITS - blocks.leading_zeros();
        u16::try_from(ceil_log2.saturating_mul(2).saturating_add(1)).unwrap()
    }

    fn assert_order_valid_and_bounded(order: &ListOrder) {
        order.validate().unwrap();
        let height = order
            .root
            .and_then(|root| order.blocks.get(&root))
            .map(|state| state.height)
            .unwrap_or(0);
        assert!(height <= hard_avl_height_bound(order.blocks.len()));
        assert!(height <= ListOrder::MAX_TREE_HEIGHT);
    }

    fn list_with_rows(row_count: u64) -> ListState {
        let mut list = ListState::default();
        for key in 1..=row_count {
            let row = order_test_row(key);
            list.rows.insert(row, Row::default());
            list.push_ordered(row).unwrap();
        }
        list
    }

    #[test]
    fn sixty_thousand_row_middle_mutations_have_bounded_location_updates() {
        const ROW_COUNT: u64 = 60_000;
        let original = (1..=ROW_COUNT).map(order_test_row).collect::<Vec<_>>();
        let mut order = ListOrder::from_rows(original.clone()).unwrap();
        assert_eq!(order.len(), ROW_COUNT as usize);
        assert_eq!(order.get(0), original.first());
        assert_eq!(order.get(30_000), original.get(30_000));
        assert_eq!(order.get(59_999), original.last());
        assert_order_valid_and_bounded(&order);

        let mut inserted = Vec::new();
        let mut maximum_location_updates = 0_usize;
        for offset in 0..1_024_u64 {
            let row = order_test_row(ROW_COUNT + offset + 1);
            let maintenance = order.insert(30_000, row).unwrap();
            maximum_location_updates =
                maximum_location_updates.max(maintenance.row_location_update_count);
            assert!(
                maintenance.row_location_update_count <= ListOrder::MAX_BLOCK_ROWS * 2 + 1,
                "one insertion rewrote {} row locations",
                maintenance.row_location_update_count
            );
            assert!(
                maintenance.tree_visit_max
                    <= usize::from(hard_avl_height_bound(order.blocks.len())).saturating_mul(4),
                "one tree operation visited {} blocks",
                maintenance.tree_visit_max
            );
            assert_eq!(order.get(30_000), Some(&row));
            assert_eq!(order.position(row), Some(30_000));
            inserted.push(row);
        }
        assert!(maximum_location_updates < ROW_COUNT as usize);

        for row in inserted.into_iter().rev() {
            let (_, maintenance) = order.remove(row).unwrap().unwrap();
            maximum_location_updates =
                maximum_location_updates.max(maintenance.row_location_update_count);
            assert!(
                maintenance.row_location_update_count <= ListOrder::MAX_BLOCK_ROWS * 2 + 1,
                "one removal rewrote {} row locations",
                maintenance.row_location_update_count
            );
            assert!(
                maintenance.tree_visit_max
                    <= usize::from(hard_avl_height_bound(order.blocks.len())).saturating_mul(4)
            );
        }
        assert_eq!(order.to_vec(), original);
        assert_eq!(order.locations.len(), ROW_COUNT as usize);
        assert!(maximum_location_updates < ROW_COUNT as usize);
        assert_order_valid_and_bounded(&order);
    }

    #[test]
    fn exhausted_source_order_gap_relabels_only_a_bounded_window() {
        const ROW_COUNT: u64 = 600;
        let mut list = list_with_rows(ROW_COUNT);
        let insertion_index = 300_usize;
        let lower_row = *list.order.get(insertion_index - 1).unwrap();
        let upper_row = *list.order.get(insertion_index).unwrap();
        let lower = list.order_token(lower_row).unwrap();
        list.order_tokens.insert(upper_row, lower + 1);

        let moved = order_test_row(ROW_COUNT);
        let old_order = list.order.to_vec();
        let old_tokens = list.order_tokens.clone();
        let old_next_order_token = list.next_order_token;
        let (changed, maintenance, undo) = list.reorder_rows(insertion_index, &[moved]).unwrap();
        assert!(changed);
        assert_eq!(list.order.get(insertion_index), Some(&moved));
        assert!(maintenance.relabel_operation_count >= 1);
        assert!(maintenance.relabel_window_max <= ListState::MAX_RELABEL_ROWS);
        assert!(maintenance.relabeled_rows.len() <= ListState::MAX_RELABEL_ROWS + 1);
        assert!(maintenance.changed_order_rows.len() <= ListState::MAX_RELABEL_ROWS + 1);
        assert!(maintenance.relabeled_rows.contains(&moved));
        assert!(maintenance.relabeled_rows.len() < ROW_COUNT as usize);

        let tokens = list
            .order
            .iter()
            .map(|row| list.order_token(*row).unwrap())
            .collect::<Vec<_>>();
        assert!(tokens.windows(2).all(|tokens| tokens[0] < tokens[1]));
        let mut expected = old_order.clone();
        expected.retain(|row| *row != moved);
        expected.insert(insertion_index, moved);
        assert_eq!(list.order.to_vec(), expected);

        list.restore_source_order(&undo).unwrap();
        list.restore_source_order(&undo).unwrap();
        assert_eq!(list.order.to_vec(), old_order);
        assert_eq!(list.order_tokens, old_tokens);
        assert_eq!(list.next_order_token, old_next_order_token);
        assert_order_valid_and_bounded(&list.order);
    }

    #[test]
    fn dense_restored_labels_stay_canonical_and_reject_unbounded_middle_maintenance_atomically() {
        const ROW_COUNT: u64 = 600;
        let order = (1..=ROW_COUNT).map(order_test_row).collect::<Vec<_>>();
        let rows = order
            .iter()
            .copied()
            .map(|row| (row, Row::default()))
            .collect();
        let tokens = order
            .iter()
            .copied()
            .enumerate()
            .map(|(offset, row)| (row, u128::try_from(offset + 1).unwrap()))
            .collect();
        let mut list = ListState::from_authority(
            rows,
            order,
            tokens,
            u128::from(ROW_COUNT + 1),
            ROW_COUNT + 1,
            7,
        )
        .unwrap();
        assert_order_valid_and_bounded(&list.order);

        let appended = order_test_row(ROW_COUNT + 1);
        let prepared = list.prepare_push_ordered(appended, None).unwrap();
        prepared.commit(&mut list);
        list.rows.insert(appended, Row::default());
        assert_eq!(list.order.get(ROW_COUNT as usize), Some(&appended));

        let old_order = list.order.to_vec();
        let old_tokens = list.order_tokens.clone();
        let old_next_order_token = list.next_order_token;
        let old_structure = list.order.clone();
        let error = list
            .prepare_reorder_rows(300, &[appended], None)
            .err()
            .expect("dense middle labels must not trigger a global rebase");
        assert!(
            error
                .to_string()
                .contains("bounded 256-row maintenance window")
        );
        assert_eq!(list.order.to_vec(), old_order);
        assert_eq!(list.order, old_structure);
        assert_eq!(list.order_tokens, old_tokens);
        assert_eq!(list.next_order_token, old_next_order_token);
    }

    #[test]
    fn end_reorder_rollback_restores_the_advanced_allocator_idempotently() {
        const ROW_COUNT: u64 = 600;
        let mut list = list_with_rows(ROW_COUNT);
        let moved = order_test_row(1);
        let old_order = list.order.to_vec();
        let old_tokens = list.order_tokens.clone();
        let old_next_order_token = list.next_order_token;

        let (changed, _, undo) = list.reorder_rows(usize::MAX, &[moved]).unwrap();
        assert!(changed);
        assert_eq!(list.order.get(ROW_COUNT as usize - 1), Some(&moved));
        assert!(list.next_order_token > old_next_order_token);

        list.restore_source_order(&undo).unwrap();
        list.restore_source_order(&undo).unwrap();
        assert_eq!(list.order.to_vec(), old_order);
        assert_eq!(list.order_tokens, old_tokens);
        assert_eq!(list.next_order_token, old_next_order_token);
    }

    #[test]
    fn repeated_same_gap_pressure_has_bounded_relabels_and_atomic_exhaustion() {
        const ROW_COUNT: u64 = 2_048;
        let mut list = list_with_rows(ROW_COUNT);
        let insertion_index = 1_024;
        let mut completed = 0_usize;
        for _ in 0..1_024 {
            let moved = *list.order.get(list.order.len() - 1).unwrap();
            let before_order = list.order.clone();
            let before_tokens = list.order_tokens.clone();
            let before_next = list.next_order_token;
            match list.prepare_reorder_rows(insertion_index, &[moved], None) {
                Ok(Some(prepared)) => {
                    assert!(
                        prepared.maintenance.changed_order_rows.len()
                            <= ListState::MAX_RELABEL_ROWS + 1
                    );
                    assert!(
                        prepared.maintenance.relabeled_rows.len()
                            <= ListState::MAX_RELABEL_ROWS + 1
                    );
                    prepared.commit(&mut list);
                    assert_eq!(list.order.get(insertion_index), Some(&moved));
                    assert_order_valid_and_bounded(&list.order);
                    completed += 1;
                }
                Err(error) => {
                    assert!(
                        error
                            .to_string()
                            .contains("bounded 256-row maintenance window")
                    );
                    assert_eq!(list.order, before_order);
                    assert_eq!(list.order_tokens, before_tokens);
                    assert_eq!(list.next_order_token, before_next);
                    break;
                }
                Ok(None) => panic!("same-gap move unexpectedly became a no-op"),
            }
        }
        assert!(
            completed >= 64,
            "only {completed} concentrated moves succeeded"
        );
    }

    #[test]
    fn priority_like_churn_histories_keep_a_deterministic_hard_avl_bound() {
        let original = (1..=20_000).map(order_test_row).collect::<Vec<_>>();
        let mut order = ListOrder::from_rows(original).unwrap();
        let mut inserted = Vec::new();
        let mut random = 0x9e37_79b9_7f4a_7c15_u64;
        for offset in 0..4_096_u64 {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let index = usize::try_from(random % u64::try_from(order.len()).unwrap()).unwrap();
            let row = order_test_row(30_000 + offset);
            let maintenance = order.insert(index, row).unwrap();
            assert!(
                maintenance.tree_visit_max
                    <= usize::from(hard_avl_height_bound(order.blocks.len())).saturating_mul(4)
            );
            inserted.push(row);
            if inserted.len() > 512 {
                let removed = inserted.remove(0);
                order.remove(removed).unwrap().unwrap();
            }
            if offset % 128 == 0 {
                assert_order_valid_and_bounded(&order);
            }
        }
        assert_order_valid_and_bounded(&order);
    }

    #[test]
    fn allocator_split_and_finalize_failures_leave_canonical_order_unchanged() {
        let rows = (1..=ListOrder::MAX_BLOCK_ROWS as u64)
            .map(order_test_row)
            .collect::<Vec<_>>();
        let mut order = ListOrder::default();
        for row in rows {
            order.push(row).unwrap();
        }
        assert_eq!(order.blocks.len(), 1);
        let inserted = order_test_row(90_000);
        for fault in [
            ListOrderFaultPoint::BlockSplit,
            ListOrderFaultPoint::BlockAllocation,
            ListOrderFaultPoint::TreeRebalance,
            ListOrderFaultPoint::Finalize,
        ] {
            let before = order.clone();
            assert!(order.prepare_insert(128, inserted, Some(fault)).is_err());
            assert_eq!(order, before, "fault {fault:?} changed canonical order");
        }
        let prepared = order.prepare_insert(128, inserted, None).unwrap();
        prepared.commit(&mut order);
        assert_eq!(order.get(128), Some(&inserted));
        assert_order_valid_and_bounded(&order);
    }

    #[test]
    fn exhausted_allocators_fail_before_authority_changes() {
        let mut order = ListOrder::default();
        for key in 1..=ListOrder::MAX_BLOCK_ROWS as u64 {
            order.push(order_test_row(key)).unwrap();
        }
        order.next_block = u64::MAX;
        let before = order.clone();
        assert!(order.prepare_push(order_test_row(79_999), None).is_err());
        assert_eq!(order, before);
        assert!(
            order
                .prepare_insert(128, order_test_row(80_000), None)
                .is_err()
        );
        assert_eq!(order, before);

        let mut list = list_with_rows(1);
        list.next_order_token = u128::MAX;
        let before_order = list.order.clone();
        let before_tokens = list.order_tokens.clone();
        assert!(list.prepare_push_ordered(order_test_row(2), None).is_err());
        assert_eq!(list.order, before_order);
        assert_eq!(list.order_tokens, before_tokens);
        assert_eq!(list.next_order_token, u128::MAX);
    }

    #[test]
    fn source_order_rollback_is_exact_when_committed_and_noop_when_never_inserted() {
        let mut list = list_with_rows(32);
        let row = order_test_row(33);
        let prepared = list.prepare_push_ordered(row, None).unwrap();
        let never_committed = SourceOrderUndo {
            patch: Some(prepared.patch.clone()),
        };
        let before_order = list.order.clone();
        let before_tokens = list.order_tokens.clone();
        let before_next = list.next_order_token;
        list.restore_source_order(&never_committed).unwrap();
        list.restore_source_order(&never_committed).unwrap();
        assert_eq!(list.order, before_order);
        assert_eq!(list.order_tokens, before_tokens);
        assert_eq!(list.next_order_token, before_next);

        let undo = prepared.commit(&mut list);
        assert_eq!(list.order.get(32), Some(&row));
        list.restore_source_order(&undo).unwrap();
        list.restore_source_order(&undo).unwrap();
        assert_eq!(list.order, before_order);
        assert_eq!(list.order_tokens, before_tokens);
        assert_eq!(list.next_order_token, before_next);
    }

    #[test]
    fn changed_order_keys_stage_precise_bounded_index_dirty_fanout() {
        let compiled = boon_compiler::compile_source_text_to_machine_plan_for_role(
            "source-order-dirty-fanout-internal.bn",
            r#"
store: [
    items:
        List/range(from: 0, to: 599)
        |> List/map(item, new: [a: item, b: item + 1, c: item + 2])
    by_a:
        items
        |> List/sort_by(item, key: item.a)
        |> List/take(count: 20)
    by_b:
        items
        |> List/sort_by(item, key: item.b)
        |> List/take(count: 20)
    by_c:
        items
        |> List/sort_by(item, key: item.c)
        |> List/take(count: 20)
]
outputs: [
    a: store.by_a
    b: store.by_b
    c: store.by_c
]
"#,
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
        )
        .unwrap();
        let source = compiled
            .plan
            .list_indexes
            .first()
            .map(|index| index.source_list)
            .expect("fixture has typed source-order indexes");
        assert!(
            compiled
                .plan
                .list_indexes
                .iter()
                .all(|index| index.source_list == source)
        );
        let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
        let changed = session
            .list_row_ids(source)
            .into_iter()
            .take(ListState::MAX_RELABEL_ROWS + 1)
            .collect::<Vec<_>>();
        assert_eq!(changed.len(), ListState::MAX_RELABEL_ROWS + 1);
        let index_count = session
            .metadata
            .ordered_indexes_by_list
            .get(&source)
            .map(BTreeSet::len)
            .unwrap_or_default();
        assert!(index_count >= 3);
        assert!(
            index_count
                <= TargetProfile::SoftwareDefault
                    .typed_list_index_limits()
                    .max_indexes_per_list
        );
        let before = session.dirty_ordered_index_rows.clone();
        let prepared =
            session.prepare_ordered_index_rows_dirty_for_list(source, changed.iter().copied());
        assert_eq!(prepared.fanout, changed.len() * index_count);
        assert!(
            prepared
                .rows
                .values()
                .all(|rows| rows.len() == changed.len())
        );
        assert!(
            prepared.fanout
                <= (ListState::MAX_RELABEL_ROWS + 1)
                    * TargetProfile::SoftwareDefault
                        .typed_list_index_limits()
                        .max_indexes_per_list
        );

        let mut rejected = Work::with_limit(Some(
            u64::try_from(prepared.fanout.saturating_sub(1)).unwrap(),
        ));
        rejected.begin_turn(None, 0);
        assert!(matches!(
            prepared.charge(&mut rejected),
            Err(Error::WorkBudgetExceeded { .. })
        ));
        assert_eq!(session.dirty_ordered_index_rows, before);

        let mut accepted = Work::with_limit(Some(u64::try_from(prepared.fanout).unwrap()));
        accepted.begin_turn(None, 0);
        prepared.charge(&mut accepted).unwrap();
        let fanout = prepared.fanout;
        session.commit_ordered_index_dirty(prepared);
        assert_eq!(accepted.work_units, u64::try_from(fanout).unwrap());
        assert_eq!(
            session
                .dirty_ordered_index_rows
                .values()
                .map(BTreeSet::len)
                .sum::<usize>(),
            fanout
        );
    }

    #[test]
    fn removing_an_owner_row_retires_exact_generation_descendants_deepest_first() {
        let plan = boon_compiler::compile_source_text_to_machine_plan_for_role(
            "owner-cascade-internal.bn",
            r#"
store: [
    seed: SOURCE
    parents:
        LIST {}
        |> List/append(item: seed |> THEN { [value: TEXT { parent }] })
    children:
        LIST {}
        |> List/append(item: seed |> THEN { [value: TEXT { child }] })
    grandchildren:
        LIST {}
        |> List/append(item: seed |> THEN { [value: TEXT { grandchild }] })
]
outputs: [
    parents: store.parents
    children: store.children
    grandchildren: store.grandchildren
]
"#,
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
        )
        .unwrap()
        .plan;
        let list_ids = plan
            .storage_layout
            .list_slots
            .iter()
            .map(|slot| slot.list_id)
            .collect::<Vec<_>>();
        assert_eq!(list_ids.len(), 3);
        let mut session = MachineInstance::new(plan.clone(), SessionOptions::default()).unwrap();
        let mut work = session.fresh_work();
        work.begin_turn(None, 0);
        let parent = session
            .append_row_with_owner_prefix(list_ids[0], BTreeMap::new(), &[], None, &mut work)
            .unwrap();
        let parent_owner = session.row_owner_ancestors(parent).unwrap().to_vec();
        let child = session
            .append_row_with_owner_prefix(
                list_ids[1],
                BTreeMap::new(),
                &parent_owner,
                None,
                &mut work,
            )
            .unwrap();
        let child_owner = session.row_owner_ancestors(child).unwrap().to_vec();
        let grandchild = session
            .append_row_with_owner_prefix(
                list_ids[2],
                BTreeMap::new(),
                &child_owner,
                None,
                &mut work,
            )
            .unwrap();

        session.remove_row(parent, &mut work).unwrap();

        assert!(
            list_ids
                .iter()
                .all(|list| session.list_rows(*list).is_empty())
        );
        let removed = work
            .deltas
            .iter()
            .filter_map(|delta| match delta {
                Delta::RemoveRow { row } => Some(*row),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(removed, vec![grandchild, child, parent]);
    }

    #[test]
    fn materialized_list_reconciliation_is_partitioned_by_exact_parent_owner() {
        let plan = boon_compiler::compile_source_text_to_machine_plan_for_role(
            "owner-partitioned-materialization-internal.bn",
            r#"
store: [
    seed: SOURCE
    parents:
        LIST {}
        |> List/append(item: seed |> THEN { [value: TEXT { parent }] })
    children:
        LIST {}
        |> List/append(item: seed |> THEN { [value: TEXT { child }] })
]
"#,
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
        )
        .unwrap()
        .plan;
        let parent_list = plan.storage_layout.list_slots[0].list_id;
        let origin_list = parent_list;
        let child_slot = plan.storage_layout.list_slots[1].clone();
        let child_list = child_slot.list_id;
        let child_fields = child_slot
            .row_fields
            .iter()
            .map(|field| (field.name.clone(), field.field_id))
            .collect::<BTreeMap<_, _>>();
        let value_field = child_fields["value"];
        let item = |id, value: &str| EvalValue::MappedRow {
            id,
            fields: BTreeMap::from([(
                "value".to_owned(),
                EvalValue::Value(Value::Text(value.to_owned())),
            )]),
            captures: BTreeMap::new(),
        };

        let mut session = MachineInstance::new(plan.clone(), SessionOptions::default()).unwrap();
        let mut work = session.fresh_work();
        work.begin_turn(None, 0);
        let parent_a = session
            .append_row_with_owner_prefix(parent_list, BTreeMap::new(), &[], None, &mut work)
            .unwrap();
        let parent_b = session
            .append_row_with_owner_prefix(parent_list, BTreeMap::new(), &[], None, &mut work)
            .unwrap();
        let owner_a = session.row_owner_ancestors(parent_a).unwrap().to_vec();
        let owner_b = session.row_owner_ancestors(parent_b).unwrap().to_vec();
        let origin_a1 = session
            .append_row_with_owner_prefix(origin_list, BTreeMap::new(), &[], None, &mut work)
            .unwrap();
        let origin_a2 = session
            .append_row_with_owner_prefix(origin_list, BTreeMap::new(), &[], None, &mut work)
            .unwrap();
        let origin_b1 = session
            .append_row_with_owner_prefix(origin_list, BTreeMap::new(), &[], None, &mut work)
            .unwrap();

        session
            .reconcile_materialized_list(
                child_list,
                None,
                &child_fields,
                &[],
                EvalValue::List(vec![item(origin_a1, "a-1"), item(origin_a2, "a-2")]),
                &owner_a,
                None,
                None,
                &mut work,
            )
            .unwrap();
        let initial_a = session
            .list_row_ids_for_owner(child_list, &owner_a)
            .unwrap();
        assert_eq!(initial_a.len(), 2);
        let reordered = session
            .reconcile_materialized_list(
                child_list,
                None,
                &child_fields,
                &[],
                EvalValue::List(vec![item(origin_a2, "a-2"), item(origin_a1, "a-1")]),
                &owner_a,
                None,
                None,
                &mut work,
            )
            .unwrap();
        assert_eq!(
            reordered,
            EvalValue::List(vec![
                EvalValue::Row(initial_a[1]),
                EvalValue::Row(initial_a[0]),
            ])
        );
        session
            .reconcile_materialized_list(
                child_list,
                None,
                &child_fields,
                &[],
                EvalValue::List(vec![item(origin_b1, "b-1")]),
                &owner_b,
                None,
                None,
                &mut work,
            )
            .unwrap();
        let rows_b = session
            .list_row_ids_for_owner(child_list, &owner_b)
            .unwrap();
        assert_eq!(rows_b.len(), 1);

        let result = session
            .reconcile_materialized_list(
                child_list,
                None,
                &child_fields,
                &[],
                EvalValue::List(vec![item(origin_a2, "a-replaced")]),
                &owner_a,
                None,
                None,
                &mut work,
            )
            .unwrap();
        let rows_a = session
            .list_row_ids_for_owner(child_list, &owner_a)
            .unwrap();
        assert_eq!(result, EvalValue::List(vec![EvalValue::Row(rows_a[0])]));
        assert_eq!(rows_a.len(), 1);
        assert_eq!(rows_a[0], initial_a[1]);
        assert_eq!(
            session
                .list_row_ids_for_owner(child_list, &owner_b)
                .unwrap(),
            rows_b
        );
        assert_eq!(
            session.row_snapshot(rows_a[0]).unwrap().fields[&value_field],
            Value::Text("a-replaced".to_owned())
        );
        assert_eq!(
            session.row_snapshot(rows_b[0]).unwrap().fields[&value_field],
            Value::Text("b-1".to_owned())
        );

        let durable = session.durable_restore_image(1, BTreeSet::new()).unwrap();
        let stored_children = durable
            .lists
            .get(&session.persistence_list(child_list).unwrap().memory_id)
            .unwrap();
        assert!(
            stored_children
                .rows
                .iter()
                .all(|row| row.owner.ancestors.len() == 2)
        );
        let restored = MachineInstanceBuilder::new(plan, SessionOptions::default())
            .unwrap()
            .restore_durable(durable)
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            restored
                .list_row_ids_for_owner(child_list, &owner_a)
                .unwrap(),
            rows_a
        );
        assert_eq!(
            restored
                .list_row_ids_for_owner(child_list, &owner_b)
                .unwrap(),
            rows_b
        );
    }

    #[test]
    fn equal_sequence_values_for_one_mutation_site_fail_before_authority_commit() {
        let plan = boon_compiler::compile_source_text_to_machine_plan_for_role(
            "list-mutation-tie-internal.bn",
            r#"
store: [
    seed: SOURCE
    rows:
        LIST {}
        |> List/append(item: seed |> THEN { [value: TEXT { row }] })
]
"#,
            TargetProfile::SoftwareDefault,
            ProgramRole::Server,
        )
        .unwrap()
        .plan;
        let list = plan.storage_layout.list_slots[0].list_id;
        let mut session = MachineInstance::new(plan, SessionOptions::default()).unwrap();
        let mut work = session.fresh_work();
        work.begin_turn(None, 1);
        let mutation = |ordinal| PendingListMutation::Append {
            site: 7,
            ordinal,
            sequence: 1,
            owner: OwnerInstanceId::root(),
            list,
            fields: BTreeMap::new(),
        };
        work.pending_list_mutations.push(mutation(0));
        work.pending_list_mutations.push(mutation(1));

        let error = session
            .commit_pending_list_mutations(&mut work)
            .expect_err("equal-sequence values from one site must be explicit");

        assert!(
            error
                .to_string()
                .contains("use explicit PRIORITY or proven EXCLUSIVE"),
            "unexpected arbitration error: {error}"
        );
        assert!(session.list_rows(list).is_empty());
    }
}
