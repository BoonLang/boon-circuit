use crate::machine::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectStreamValidationError {
    detail: String,
}

impl EffectStreamValidationError {
    fn new(detail: impl ToString) -> Self {
        Self {
            detail: detail.to_string(),
        }
    }
}

impl fmt::Display for EffectStreamValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for EffectStreamValidationError {}

/// Stateful semantic validator for the canonical bounded byte-stream result.
#[derive(Clone, Debug)]
pub struct ByteStreamValidator {
    max_chunk_bytes: usize,
    expected_result_sequence: u64,
    opened_size: Option<u64>,
    next_chunk_sequence: u64,
    next_offset: u64,
    terminal: bool,
}

impl ByteStreamValidator {
    pub fn new(max_chunk_bytes: usize) -> Result<Self, EffectStreamValidationError> {
        if max_chunk_bytes == 0 {
            return Err(EffectStreamValidationError::new(
                "byte stream chunk limit must be positive",
            ));
        }
        Ok(Self {
            max_chunk_bytes,
            expected_result_sequence: 0,
            opened_size: None,
            next_chunk_sequence: 0,
            next_offset: 0,
            terminal: false,
        })
    }

    pub fn accept(
        &mut self,
        result_sequence: u64,
        outcome: &Value,
        terminal: bool,
    ) -> Result<(), EffectStreamValidationError> {
        if self.terminal {
            return Err(EffectStreamValidationError::new(
                "byte stream emitted a result after its terminal result",
            ));
        }
        expect_result_sequence(
            "byte stream",
            self.expected_result_sequence,
            result_sequence,
        )?;
        let fields = record_fields(outcome, "byte stream result")?;
        let tag = text_field(fields, "$tag", "byte stream result")?;
        match tag {
            "Opened" => {
                if terminal || self.opened_size.is_some() || result_sequence != 0 {
                    return Err(EffectStreamValidationError::new(
                        "byte stream Opened must be its one non-terminal first result",
                    ));
                }
                self.opened_size = Some(number_field(fields, "size", "byte stream result")?);
            }
            "Chunk" => {
                if terminal || self.opened_size.is_none() {
                    return Err(EffectStreamValidationError::new(
                        "byte stream Chunk must follow Opened and be non-terminal",
                    ));
                }
                let sequence = number_field(fields, "sequence", "byte stream result")?;
                if sequence != self.next_chunk_sequence {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "byte stream expected chunk sequence {}, received {sequence}",
                        self.next_chunk_sequence
                    )));
                }
                let offset = number_field(fields, "offset", "byte stream result")?;
                if offset != self.next_offset {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "byte stream expected chunk offset {}, received {offset}",
                        self.next_offset
                    )));
                }
                let bytes = bytes_field(fields, "bytes", "byte stream result")?;
                if bytes.is_empty() || bytes.len() > self.max_chunk_bytes {
                    return Err(EffectStreamValidationError::new(
                        "byte stream Chunk byte length is zero or exceeds its declared limit",
                    ));
                }
                let byte_count = u64::try_from(bytes.len()).map_err(|_| {
                    EffectStreamValidationError::new("byte stream chunk length exceeds u64")
                })?;
                self.next_offset = self.next_offset.checked_add(byte_count).ok_or_else(|| {
                    EffectStreamValidationError::new("byte stream byte offset overflow")
                })?;
                if self.next_offset > self.opened_size.expect("opened size was checked") {
                    return Err(EffectStreamValidationError::new(
                        "byte stream chunks exceed the size declared by Opened",
                    ));
                }
                self.next_chunk_sequence =
                    self.next_chunk_sequence.checked_add(1).ok_or_else(|| {
                        EffectStreamValidationError::new("byte stream chunk sequence overflow")
                    })?;
            }
            "Finished" => {
                let Some(opened_size) = self.opened_size else {
                    return Err(EffectStreamValidationError::new(
                        "byte stream Finished must follow Opened",
                    ));
                };
                if !terminal
                    || number_field(fields, "byte_count", "byte stream result")? != self.next_offset
                    || self.next_offset != opened_size
                    || bytes_field(fields, "digest", "byte stream result")?.len() != 32
                {
                    return Err(EffectStreamValidationError::new(
                        "byte stream Finished does not match its declared size, chunks, or digest contract",
                    ));
                }
            }
            "Failed" | "Cancelled" => {
                if !terminal {
                    return Err(EffectStreamValidationError::new(
                        "byte stream failure or cancellation must be terminal",
                    ));
                }
            }
            _ => {
                return Err(EffectStreamValidationError::new(format_args!(
                    "byte stream emitted unsupported result tag `{tag}`"
                )));
            }
        }
        self.expected_result_sequence =
            next_result_sequence("byte stream", self.expected_result_sequence)?;
        self.terminal = terminal;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ContentProgressValidator {
    operation: &'static str,
    expected_result_sequence: u64,
    expected_total: Option<u64>,
    started_total: Option<u64>,
    completed_bytes: u64,
    terminal: bool,
}

