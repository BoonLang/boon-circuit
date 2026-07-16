use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub const BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION: u32 = 1;
pub const BROWSER_PREFERENCE_VALUE_FORMAT_VERSION: u8 = 1;
pub const MAX_BROWSER_PREFERENCE_DATABASE_NAME_BYTES: usize = 128;
pub const MAX_BROWSER_PREFERENCE_NAMESPACES: usize = 64;
pub const MAX_BROWSER_PREFERENCE_NAMESPACE_NAME_BYTES: usize = 96;
pub const MAX_BROWSER_PREFERENCE_KEY_BYTES: usize = 1_024;
pub const MAX_BROWSER_PREFERENCE_VALUE_BYTES: usize = 1_048_576;
pub const MAX_BROWSER_PREFERENCE_ENTRIES_PER_NAMESPACE: u32 = 65_536;
pub const MAX_BROWSER_PREFERENCE_PLATFORM_ERROR_BYTES: usize = 512;

#[cfg(target_arch = "wasm32")]
pub(crate) const BROWSER_PREFERENCE_STORAGE_OBJECT_STORE: &str = "preferences";
#[cfg(any(test, target_arch = "wasm32"))]
const VALUE_KIND_BYTES: u8 = 0;
#[cfg(any(test, target_arch = "wasm32"))]
const VALUE_KIND_TEXT: u8 = 1;

pub type BrowserPreferenceStorageResult<T> = Result<T, BrowserPreferenceStorageError>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserPreferenceStorageError {
    InvalidInput {
        field: String,
        reason: String,
    },
    LimitExceeded {
        resource: String,
        limit: usize,
    },
    NamespaceNotDeclared {
        namespace: String,
    },
    ValueKindMismatch {
        namespace: String,
        expected: BrowserPreferenceValueKind,
        actual: BrowserPreferenceValueKind,
    },
    SchemaMismatch {
        expected_version: u32,
        actual_version: Option<u32>,
        reason: String,
    },
    CorruptValue {
        namespace: String,
        reason: String,
    },
    QuotaExceeded {
        operation: String,
        message: String,
    },
    Platform {
        operation: String,
        message: String,
    },
}

impl BrowserPreferenceStorageError {
    pub fn from_platform(
        operation: impl Into<String>,
        error_name: Option<&str>,
        message: &str,
    ) -> Self {
        let operation = operation.into();
        let detail = bounded_platform_detail(error_name, message);
        let classification = detail.to_ascii_lowercase();
        if classification.contains("quotaexceedederror")
            || classification.contains("quota exceeded")
        {
            Self::QuotaExceeded {
                operation,
                message: detail,
            }
        } else {
            Self::Platform {
                operation,
                message: detail,
            }
        }
    }

    pub fn is_quota_exceeded(&self) -> bool {
        matches!(self, Self::QuotaExceeded { .. })
    }
}

impl Display for BrowserPreferenceStorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput { field, reason } => {
                write!(formatter, "invalid browser preference {field}: {reason}")
            }
            Self::LimitExceeded { resource, limit } => write!(
                formatter,
                "browser preference resource {resource} exceeds limit {limit}"
            ),
            Self::NamespaceNotDeclared { namespace } => write!(
                formatter,
                "browser preference namespace {namespace} is not declared"
            ),
            Self::ValueKindMismatch {
                namespace,
                expected,
                actual,
            } => write!(
                formatter,
                "browser preference namespace {namespace} expects {expected}, received {actual}"
            ),
            Self::SchemaMismatch {
                expected_version,
                actual_version,
                reason,
            } => write!(
                formatter,
                "browser preference schema mismatch: expected version {expected_version}, found {}; {reason}",
                actual_version
                    .map(|version| version.to_string())
                    .unwrap_or_else(|| "unknown".to_owned())
            ),
            Self::CorruptValue { namespace, reason } => write!(
                formatter,
                "browser preference value in namespace {namespace} is corrupt: {reason}"
            ),
            Self::QuotaExceeded { operation, message } => write!(
                formatter,
                "browser preference operation {operation} exceeded browser quota: {message}"
            ),
            Self::Platform { operation, message } => write!(
                formatter,
                "browser preference operation {operation} failed: {message}"
            ),
        }
    }
}

impl Error for BrowserPreferenceStorageError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BrowserPreferenceNamespaceId(String);

