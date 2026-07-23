use crate::machine::{RowId as RuntimeRowId, Value};
use boon_list_access::{
    ClosedTag, RowId, SourceOrderToken, StructuralKey, StructuralValue, TagTypeId,
};
use boon_plan::FieldId;
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

const TOKEN_VERSION: u8 = 5;
const PAYLOAD_MAGIC: &[u8; 4] = b"BPGC";
const NONCE_BYTES: usize = 12;
const AEAD_TAG_BYTES: usize = 16;
const MAX_CURSOR_BYTES: usize = 4_096;
const TOKEN_AAD: &[u8] = b"boon.page-cursor.token.v5\0";

#[derive(Clone, Eq, PartialEq)]
pub struct CursorSealingKey([u8; 32]);

impl CursorSealingKey {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for CursorSealingKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CursorSealingKey(<opaque>)")
    }
}

/// Host-private identity for the Session, tenant, and authorization scope in
/// which a cursor is valid. The bytes are hashed into the cursor binding and
/// are never serialized into Boon data or diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct CursorScopeFingerprint([u8; 32]);

impl CursorScopeFingerprint {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for CursorScopeFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CursorScopeFingerprint(<opaque>)")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PageCursor {
    pub view_fingerprint: [u8; 32],
    pub authority_revision: u64,
    pub capture_fingerprint: [u8; 32],
    pub accepted_offset: u64,
    pub semantic_key: StructuralKey,
    pub source_order: SourceOrderToken,
    pub row_id: RowId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CursorSemanticRowId {
    list_memory_id: [u8; 32],
    list_type_fingerprint: [u8; 32],
    row_key: u64,
    row_generation: u64,
}

impl CursorSemanticRowId {
    pub(crate) const fn new(
        list_memory_id: [u8; 32],
        list_type_fingerprint: [u8; 32],
        row_key: u64,
        row_generation: u64,
    ) -> Self {
        Self {
            list_memory_id,
            list_type_fingerprint,
            row_key,
            row_generation,
        }
    }
}

pub(crate) trait CursorIdentityResolver {
    fn semantic_row_id(&self, row: RuntimeRowId) -> Option<CursorSemanticRowId>;

    fn semantic_row_field_id(&self, row: RuntimeRowId, field: FieldId) -> Option<[u8; 32]>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorError {
    Invalid,
    TooLarge,
    Randomness,
}

pub(crate) fn seal_cursor(
    key: &CursorSealingKey,
    cursor: &PageCursor,
) -> Result<Vec<u8>, CursorError> {
    let payload = encode_payload(cursor)?;
    let mut nonce = [0_u8; NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|_| CursorError::Randomness)?;
    let encryption_key = derive_encryption_key(key);
    let cipher = ChaCha20Poly1305::new((&encryption_key).into());
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &payload,
                aad: TOKEN_AAD,
            },
        )
        .map_err(|_| CursorError::Invalid)?;

    let mut token = Vec::with_capacity(1 + NONCE_BYTES + ciphertext.len());
    token.push(TOKEN_VERSION);
    token.extend_from_slice(&nonce);
    token.extend_from_slice(&ciphertext);
    if token.len() > MAX_CURSOR_BYTES {
        return Err(CursorError::TooLarge);
    }
    Ok(token)
}

pub(crate) fn open_cursor(key: &CursorSealingKey, token: &[u8]) -> Result<PageCursor, CursorError> {
    if token.len() < 1 + NONCE_BYTES + AEAD_TAG_BYTES
        || token.len() > MAX_CURSOR_BYTES
        || token[0] != TOKEN_VERSION
    {
        return Err(CursorError::Invalid);
    }
    let nonce: [u8; NONCE_BYTES] = token[1..1 + NONCE_BYTES]
        .try_into()
        .map_err(|_| CursorError::Invalid)?;
    let encryption_key = derive_encryption_key(key);
    let cipher = ChaCha20Poly1305::new((&encryption_key).into());
    let payload = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &token[1 + NONCE_BYTES..],
                aad: TOKEN_AAD,
            },
        )
        .map_err(|_| CursorError::Invalid)?;
    decode_payload(&payload)
}

