//! Bounded JSON conversion at Boon's transport boundary.
//!
//! JSON is decoded directly into the canonical structural host value. No
//! `serde_json::Value` is created or retained. Domain validation and any
//! domain-specific variant-to-wire convention remain Boon responsibilities.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

pub use boon_data::{FiniteReal, Value};

mod decode;
mod encode;

pub use decode::decode;
pub use encode::encode;

pub const JSON_DECODED_TAG: &str = "JsonDecoded";
pub const JSON_DECODE_FAILED_TAG: &str = "JsonDecodeFailed";
pub const JSON_ENCODED_TAG: &str = "JsonEncoded";
pub const JSON_ENCODE_FAILED_TAG: &str = "JsonEncodeFailed";
pub const JSON_DIAGNOSTIC_TAG: &str = "JsonDiagnostic";

/// Largest supported nesting limit. Keeping this finite bounds parser stack
/// use on native and Wasm targets even when a caller supplies bad policy.
pub const MAX_SUPPORTED_DEPTH: usize = 128;

/// Diagnostics are always capped independently of caller policy.
pub const MAX_DIAGNOSTIC_BYTES: usize = 1024;

/// Largest byte offset that can be represented exactly as a Boon `Number`.
pub const MAX_EXACT_NUMBER_BOUND: usize = if usize::BITS > 53 {
    (1u64 << 53) as usize
} else {
    usize::MAX
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    /// Root is depth zero; each array/object child increases depth by one.
    pub max_depth: usize,
    /// Counts JSON values. Object keys are strings but are not value nodes.
    pub max_nodes: usize,
    /// Maximum decoded UTF-8 bytes in one text value or object key.
    pub max_string_bytes: usize,
    pub max_array_items: usize,
    pub max_object_fields: usize,
    pub max_diagnostic_bytes: usize,
}

impl Limits {
    pub const STRICT_SERVER_CLIENT: Self = Self {
        max_input_bytes: 1024 * 1024,
        max_output_bytes: 1024 * 1024,
        max_depth: 64,
        max_nodes: 100_000,
        max_string_bytes: 256 * 1024,
        max_array_items: 50_000,
        max_object_fields: 10_000,
        max_diagnostic_bytes: 256,
    };

    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.max_depth > MAX_SUPPORTED_DEPTH {
            return Err(self.invalid(format!(
                "max_depth {} exceeds supported maximum {MAX_SUPPORTED_DEPTH}",
                self.max_depth
            )));
        }
        if self.max_input_bytes > MAX_EXACT_NUMBER_BOUND {
            return Err(self.invalid(format!(
                "max_input_bytes {} exceeds exact Boon Number offset range {MAX_EXACT_NUMBER_BOUND}",
                self.max_input_bytes
            )));
        }
        if self.max_output_bytes > MAX_EXACT_NUMBER_BOUND {
            return Err(self.invalid(format!(
                "max_output_bytes {} exceeds exact Boon Number offset range {MAX_EXACT_NUMBER_BOUND}",
                self.max_output_bytes
            )));
        }
        Ok(())
    }

    fn invalid(&self, message: String) -> Diagnostic {
        make_diagnostic(DiagnosticCode::InvalidLimits, 0, message, self)
    }

    pub(crate) fn diagnostic_budget(&self) -> usize {
        self.max_diagnostic_bytes.min(MAX_DIAGNOSTIC_BYTES)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::STRICT_SERVER_CLIENT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    InvalidLimits,
    InputTooLarge,
    OutputTooLarge,
    InvalidUtf8,
    InvalidSyntax,
    DuplicateKey,
    NumberOutOfRange,
    DepthLimit,
    NodeLimit,
    StringLimit,
    ArrayItemsLimit,
    ObjectFieldsLimit,
    UnsupportedValue,
}