impl ContentProgressValidator {
    fn new(operation: &'static str, expected_total: Option<u64>) -> Self {
        Self {
            operation,
            expected_result_sequence: 0,
            expected_total,
            started_total: None,
            completed_bytes: 0,
            terminal: false,
        }
    }

    fn accept(
        &mut self,
        result_sequence: u64,
        outcome: &Value,
        terminal: bool,
    ) -> Result<(), EffectStreamValidationError> {
        if self.terminal {
            return Err(EffectStreamValidationError::new(format_args!(
                "{} emitted a result after its terminal result",
                self.operation
            )));
        }
        expect_result_sequence(
            self.operation,
            self.expected_result_sequence,
            result_sequence,
        )?;
        let fields = record_fields(outcome, self.operation)?;
        let tag = text_field(fields, "$tag", self.operation)?;
        match tag {
            "Started" => {
                if terminal || self.started_total.is_some() || result_sequence != 0 {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "{} Started must be its one non-terminal first result",
                        self.operation
                    )));
                }
                let total = number_field(fields, "byte_count", self.operation)?;
                if self
                    .expected_total
                    .is_some_and(|expected| expected != total)
                {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "{} Started byte count does not match its content reference",
                        self.operation
                    )));
                }
                self.started_total = Some(total);
            }
            "Progress" => {
                let Some(total) = self.started_total else {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "{} Progress must follow Started",
                        self.operation
                    )));
                };
                if terminal || number_field(fields, "total_bytes", self.operation)? != total {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "{} Progress must be non-terminal and keep one total byte count",
                        self.operation
                    )));
                }
                let completed = number_field(fields, "completed_bytes", self.operation)?;
                if completed < self.completed_bytes || completed > total {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "{} Progress must be monotonic and bounded by its total",
                        self.operation
                    )));
                }
                self.completed_bytes = completed;
            }
            "Imported" => {
                let Some(total) = self.started_total else {
                    return Err(EffectStreamValidationError::new(
                        "Content/import Imported must follow Started",
                    ));
                };
                let content = record_field(fields, "content", self.operation)?;
                if self.operation != boon_effect_schema::CONTENT_IMPORT_OPERATION
                    || !terminal
                    || number_field(content, "size", "Content/import content")? != total
                    || bytes_field(content, "digest", "Content/import content")?.len() != 32
                {
                    return Err(EffectStreamValidationError::new(
                        "Content/import Imported does not match its declared content",
                    ));
                }
            }
            "Saved" => {
                let Some(total) = self.started_total else {
                    return Err(EffectStreamValidationError::new(
                        "Content/save Saved must follow Started",
                    ));
                };
                if self.operation != boon_effect_schema::CONTENT_SAVE_OPERATION
                    || !terminal
                    || number_field(fields, "byte_count", self.operation)? != total
                {
                    return Err(EffectStreamValidationError::new(
                        "Content/save Saved does not match its declared content",
                    ));
                }
            }
            "Busy" | "Failed" | "Cancelled" => {
                if !terminal {
                    return Err(EffectStreamValidationError::new(format_args!(
                        "{} failure, cancellation, or busy result must be terminal",
                        self.operation
                    )));
                }
            }
            _ => {
                return Err(EffectStreamValidationError::new(format_args!(
                    "{} emitted unsupported result tag `{tag}`",
                    self.operation
                )));
            }
        }
        self.expected_result_sequence =
            next_result_sequence(self.operation, self.expected_result_sequence)?;
        self.terminal = terminal;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) enum TransientEffectSemanticValidator {
    None,
    ByteStream(ByteStreamValidator),
    ContentProgress(ContentProgressValidator),
}

impl TransientEffectSemanticValidator {
    pub(crate) fn for_invocation(
        operation: &str,
        intent: &Value,
    ) -> Result<Self, EffectStreamValidationError> {
        match operation {
            boon_effect_schema::FILE_READ_STREAM_OPERATION => {
                let intent = record_fields(intent, "File/read_stream intent")?;
                let chunk_bytes = number_field(intent, "chunk_bytes", "File/read_stream intent")?;
                let chunk_bytes = usize::try_from(chunk_bytes).map_err(|_| {
                    EffectStreamValidationError::new(
                        "File/read_stream chunk_bytes exceeds the host address space",
                    )
                })?;
                Ok(Self::ByteStream(ByteStreamValidator::new(chunk_bytes)?))
            }
            boon_effect_schema::CONTENT_IMPORT_OPERATION => Ok(Self::ContentProgress(
                ContentProgressValidator::new(boon_effect_schema::CONTENT_IMPORT_OPERATION, None),
            )),
            boon_effect_schema::CONTENT_SAVE_OPERATION => {
                let intent = record_fields(intent, "Content/save intent")?;
                let content = record_field(intent, "content", "Content/save intent")?;
                let size = number_field(content, "size", "Content/save content")?;
                Ok(Self::ContentProgress(ContentProgressValidator::new(
                    boon_effect_schema::CONTENT_SAVE_OPERATION,
                    Some(size),
                )))
            }
            _ => Ok(Self::None),
        }
    }