pub(crate) fn capture_fingerprint<'a>(
    view_fingerprint: [u8; 32],
    ephemeral_launch_epoch: Option<u64>,
    host_scope: Option<&CursorScopeFingerprint>,
    owner_scope: &[RuntimeRowId],
    principal_scope: &Value,
    captures: impl IntoIterator<Item = &'a Value>,
    identities: &impl CursorIdentityResolver,
) -> Result<[u8; 32], CursorError> {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.page-cursor.capture.v5\0");
    hasher.update(view_fingerprint);
    match ephemeral_launch_epoch {
        Some(launch_epoch) => {
            hasher.update([1]);
            hasher.update(launch_epoch.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    match host_scope {
        Some(scope) => {
            hasher.update([2]);
            hasher.update(scope.as_bytes());
        }
        None => hasher.update([3]),
    }
    hasher.update((owner_scope.len() as u64).to_be_bytes());
    for owner in owner_scope {
        hash_semantic_row_id(
            &mut hasher,
            identities
                .semantic_row_id(*owner)
                .ok_or(CursorError::Invalid)?,
        );
    }
    hasher.update([4]);
    hash_value(&mut hasher, principal_scope, identities)?;
    let mut capture_count = 0_u64;
    for capture in captures {
        hasher.update([5]);
        hasher.update(capture_count.to_be_bytes());
        hash_value(&mut hasher, capture, identities)?;
        capture_count = capture_count.saturating_add(1);
    }
    hasher.update([6]);
    hasher.update(capture_count.to_be_bytes());
    Ok(hasher.finalize().into())
}

pub(crate) fn bounded_page_position(
    accepted_offset: u64,
    value: &Value,
    identities: &impl CursorIdentityResolver,
) -> Result<(SourceOrderToken, RowId), CursorError> {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.bounded-page-position.v1\0");
    hasher.update(accepted_offset.to_be_bytes());
    hash_value(&mut hasher, value, identities)?;
    let digest: [u8; 32] = hasher.finalize().into();
    let mut row_id = [0_u8; 16];
    row_id.copy_from_slice(&digest[..16]);
    Ok((
        SourceOrderToken::from_u128(u128::from(accepted_offset)),
        RowId::from_bytes(row_id),
    ))
}

fn derive_encryption_key(key: &CursorSealingKey) -> [u8; 32] {
    let mut encryption = Sha256::new();
    encryption.update(b"boon.page-cursor.aead-key.v5\0");
    encryption.update(key.as_bytes());
    encryption.finalize().into()
}

fn encode_payload(cursor: &PageCursor) -> Result<Vec<u8>, CursorError> {
    let mut output = Vec::new();
    output.extend_from_slice(PAYLOAD_MAGIC);
    output.push(TOKEN_VERSION);
    output.extend_from_slice(&cursor.view_fingerprint);
    output.extend_from_slice(&cursor.authority_revision.to_be_bytes());
    output.extend_from_slice(&cursor.capture_fingerprint);
    output.extend_from_slice(&cursor.accepted_offset.to_be_bytes());
    encode_semantic_key(&mut output, &cursor.semantic_key)?;
    output.extend_from_slice(cursor.source_order.as_bytes());
    output.extend_from_slice(cursor.row_id.as_bytes());
    Ok(output)
}

fn decode_payload(payload: &[u8]) -> Result<PageCursor, CursorError> {
    let mut reader = Reader::new(payload);
    if reader.take(4)? != PAYLOAD_MAGIC || reader.u8()? != TOKEN_VERSION {
        return Err(CursorError::Invalid);
    }
    let view_fingerprint = reader.array()?;
    let authority_revision = reader.u64()?;
    let capture_fingerprint = reader.array()?;
    let accepted_offset = reader.u64()?;
    let semantic_key = decode_semantic_key(&mut reader)?;
    let source_order = SourceOrderToken::from_bytes(reader.array()?);
    let row_id = RowId::from_bytes(reader.array()?);
    if !reader.is_empty() {
        return Err(CursorError::Invalid);
    }
    Ok(PageCursor {
        view_fingerprint,
        authority_revision,
        capture_fingerprint,
        accepted_offset,
        semantic_key,
        source_order,
        row_id,
    })
}

fn encode_semantic_key(output: &mut Vec<u8>, key: &StructuralKey) -> Result<(), CursorError> {
    let component_count = u8::try_from(key.parts().len()).map_err(|_| CursorError::TooLarge)?;
    output.push(component_count);
    for component in key.parts() {
        match component {
            StructuralValue::Number(value) => {
                output.push(0);
                output.extend_from_slice(&value.to_bits().to_be_bytes());
            }
            StructuralValue::Text(value) => {
                output.push(1);
                let length = u32::try_from(value.len()).map_err(|_| CursorError::TooLarge)?;
                output.extend_from_slice(&length.to_be_bytes());
                output.extend_from_slice(value.as_bytes());
            }
            StructuralValue::Bool(value) => {
                output.push(2);
                output.push(u8::from(*value));
            }
            StructuralValue::ClosedTag(value) => {
                output.push(3);
                output.extend_from_slice(value.type_id().as_bytes());
                output.extend_from_slice(&value.ordinal().to_be_bytes());
            }
        }
        if output.len() > MAX_CURSOR_BYTES {
            return Err(CursorError::TooLarge);
        }
    }
    Ok(())
}

fn decode_semantic_key(reader: &mut Reader<'_>) -> Result<StructuralKey, CursorError> {
    let component_count = usize::from(reader.u8()?);
    let mut components = Vec::with_capacity(component_count);
    for _ in 0..component_count {
        let component = match reader.u8()? {
            0 => {
                let value = f64::from_bits(reader.u64()?);
                StructuralValue::number(value).map_err(|_| CursorError::Invalid)?
            }
            1 => {
                let length = usize::try_from(reader.u32()?).map_err(|_| CursorError::Invalid)?;
                let value =
                    std::str::from_utf8(reader.take(length)?).map_err(|_| CursorError::Invalid)?;
                StructuralValue::text(value)
            }
            2 => match reader.u8()? {
                0 => StructuralValue::Bool(false),
                1 => StructuralValue::Bool(true),
                _ => return Err(CursorError::Invalid),
            },
            3 => StructuralValue::ClosedTag(ClosedTag::new(
                TagTypeId::from_bytes(reader.array()?),
                reader.u32()?,
            )),
            _ => return Err(CursorError::Invalid),
        };
        components.push(component);
    }
    StructuralKey::new(components).map_err(|_| CursorError::Invalid)
}

fn hash_value(
    hasher: &mut Sha256,
    value: &Value,
    identities: &impl CursorIdentityResolver,
) -> Result<(), CursorError> {
    match value {
        Value::Null => hasher.update([0]),
        Value::Bool(value) => hasher.update([1, u8::from(*value)]),
        Value::Number(value) => {
            hasher.update([2]);
            hasher.update(value.get().to_bits().to_be_bytes());
        }
        Value::Text(value) => hash_bytes(hasher, 3, value.as_bytes()),
        Value::Bytes(value) => hash_bytes(hasher, 4, value),
        Value::List(values) => {
            hasher.update([5]);
            hasher.update((values.len() as u64).to_be_bytes());
            for value in values {
                hash_value(hasher, value, identities)?;
            }
        }
        Value::Record(fields) => hash_record(hasher, 6, fields, identities)?,
        Value::MappedRow { id, fields } => {
            hasher.update([7]);
            hash_semantic_row_id(
                hasher,
                identities
                    .semantic_row_id(*id)
                    .ok_or(CursorError::Invalid)?,
            );
            hash_record(hasher, 8, fields, identities)?;
        }
        Value::Row { id, fields } => {
            hasher.update([9]);
            hash_semantic_row_id(
                hasher,
                identities
                    .semantic_row_id(*id)
                    .ok_or(CursorError::Invalid)?,
            );
            hasher.update((fields.len() as u64).to_be_bytes());
            let mut semantic_fields = fields
                .iter()
                .map(|(field, value)| {
                    identities
                        .semantic_row_field_id(*id, *field)
                        .map(|identity| (identity, value))
                        .ok_or(CursorError::Invalid)
                })
                .collect::<Result<Vec<_>, _>>()?;
            semantic_fields.sort_by_key(|(identity, _)| *identity);
            if semantic_fields
                .windows(2)
                .any(|fields| fields[0].0 == fields[1].0)
            {
                return Err(CursorError::Invalid);
            }
            for (field, value) in semantic_fields {
                hasher.update(field);
                hash_value(hasher, value, identities)?;
            }
        }
        Value::Error { code } => hash_bytes(hasher, 10, code.as_bytes()),
        Value::HostBound { visible, .. } => {
            hasher.update([11]);
            hash_value(hasher, visible, identities)?;
        }
    }
    Ok(())
}

fn hash_record(
    hasher: &mut Sha256,
    marker: u8,
    fields: &BTreeMap<String, Value>,
    identities: &impl CursorIdentityResolver,
) -> Result<(), CursorError> {
    hasher.update([marker]);
    hasher.update((fields.len() as u64).to_be_bytes());
    for (name, value) in fields {
        hash_bytes(hasher, 12, name.as_bytes());
        hash_value(hasher, value, identities)?;
    }
    Ok(())
}

fn hash_semantic_row_id(hasher: &mut Sha256, row: CursorSemanticRowId) {
    hasher.update(row.list_memory_id);
    hasher.update(row.list_type_fingerprint);
    hasher.update(row.row_key.to_be_bytes());
    hasher.update(row.row_generation.to_be_bytes());
}

fn hash_bytes(hasher: &mut Sha256, marker: u8, bytes: &[u8]) {
    hasher.update([marker]);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], CursorError> {
        if count > self.remaining.len() {
            return Err(CursorError::Invalid);
        }
        let (value, remaining) = self.remaining.split_at(count);
        self.remaining = remaining;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, CursorError> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, CursorError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, CursorError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CursorError> {
        self.take(N)?.try_into().map_err(|_| CursorError::Invalid)
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::ListId;
    use std::collections::BTreeMap;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn cursor_payload_has_one_fixed_width_cross_target_encoding() {
        let cursor = PageCursor {
            view_fingerprint: [0x11; 32],
            authority_revision: 0x1112_1314_1516_1718,
            capture_fingerprint: [0x22; 32],
            accepted_offset: 0x2122_2324_2526_2728,
            semantic_key: StructuralKey::new(vec![StructuralValue::text("A\0")]).unwrap(),
            source_order: SourceOrderToken::from_u128(0x3132_3334_3536_3738_4142_4344_4546_4748),
            row_id: RowId::from_u128(0x5152_5354_5556_5758_6162_6364_6566_6768),
        };
        let mut expected = b"BPGC".to_vec();
        expected.push(5);
        expected.extend_from_slice(&[0x11; 32]);
        expected.extend_from_slice(&0x1112_1314_1516_1718_u64.to_be_bytes());
        expected.extend_from_slice(&[0x22; 32]);
        expected.extend_from_slice(&0x2122_2324_2526_2728_u64.to_be_bytes());
        expected.extend_from_slice(&[1, 1, 0, 0, 0, 2, b'A', 0]);
        expected.extend_from_slice(&0x3132_3334_3536_3738_4142_4344_4546_4748_u128.to_be_bytes());
        expected.extend_from_slice(&0x5152_5354_5556_5758_6162_6364_6566_6768_u128.to_be_bytes());

        let encoded = encode_payload(&cursor).unwrap();
        assert_eq!(encoded, expected);
        assert_eq!(decode_payload(&encoded).unwrap(), cursor);
    }

    #[test]
    fn cursor_round_trip_is_confidential_and_tamper_evident() {
        let cursor = PageCursor {
            view_fingerprint: [1; 32],
            authority_revision: 9,
            capture_fingerprint: [2; 32],
            accepted_offset: 20,
            semantic_key: StructuralKey::new(vec![
                StructuralValue::text("secret-key"),
                StructuralValue::number(42.5).unwrap(),
                StructuralValue::Bool(true),
                StructuralValue::ClosedTag(ClosedTag::new(TagTypeId::from_u128(8), 3)),
            ])
            .unwrap(),
            source_order: SourceOrderToken::from_u128(19),
            row_id: RowId::from_u128(u128::from_be_bytes(*b"secret-row-id!!!")),
        };
        let key = CursorSealingKey::from_bytes([3; 32]);
        let token = seal_cursor(&key, &cursor).unwrap();
        assert!(
            !token
                .windows(b"secret-row-id!!!".len())
                .any(|window| window == b"secret-row-id!!!")
        );
        assert!(
            !token
                .windows(b"secret-key".len())
                .any(|window| window == b"secret-key")
        );
        assert_eq!(open_cursor(&key, &token).unwrap(), cursor);
        assert_eq!(
            open_cursor(&CursorSealingKey::from_bytes([4; 32]), &token),
            Err(CursorError::Invalid)
        );

        let mut wrong_version = token.clone();
        wrong_version[0] = TOKEN_VERSION.saturating_sub(1);
        assert_eq!(open_cursor(&key, &wrong_version), Err(CursorError::Invalid));

        let mut tampered = token;
        tampered[20] ^= 1;
        assert_eq!(open_cursor(&key, &tampered), Err(CursorError::Invalid));
    }

    #[test]
    fn cursor_payload_is_independent_of_physical_key_schema() {
        let cursor = PageCursor {
            view_fingerprint: [4; 32],
            authority_revision: 1,
            capture_fingerprint: [5; 32],
            accepted_offset: 2,
            semantic_key: StructuralKey::new(vec![StructuralValue::text("semantic")]).unwrap(),
            source_order: SourceOrderToken::from_u128(3),
            row_id: RowId::from_u128(4),
        };
        let key = CursorSealingKey::from_bytes([6; 32]);
        let token = seal_cursor(&key, &cursor).unwrap();
        assert_eq!(open_cursor(&key, &token).unwrap(), cursor);
    }

    #[derive(Default)]
    struct TestIdentityResolver {
        rows: BTreeMap<RuntimeRowId, CursorSemanticRowId>,
        fields: BTreeMap<(RuntimeRowId, FieldId), [u8; 32]>,
    }

    impl CursorIdentityResolver for TestIdentityResolver {
        fn semantic_row_id(&self, row: RuntimeRowId) -> Option<CursorSemanticRowId> {
            self.rows.get(&row).copied()
        }

        fn semantic_row_field_id(&self, row: RuntimeRowId, field: FieldId) -> Option<[u8; 32]> {
            self.fields.get(&(row, field)).copied()
        }
    }

    #[test]
    fn capture_identity_ignores_physical_row_and_field_numbering() {
        let first_row = RuntimeRowId {
            list: ListId(2),
            key: 41,
            generation: 7,
        };
        let shifted_row = RuntimeRowId {
            list: ListId(91),
            key: 41,
            generation: 7,
        };
        let semantic_row = CursorSemanticRowId::new([0x31; 32], [0x32; 32], 41, 7);
        let first_field = FieldId(4);
        let shifted_field = FieldId(404);
        let first = TestIdentityResolver {
            rows: BTreeMap::from([(first_row, semantic_row)]),
            fields: BTreeMap::from([((first_row, first_field), [0x33; 32])]),
        };
        let shifted = TestIdentityResolver {
            rows: BTreeMap::from([(shifted_row, semantic_row)]),
            fields: BTreeMap::from([((shifted_row, shifted_field), [0x33; 32])]),
        };
        let first_capture = Value::Row {
            id: first_row,
            fields: BTreeMap::from([(first_field, Value::Text("same".to_owned()))]),
        };
        let shifted_capture = Value::Row {
            id: shifted_row,
            fields: BTreeMap::from([(shifted_field, Value::Text("same".to_owned()))]),
        };

        let first_fingerprint = capture_fingerprint(
            [0x21; 32],
            None,
            None,
            &[first_row],
            &Value::Null,
            [&first_capture],
            &first,
        )
        .unwrap();
        let shifted_fingerprint = capture_fingerprint(
            [0x21; 32],
            None,
            None,
            &[shifted_row],
            &Value::Null,
            [&shifted_capture],
            &shifted,
        )
        .unwrap();

        assert_eq!(first_fingerprint, shifted_fingerprint);

        let changed = TestIdentityResolver {
            rows: BTreeMap::from([(
                shifted_row,
                CursorSemanticRowId::new([0x41; 32], [0x32; 32], 41, 7),
            )]),
            fields: shifted.fields,
        };
        assert_ne!(
            first_fingerprint,
            capture_fingerprint(
                [0x21; 32],
                None,
                None,
                &[shifted_row],
                &Value::Null,
                [&shifted_capture],
                &changed,
            )
            .unwrap()
        );
    }
}