impl DiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidLimits => "invalid_limits",
            Self::InputTooLarge => "input_too_large",
            Self::OutputTooLarge => "output_too_large",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::InvalidSyntax => "invalid_syntax",
            Self::DuplicateKey => "duplicate_key",
            Self::NumberOutOfRange => "number_out_of_range",
            Self::DepthLimit => "depth_limit",
            Self::NodeLimit => "node_limit",
            Self::StringLimit => "string_limit",
            Self::ArrayItemsLimit => "array_items_limit",
            Self::ObjectFieldsLimit => "object_fields_limit",
            Self::UnsupportedValue => "unsupported_value",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    /// Zero-based byte cursor at which the boundary rejected the operation.
    pub offset: usize,
    pub message: String,
}

impl Diagnostic {
    pub fn into_boon_value(self) -> Value {
        let offset = FiniteReal::from_i64_exact(
            i64::try_from(self.offset).expect("validated JSON byte offset fits i64"),
        )
        .expect("validated JSON byte offset is an exact Boon Number");
        Value::Variant {
            tag: JSON_DIAGNOSTIC_TAG.to_owned(),
            fields: BTreeMap::from([
                (
                    "code".to_owned(),
                    Value::Text(self.code.as_str().to_owned()),
                ),
                ("message".to_owned(), Value::Text(self.message)),
                ("offset".to_owned(), Value::Number(offset)),
            ]),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at byte {}: {}",
            self.code.as_str(),
            self.offset,
            self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

/// Decode and expose the result as an ordinary Boon variant value.
pub fn decode_boon(input: &[u8], limits: &Limits) -> Value {
    match decode(input, limits) {
        Ok(value) => Value::Variant {
            tag: JSON_DECODED_TAG.to_owned(),
            fields: BTreeMap::from([("value".to_owned(), value)]),
        },
        Err(diagnostic) => Value::Variant {
            tag: JSON_DECODE_FAILED_TAG.to_owned(),
            fields: BTreeMap::from([("diagnostic".to_owned(), diagnostic.into_boon_value())]),
        },
    }
}

/// Encode and expose UTF-8 JSON text as an ordinary Boon variant value.
pub fn encode_boon(value: &Value, limits: &Limits) -> Value {
    match encode(value, limits) {
        Ok(bytes) => Value::Variant {
            tag: JSON_ENCODED_TAG.to_owned(),
            fields: BTreeMap::from([(
                "text".to_owned(),
                Value::Text(String::from_utf8(bytes).expect("JSON encoder emits UTF-8")),
            )]),
        },
        Err(diagnostic) => Value::Variant {
            tag: JSON_ENCODE_FAILED_TAG.to_owned(),
            fields: BTreeMap::from([("diagnostic".to_owned(), diagnostic.into_boon_value())]),
        },
    }
}

/// Incremental body/message accumulator that enforces the byte boundary before
/// a complete streaming transport payload is assembled.
#[derive(Clone, Debug)]
pub struct BoundedJsonInput {
    bytes: Vec<u8>,
    limits: Limits,
}

impl BoundedJsonInput {
    pub fn new(limits: Limits) -> Result<Self, Diagnostic> {
        limits.validate()?;
        Ok(Self {
            bytes: Vec::new(),
            limits,
        })
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), Diagnostic> {
        let remaining = self.limits.max_input_bytes.saturating_sub(self.bytes.len());
        if chunk.len() > remaining {
            return Err(make_diagnostic(
                DiagnosticCode::InputTooLarge,
                self.limits.max_input_bytes,
                format!(
                    "JSON input exceeds {} byte limit",
                    self.limits.max_input_bytes
                ),
                &self.limits,
            ));
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn finish(self) -> Result<Value, Diagnostic> {
        decode(&self.bytes, &self.limits)
    }
}

pub(crate) fn make_diagnostic(
    code: DiagnosticCode,
    offset: usize,
    message: impl Into<String>,
    limits: &Limits,
) -> Diagnostic {
    Diagnostic {
        code,
        offset: offset.min(MAX_EXACT_NUMBER_BOUND),
        message: bounded_text(message.into(), limits.diagnostic_budget()),
    }
}

pub(crate) fn bounded_preview(text: &str) -> String {
    bounded_text(text.to_owned(), 64)
}

fn bounded_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    if max_bytes == 0 {
        return String::new();
    }
    let suffix = if max_bytes >= 3 { "..." } else { "" };
    let mut end = max_bytes.saturating_sub(suffix.len()).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(suffix);
    text
}