impl BrowserPreferenceNamespaceId {
    pub fn new(value: impl Into<String>) -> BrowserPreferenceStorageResult<Self> {
        let value = value.into();
        validate_identifier(
            "namespace name",
            &value,
            MAX_BROWSER_PREFERENCE_NAMESPACE_NAME_BYTES,
        )?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(&self) -> BrowserPreferenceStorageResult<()> {
        validate_identifier(
            "namespace name",
            &self.0,
            MAX_BROWSER_PREFERENCE_NAMESPACE_NAME_BYTES,
        )
    }
}

impl Display for BrowserPreferenceNamespaceId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BrowserPreferenceKey(String);

impl BrowserPreferenceKey {
    pub fn new(value: impl Into<String>) -> BrowserPreferenceStorageResult<Self> {
        let value = value.into();
        validate_key_text(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn byte_len(&self) -> usize {
        self.0.len()
    }

    fn validate(&self) -> BrowserPreferenceStorageResult<()> {
        validate_key_text(&self.0)
    }
}

impl Display for BrowserPreferenceKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserPreferenceValueKind {
    Bytes,
    Text,
}

impl Display for BrowserPreferenceValueKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bytes => "bytes",
            Self::Text => "text",
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum BrowserPreferenceValue {
    Bytes(Vec<u8>),
    Text(String),
}

impl BrowserPreferenceValue {
    pub fn kind(&self) -> BrowserPreferenceValueKind {
        match self {
            Self::Bytes(_) => BrowserPreferenceValueKind::Bytes,
            Self::Text(_) => BrowserPreferenceValueKind::Text,
        }
    }

    pub fn byte_len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Text(text) => text.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserPreferenceNamespaceLimits {
    max_key_bytes: usize,
    max_value_bytes: usize,
    max_entries: u32,
}

impl BrowserPreferenceNamespaceLimits {
    pub fn new(
        max_key_bytes: usize,
        max_value_bytes: usize,
        max_entries: u32,
    ) -> BrowserPreferenceStorageResult<Self> {
        let limits = Self {
            max_key_bytes,
            max_value_bytes,
            max_entries,
        };
        limits.validate()?;
        Ok(limits)
    }

    pub fn max_key_bytes(self) -> usize {
        self.max_key_bytes
    }

    pub fn max_value_bytes(self) -> usize {
        self.max_value_bytes
    }

    pub fn max_entries(self) -> u32 {
        self.max_entries
    }

    fn validate(self) -> BrowserPreferenceStorageResult<()> {
        validate_nonzero_bounded_limit(
            "namespace max_key_bytes",
            self.max_key_bytes,
            MAX_BROWSER_PREFERENCE_KEY_BYTES,
        )?;
        validate_nonzero_bounded_limit(
            "namespace max_value_bytes",
            self.max_value_bytes,
            MAX_BROWSER_PREFERENCE_VALUE_BYTES,
        )?;
        if self.max_entries == 0 {
            return Err(invalid(
                "namespace max_entries",
                "must be greater than zero",
            ));
        }
        if self.max_entries > MAX_BROWSER_PREFERENCE_ENTRIES_PER_NAMESPACE {
            return Err(BrowserPreferenceStorageError::LimitExceeded {
                resource: "namespace max_entries".to_owned(),
                limit: MAX_BROWSER_PREFERENCE_ENTRIES_PER_NAMESPACE as usize,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserPreferenceNamespace {
    id: BrowserPreferenceNamespaceId,
    value_kind: BrowserPreferenceValueKind,
    limits: BrowserPreferenceNamespaceLimits,
}

impl BrowserPreferenceNamespace {
    pub fn new(
        id: impl Into<String>,
        value_kind: BrowserPreferenceValueKind,
        limits: BrowserPreferenceNamespaceLimits,
    ) -> BrowserPreferenceStorageResult<Self> {
        let namespace = Self {
            id: BrowserPreferenceNamespaceId::new(id)?,
            value_kind,
            limits,
        };
        namespace.validate()?;
        Ok(namespace)
    }

    pub fn id(&self) -> &BrowserPreferenceNamespaceId {
        &self.id
    }

    pub fn value_kind(&self) -> BrowserPreferenceValueKind {
        self.value_kind
    }

    pub fn limits(&self) -> BrowserPreferenceNamespaceLimits {
        self.limits
    }

    pub fn validate_key(&self, key: &BrowserPreferenceKey) -> BrowserPreferenceStorageResult<()> {
        key.validate()?;
        if key.byte_len() > self.limits.max_key_bytes {
            return Err(BrowserPreferenceStorageError::LimitExceeded {
                resource: format!("key bytes in namespace {}", self.id),
                limit: self.limits.max_key_bytes,
            });
        }
        Ok(())
    }

    pub fn validate_value(
        &self,
        value: &BrowserPreferenceValue,
    ) -> BrowserPreferenceStorageResult<()> {
        let actual = value.kind();
        if actual != self.value_kind {
            return Err(BrowserPreferenceStorageError::ValueKindMismatch {
                namespace: self.id.to_string(),
                expected: self.value_kind,
                actual,
            });
        }
        if value.byte_len() > self.limits.max_value_bytes {
            return Err(BrowserPreferenceStorageError::LimitExceeded {
                resource: format!("value bytes in namespace {}", self.id),
                limit: self.limits.max_value_bytes,
            });
        }
        Ok(())
    }

    fn validate(&self) -> BrowserPreferenceStorageResult<()> {
        self.id.validate()?;
        self.limits.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowserPreferenceStorageConfig {
    pub database_name: String,
    pub schema_version: u32,
    pub namespaces: Vec<BrowserPreferenceNamespace>,
}

impl BrowserPreferenceStorageConfig {
    pub fn new(
        database_name: impl Into<String>,
        namespaces: impl IntoIterator<Item = BrowserPreferenceNamespace>,
    ) -> BrowserPreferenceStorageResult<Self> {
        let mut namespaces = namespaces.into_iter().collect::<Vec<_>>();
        if namespaces.len() > MAX_BROWSER_PREFERENCE_NAMESPACES {
            return Err(BrowserPreferenceStorageError::LimitExceeded {
                resource: "namespace count".to_owned(),
                limit: MAX_BROWSER_PREFERENCE_NAMESPACES,
            });
        }
        namespaces.sort_by(|left, right| left.id.cmp(&right.id));
        let config = Self {
            database_name: database_name.into(),
            schema_version: BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
            namespaces,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> BrowserPreferenceStorageResult<()> {
        validate_identifier(
            "database name",
            &self.database_name,
            MAX_BROWSER_PREFERENCE_DATABASE_NAME_BYTES,
        )?;
        if self.schema_version != BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION {
            return Err(BrowserPreferenceStorageError::SchemaMismatch {
                expected_version: BROWSER_PREFERENCE_STORAGE_SCHEMA_VERSION,
                actual_version: Some(self.schema_version),
                reason: "the host implements one explicit IndexedDB schema version".to_owned(),
            });
        }
        if self.namespaces.is_empty() {
            return Err(invalid(
                "namespaces",
                "at least one typed namespace must be declared",
            ));
        }
        if self.namespaces.len() > MAX_BROWSER_PREFERENCE_NAMESPACES {
            return Err(BrowserPreferenceStorageError::LimitExceeded {
                resource: "namespace count".to_owned(),
                limit: MAX_BROWSER_PREFERENCE_NAMESPACES,
            });
        }

        let mut ids = BTreeSet::new();
        for namespace in &self.namespaces {
            namespace.validate()?;
            if !ids.insert(namespace.id.as_str()) {
                return Err(invalid(
                    "namespaces",
                    format!("duplicate namespace {}", namespace.id),
                ));
            }
        }
        Ok(())
    }

    pub fn namespace(
        &self,
        id: &BrowserPreferenceNamespaceId,
    ) -> BrowserPreferenceStorageResult<&BrowserPreferenceNamespace> {
        self.namespaces
            .iter()
            .find(|namespace| namespace.id == *id)
            .ok_or_else(|| BrowserPreferenceStorageError::NamespaceNotDeclared {
                namespace: id.to_string(),
            })
    }

    pub fn validate_key(
        &self,
        namespace: &BrowserPreferenceNamespaceId,
        key: &BrowserPreferenceKey,
    ) -> BrowserPreferenceStorageResult<()> {
        self.namespace(namespace)?.validate_key(key)
    }

    pub fn validate_entry(
        &self,
        namespace: &BrowserPreferenceNamespaceId,
        key: &BrowserPreferenceKey,
        value: &BrowserPreferenceValue,
    ) -> BrowserPreferenceStorageResult<()> {
        let namespace = self.namespace(namespace)?;
        namespace.validate_key(key)?;
        namespace.validate_value(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserPreferencePutOutcome {
    Inserted,
    Replaced,
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn encode_preference_value(
    namespace: &BrowserPreferenceNamespace,
    value: &BrowserPreferenceValue,
) -> BrowserPreferenceStorageResult<Vec<u8>> {
    namespace.validate_value(value)?;
    let mut encoded = Vec::with_capacity(value.byte_len() + 2);
    encoded.push(BROWSER_PREFERENCE_VALUE_FORMAT_VERSION);
    match value {
        BrowserPreferenceValue::Bytes(bytes) => {
            encoded.push(VALUE_KIND_BYTES);
            encoded.extend_from_slice(bytes);
        }
        BrowserPreferenceValue::Text(text) => {
            encoded.push(VALUE_KIND_TEXT);
            encoded.extend_from_slice(text.as_bytes());
        }
    }
    Ok(encoded)
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn decode_preference_value(
    namespace: &BrowserPreferenceNamespace,
    encoded: &[u8],
) -> BrowserPreferenceStorageResult<BrowserPreferenceValue> {
    if encoded.len() < 2 {
        return Err(corrupt(namespace, "encoded value is missing its header"));
    }
    if encoded[0] != BROWSER_PREFERENCE_VALUE_FORMAT_VERSION {
        return Err(corrupt(
            namespace,
            format!("value format version {} is not supported", encoded[0]),
        ));
    }
    let actual_kind = match encoded[1] {
        VALUE_KIND_BYTES => BrowserPreferenceValueKind::Bytes,
        VALUE_KIND_TEXT => BrowserPreferenceValueKind::Text,
        tag => {
            return Err(corrupt(
                namespace,
                format!("value kind tag {tag} is not supported"),
            ));
        }
    };
    if actual_kind != namespace.value_kind {
        return Err(corrupt(
            namespace,
            format!(
                "stored value kind {actual_kind} does not match declared kind {}",
                namespace.value_kind
            ),
        ));
    }

    let payload = &encoded[2..];
    if payload.len() > namespace.limits.max_value_bytes {
        return Err(corrupt(
            namespace,
            format!(
                "stored value has {} bytes, exceeding {}",
                payload.len(),
                namespace.limits.max_value_bytes
            ),
        ));
    }
    match actual_kind {
        BrowserPreferenceValueKind::Bytes => Ok(BrowserPreferenceValue::Bytes(payload.to_vec())),
        BrowserPreferenceValueKind::Text => String::from_utf8(payload.to_vec())
            .map(BrowserPreferenceValue::Text)
            .map_err(|_| corrupt(namespace, "stored text is not valid UTF-8")),
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn indexed_db_storage_key(
    namespace: &BrowserPreferenceNamespaceId,
    key: &BrowserPreferenceKey,
) -> String {
    let mut storage_key = String::with_capacity(namespace.as_str().len() + key.byte_len() + 1);
    storage_key.push_str(namespace.as_str());
    storage_key.push('\0');
    storage_key.push_str(key.as_str());
    storage_key
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn indexed_db_namespace_bounds(
    namespace: &BrowserPreferenceNamespaceId,
) -> (String, String) {
    let mut lower = String::with_capacity(namespace.as_str().len() + 1);
    lower.push_str(namespace.as_str());
    lower.push('\0');
    let mut upper = String::with_capacity(namespace.as_str().len() + 1);
    upper.push_str(namespace.as_str());
    upper.push('\u{1}');
    (lower, upper)
}

fn validate_identifier(
    field: &str,
    value: &str,
    limit: usize,
) -> BrowserPreferenceStorageResult<()> {
    if value.is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if value.len() > limit {
        return Err(BrowserPreferenceStorageError::LimitExceeded {
            resource: field.to_owned(),
            limit,
        });
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(
            field,
            "must contain ASCII letters, digits, '.', '_' or '-'",
        ));
    }
    Ok(())
}

fn validate_key_text(value: &str) -> BrowserPreferenceStorageResult<()> {
    if value.is_empty() {
        return Err(invalid("key", "must not be empty"));
    }
    if value.len() > MAX_BROWSER_PREFERENCE_KEY_BYTES {
        return Err(BrowserPreferenceStorageError::LimitExceeded {
            resource: "key bytes".to_owned(),
            limit: MAX_BROWSER_PREFERENCE_KEY_BYTES,
        });
    }
    if value.chars().any(char::is_control) {
        return Err(invalid("key", "must not contain control characters"));
    }
    Ok(())
}

fn validate_nonzero_bounded_limit(
    field: &str,
    value: usize,
    hard_limit: usize,
) -> BrowserPreferenceStorageResult<()> {
    if value == 0 {
        return Err(invalid(field, "must be greater than zero"));
    }
    if value > hard_limit {
        return Err(BrowserPreferenceStorageError::LimitExceeded {
            resource: field.to_owned(),
            limit: hard_limit,
        });
    }
    Ok(())
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> BrowserPreferenceStorageError {
    BrowserPreferenceStorageError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn corrupt(
    namespace: &BrowserPreferenceNamespace,
    reason: impl Into<String>,
) -> BrowserPreferenceStorageError {
    BrowserPreferenceStorageError::CorruptValue {
        namespace: namespace.id.to_string(),
        reason: reason.into(),
    }
}

fn bounded_platform_detail(error_name: Option<&str>, message: &str) -> String {
    let detail = match (
        error_name.filter(|name| !name.is_empty()),
        message.is_empty(),
    ) {
        (Some(name), false) => format!("{name}: {message}"),
        (Some(name), true) => name.to_owned(),
        (None, false) => message.to_owned(),
        (None, true) => "browser rejected the IndexedDB operation".to_owned(),
    };
    truncate_utf8(&detail, MAX_BROWSER_PREFERENCE_PLATFORM_ERROR_BYTES)
}

fn truncate_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let suffix = "...";
    let mut end = limit.saturating_sub(suffix.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut bounded = String::with_capacity(limit);
    bounded.push_str(&value[..end]);
    bounded.push_str(suffix);
    bounded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn namespace(
        id: &str,
        value_kind: BrowserPreferenceValueKind,
        max_value_bytes: usize,
    ) -> BrowserPreferenceNamespace {
        BrowserPreferenceNamespace::new(
            id,
            value_kind,
            BrowserPreferenceNamespaceLimits::new(32, max_value_bytes, 8).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn opaque_values_have_a_deterministic_bounded_format() {
        let binary = namespace("binary", BrowserPreferenceValueKind::Bytes, 8);
        let bytes = BrowserPreferenceValue::Bytes(vec![0, 255, 1]);
        let encoded = encode_preference_value(&binary, &bytes).unwrap();
        assert_eq!(
            encoded,
            [BROWSER_PREFERENCE_VALUE_FORMAT_VERSION, 0, 0, 255, 1]
        );
        assert_eq!(decode_preference_value(&binary, &encoded).unwrap(), bytes);

        let textual = namespace("textual", BrowserPreferenceValueKind::Text, 8);
        let text = BrowserPreferenceValue::Text("h\u{e9}".to_owned());
        let encoded = encode_preference_value(&textual, &text).unwrap();
        assert_eq!(
            encoded,
            [BROWSER_PREFERENCE_VALUE_FORMAT_VERSION, 1, b'h', 0xc3, 0xa9]
        );
        assert_eq!(decode_preference_value(&textual, &encoded).unwrap(), text);

        for corrupt in [
            Vec::new(),
            vec![BROWSER_PREFERENCE_VALUE_FORMAT_VERSION + 1, 0],
            vec![BROWSER_PREFERENCE_VALUE_FORMAT_VERSION, 9],
        ] {
            assert!(matches!(
                decode_preference_value(&binary, &corrupt),
                Err(BrowserPreferenceStorageError::CorruptValue { .. })
            ));
        }
        assert!(matches!(
            decode_preference_value(
                &textual,
                &[BROWSER_PREFERENCE_VALUE_FORMAT_VERSION, 1, 0xff]
            ),
            Err(BrowserPreferenceStorageError::CorruptValue { .. })
        ));
        assert!(matches!(
            decode_preference_value(&textual, &[BROWSER_PREFERENCE_VALUE_FORMAT_VERSION, 0, 1]),
            Err(BrowserPreferenceStorageError::CorruptValue { .. })
        ));

        let tiny = namespace("tiny", BrowserPreferenceValueKind::Bytes, 1);
        assert!(matches!(
            decode_preference_value(&tiny, &[BROWSER_PREFERENCE_VALUE_FORMAT_VERSION, 0, 1, 2]),
            Err(BrowserPreferenceStorageError::CorruptValue { .. })
        ));
    }

    #[test]
    fn logical_namespace_keys_are_collision_free_and_range_addressable() {
        let alpha = BrowserPreferenceNamespaceId::new("alpha").unwrap();
        let alpha_other = BrowserPreferenceNamespaceId::new("alpha-other").unwrap();
        let key = BrowserPreferenceKey::new("nested/\u{1f642}").unwrap();
        let storage_key = indexed_db_storage_key(&alpha, &key);
        let other_storage_key = indexed_db_storage_key(&alpha_other, &key);
        let (lower, upper) = indexed_db_namespace_bounds(&alpha);

        assert_eq!(storage_key, "alpha\0nested/\u{1f642}");
        assert_ne!(storage_key, other_storage_key);
        assert!(lower < storage_key);
        assert!(storage_key < upper);
        assert!(other_storage_key > upper);
        assert_eq!(lower, "alpha\0");
        assert_eq!(upper, "alpha\u{1}");
    }
}