    pub(crate) fn accept(
        &mut self,
        result_sequence: u64,
        outcome: &Value,
        terminal: bool,
    ) -> Result<(), EffectStreamValidationError> {
        match self {
            Self::None => Ok(()),
            Self::ByteStream(validator) => validator.accept(result_sequence, outcome, terminal),
            Self::ContentProgress(validator) => {
                validator.accept(result_sequence, outcome, terminal)
            }
        }
    }
}

fn expect_result_sequence(
    operation: &str,
    expected: u64,
    actual: u64,
) -> Result<(), EffectStreamValidationError> {
    if actual != expected {
        return Err(EffectStreamValidationError::new(format_args!(
            "{operation} expected result sequence {expected}, received {actual}"
        )));
    }
    Ok(())
}

fn next_result_sequence(operation: &str, current: u64) -> Result<u64, EffectStreamValidationError> {
    current.checked_add(1).ok_or_else(|| {
        EffectStreamValidationError::new(format_args!("{operation} result sequence overflow"))
    })
}

fn record_fields<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a BTreeMap<String, Value>, EffectStreamValidationError> {
    let Value::Record(fields) = value else {
        return Err(EffectStreamValidationError::new(format_args!(
            "{context} must be a record"
        )));
    };
    Ok(fields)
}

fn record_field<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
    context: &str,
) -> Result<&'a BTreeMap<String, Value>, EffectStreamValidationError> {
    let Some(value) = fields.get(name) else {
        return Err(EffectStreamValidationError::new(format_args!(
            "{context} field `{name}` is missing"
        )));
    };
    record_fields(value, &format!("{context} field `{name}`"))
}

fn text_field<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
    context: &str,
) -> Result<&'a str, EffectStreamValidationError> {
    let Some(Value::Text(value)) = fields.get(name) else {
        return Err(EffectStreamValidationError::new(format_args!(
            "{context} field `{name}` must be Text"
        )));
    };
    Ok(value)
}

fn number_field(
    fields: &BTreeMap<String, Value>,
    name: &str,
    context: &str,
) -> Result<u64, EffectStreamValidationError> {
    let Some(Value::Number(value)) = fields.get(name) else {
        return Err(EffectStreamValidationError::new(format_args!(
            "{context} field `{name}` must be Number"
        )));
    };
    let value = value.to_i64_exact().map_err(|_| {
        EffectStreamValidationError::new(format_args!(
            "{context} field `{name}` must be an exact non-negative integer"
        ))
    })?;
    u64::try_from(value).map_err(|_| {
        EffectStreamValidationError::new(format_args!(
            "{context} field `{name}` must be non-negative"
        ))
    })
}

fn bytes_field<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
    context: &str,
) -> Result<&'a [u8], EffectStreamValidationError> {
    let Some(Value::Bytes(value)) = fields.get(name) else {
        return Err(EffectStreamValidationError::new(format_args!(
            "{context} field `{name}` must be Bytes"
        )));
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_progress_rejects_unstable_totals() {
        let mut validator =
            ContentProgressValidator::new(boon_effect_schema::CONTENT_IMPORT_OPERATION, None);
        validator
            .accept(0, &tagged("Started", [("byte_count", number(4))]), false)
            .unwrap();
        let error = validator
            .accept(
                1,
                &tagged(
                    "Progress",
                    [("completed_bytes", number(2)), ("total_bytes", number(5))],
                ),
                false,
            )
            .unwrap_err();
        assert!(error.to_string().contains("one total byte count"));
    }

    #[test]
    fn content_save_requires_the_content_reference_size() {
        let mut validator =
            ContentProgressValidator::new(boon_effect_schema::CONTENT_SAVE_OPERATION, Some(4));
        let error = validator
            .accept(0, &tagged("Started", [("byte_count", number(3))]), false)
            .unwrap_err();
        assert!(error.to_string().contains("content reference"));
    }

    fn tagged<const N: usize>(tag: &str, fields: [(&str, Value); N]) -> Value {
        let mut record = BTreeMap::from([("$tag".to_owned(), Value::Text(tag.to_owned()))]);
        record.extend(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value)),
        );
        Value::Record(record)
    }

    fn number(value: i64) -> Value {
        Value::integer(value).unwrap()
    }
}
