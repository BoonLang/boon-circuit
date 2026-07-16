use crate::{Diagnostic, DiagnosticCode, Limits, Value, make_diagnostic};

pub fn encode(value: &Value, limits: &Limits) -> Result<Vec<u8>, Diagnostic> {
    limits.validate()?;
    let mut encoder = Encoder {
        bytes: Vec::new(),
        limits,
        nodes: 0,
    };
    encoder.value(value, 0)?;
    Ok(encoder.bytes)
}

struct Encoder<'a> {
    bytes: Vec<u8>,
    limits: &'a Limits,
    nodes: usize,
}

impl Encoder<'_> {
    fn value(&mut self, value: &Value, depth: usize) -> Result<(), Diagnostic> {
        self.enter_node(depth)?;
        match value {
            Value::Null => self.push(b"null"),
            Value::Bool(false) => self.push(b"false"),
            Value::Bool(true) => self.push(b"true"),
            Value::Number(value) => {
                if !value.get().is_finite() {
                    return Err(self.error(
                        DiagnosticCode::NumberOutOfRange,
                        "Boon Number is not finite at JSON boundary",
                    ));
                }
                self.push(value.to_string().as_bytes())
            }
            Value::Text(value) => {
                self.check_string(value)?;
                self.string(value)
            }
            Value::List(values) => {
                if values.len() > self.limits.max_array_items {
                    return Err(self.error(
                        DiagnosticCode::ArrayItemsLimit,
                        format!(
                            "JSON array exceeds {} item limit",
                            self.limits.max_array_items
                        ),
                    ));
                }
                self.push(b"[")?;
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        self.push(b",")?;
                    }
                    self.value(value, depth + 1)?;
                }
                self.push(b"]")
            }
            Value::Record(fields) => {
                if fields.len() > self.limits.max_object_fields {
                    return Err(self.error(
                        DiagnosticCode::ObjectFieldsLimit,
                        format!(
                            "JSON object exceeds {} field limit",
                            self.limits.max_object_fields
                        ),
                    ));
                }
                self.push(b"{")?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    self.check_string(key)?;
                    if index != 0 {
                        self.push(b",")?;
                    }
                    self.string(key)?;
                    self.push(b":")?;
                    self.value(value, depth + 1)?;
                }
                self.push(b"}")
            }
            Value::Variant { .. } => Err(self.error(
                DiagnosticCode::UnsupportedValue,
                "Boon Variant has no implicit JSON wire representation; map its tag and fields to Text/Record in Boon",
            )),
            Value::Bytes(_) => Err(self.error(
                DiagnosticCode::UnsupportedValue,
                "Boon Bytes has no implicit JSON wire representation; map it to Text/list in Boon",
            )),
            Value::Error { .. } => Err(self.error(
                DiagnosticCode::UnsupportedValue,
                "Boon Error has no implicit JSON wire representation; map it to a wire record in Boon",
            )),
        }
    }

    fn enter_node(&mut self, depth: usize) -> Result<(), Diagnostic> {
        if depth > self.limits.max_depth {
            return Err(self.error(
                DiagnosticCode::DepthLimit,
                format!("JSON nesting depth exceeds limit {}", self.limits.max_depth),
            ));
        }
        if self.nodes >= self.limits.max_nodes {
            return Err(self.error(
                DiagnosticCode::NodeLimit,
                format!("JSON node count exceeds limit {}", self.limits.max_nodes),
            ));
        }
        self.nodes += 1;
        Ok(())
    }

    fn check_string(&self, value: &str) -> Result<(), Diagnostic> {
        if value.len() > self.limits.max_string_bytes {
            return Err(self.error(
                DiagnosticCode::StringLimit,
                format!(
                    "JSON string exceeds {} decoded byte limit",
                    self.limits.max_string_bytes
                ),
            ));
        }
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), Diagnostic> {
        self.push(b"\"")?;
        let mut unescaped_start = 0;
        for (index, byte) in value.bytes().enumerate() {
            let escape = match byte {
                b'\"' => Some(b"\\\"".as_slice()),
                b'\\' => Some(b"\\\\".as_slice()),
                b'\x08' => Some(b"\\b".as_slice()),
                b'\x0c' => Some(b"\\f".as_slice()),
                b'\n' => Some(b"\\n".as_slice()),
                b'\r' => Some(b"\\r".as_slice()),
                b'\t' => Some(b"\\t".as_slice()),
                0x00..=0x1f => {
                    if unescaped_start < index {
                        self.push(&value.as_bytes()[unescaped_start..index])?;
                    }
                    let escaped = [
                        b'\\',
                        b'u',
                        b'0',
                        b'0',
                        hex_digit(byte >> 4),
                        hex_digit(byte & 0x0f),
                    ];
                    self.push(&escaped)?;
                    unescaped_start = index + 1;
                    continue;
                }
                _ => None,
            };
            if let Some(escape) = escape {
                if unescaped_start < index {
                    self.push(&value.as_bytes()[unescaped_start..index])?;
                }
                self.push(escape)?;
                unescaped_start = index + 1;
            }
        }
        if unescaped_start < value.len() {
            self.push(&value.as_bytes()[unescaped_start..])?;
        }
        self.push(b"\"")
    }

    fn push(&mut self, bytes: &[u8]) -> Result<(), Diagnostic> {
        let remaining = self
            .limits
            .max_output_bytes
            .saturating_sub(self.bytes.len());
        if bytes.len() > remaining {
            return Err(make_diagnostic(
                DiagnosticCode::OutputTooLarge,
                self.limits.max_output_bytes,
                format!(
                    "JSON output exceeds {} byte limit",
                    self.limits.max_output_bytes
                ),
                self.limits,
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn error(&self, code: DiagnosticCode, message: impl Into<String>) -> Diagnostic {
        make_diagnostic(code, self.bytes.len(), message, self.limits)
    }
}

const fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        _ => b'a' + (value - 10),
    }
}
