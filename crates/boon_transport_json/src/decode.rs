use crate::{
    Diagnostic, DiagnosticCode, FiniteReal, Limits, Value, bounded_preview, make_diagnostic,
};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::collections::BTreeMap;
use std::fmt;

pub fn decode(input: &[u8], limits: &Limits) -> Result<Value, Diagnostic> {
    limits.validate()?;
    if input.len() > limits.max_input_bytes {
        return Err(make_diagnostic(
            DiagnosticCode::InputTooLarge,
            limits.max_input_bytes,
            format!("JSON input exceeds {} byte limit", limits.max_input_bytes),
            limits,
        ));
    }
    let text = std::str::from_utf8(input).map_err(|error| {
        make_diagnostic(
            DiagnosticCode::InvalidUtf8,
            error.valid_up_to(),
            "JSON input is not valid UTF-8",
            limits,
        )
    })?;

    let mut state = DecodeState {
        limits,
        nodes: 0,
        pending: None,
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    let decoded = ValueSeed {
        state: &mut state,
        depth: 0,
    }
    .deserialize(&mut deserializer);

    match decoded {
        Ok(value) => deserializer
            .end()
            .map(|()| value)
            .map_err(|error| parser_diagnostic(text, error, limits, None)),
        Err(error) => Err(parser_diagnostic(text, error, limits, state.pending.take())),
    }
}

#[derive(Debug)]
struct PendingDiagnostic {
    code: DiagnosticCode,
    message: String,
}

struct DecodeState<'a> {
    limits: &'a Limits,
    nodes: usize,
    pending: Option<PendingDiagnostic>,
}

impl DecodeState<'_> {
    fn enter_node<E: de::Error>(&mut self, depth: usize) -> Result<(), E> {
        if depth > self.limits.max_depth {
            return self.reject(
                DiagnosticCode::DepthLimit,
                format!("JSON nesting depth exceeds limit {}", self.limits.max_depth),
            );
        }
        if self.nodes >= self.limits.max_nodes {
            return self.reject(
                DiagnosticCode::NodeLimit,
                format!("JSON node count exceeds limit {}", self.limits.max_nodes),
            );
        }
        self.nodes += 1;
        Ok(())
    }

    fn check_string<E: de::Error>(&mut self, value: &str) -> Result<(), E> {
        if value.len() > self.limits.max_string_bytes {
            return self.reject(
                DiagnosticCode::StringLimit,
                format!(
                    "decoded JSON string exceeds {} byte limit",
                    self.limits.max_string_bytes
                ),
            );
        }
        Ok(())
    }

    fn reject<T, E: de::Error>(&mut self, code: DiagnosticCode, message: String) -> Result<T, E> {
        self.pending = Some(PendingDiagnostic { code, message });
        Err(E::custom("bounded JSON boundary rejected input"))
    }
}

struct ValueSeed<'state, 'limits> {
    state: &'state mut DecodeState<'limits>,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.state.enter_node(self.depth)?;
        deserializer.deserialize_any(ValueVisitor {
            state: self.state,
            depth: self.depth,
        })
    }
}

struct ValueVisitor<'state, 'limits> {
    state: &'state mut DecodeState<'limits>,
    depth: usize,
}

impl<'de> Visitor<'de> for ValueVisitor<'_, '_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.finite_integer(value)
    }

    fn visit_u64<E: de::Error>(mut self, value: u64) -> Result<Self::Value, E> {
        let value = i64::try_from(value).map_err(|_| {
            self.number_error::<E>("JSON integer is outside canonical Boon Number range")
        })?;
        self.finite_integer(value)
    }

    fn visit_i128<E: de::Error>(mut self, value: i128) -> Result<Self::Value, E> {
        let value = i64::try_from(value).map_err(|_| {
            self.number_error::<E>("JSON integer is outside canonical Boon Number range")
        })?;
        self.finite_integer(value)
    }

    fn visit_u128<E: de::Error>(mut self, value: u128) -> Result<Self::Value, E> {
        let value = i64::try_from(value).map_err(|_| {
            self.number_error::<E>("JSON integer is outside canonical Boon Number range")
        })?;
        self.finite_integer(value)
    }

    fn visit_f64<E: de::Error>(mut self, value: f64) -> Result<Self::Value, E> {
        FiniteReal::new(value).map(Value::Number).map_err(|_| {
            self.number_error::<E>("JSON number is not a finite canonical Boon Number")
        })
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
        self.state.check_string(value)?;
        Ok(Value::Text(value.to_owned()))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.state.check_string(value)?;
        Ok(Value::Text(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        self.state.check_string(&value)?;
        Ok(Value::Text(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        if sequence
            .size_hint()
            .is_some_and(|hint| hint > self.state.limits.max_array_items)
        {
            return self.state.reject(
                DiagnosticCode::ArrayItemsLimit,
                format!(
                    "JSON array exceeds {} item limit",
                    self.state.limits.max_array_items
                ),
            );
        }
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            state: self.state,
            depth: self.depth + 1,
        })? {
            if values.len() >= self.state.limits.max_array_items {
                return self.state.reject(
                    DiagnosticCode::ArrayItemsLimit,
                    format!(
                        "JSON array exceeds {} item limit",
                        self.state.limits.max_array_items
                    ),
                );
            }
            values.push(value);
        }
        Ok(Value::List(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
        if object
            .size_hint()
            .is_some_and(|hint| hint > self.state.limits.max_object_fields)
        {
            return self.state.reject(
                DiagnosticCode::ObjectFieldsLimit,
                format!(
                    "JSON object exceeds {} field limit",
                    self.state.limits.max_object_fields
                ),
            );
        }
        let mut fields = BTreeMap::new();
        while let Some(key) = object.next_key::<String>()? {
            self.state.check_string(&key)?;
            if fields.len() >= self.state.limits.max_object_fields {
                return self.state.reject(
                    DiagnosticCode::ObjectFieldsLimit,
                    format!(
                        "JSON object exceeds {} field limit",
                        self.state.limits.max_object_fields
                    ),
                );
            }
            if fields.contains_key(&key) {
                return self.state.reject(
                    DiagnosticCode::DuplicateKey,
                    format!("duplicate JSON object key `{}`", bounded_preview(&key)),
                );
            }
            let value = object.next_value_seed(ValueSeed {
                state: self.state,
                depth: self.depth + 1,
            })?;
            fields.insert(key, value);
        }
        Ok(Value::Record(fields))
    }
}

impl ValueVisitor<'_, '_> {
    fn finite_integer<E: de::Error>(mut self, value: i64) -> Result<Value, E> {
        FiniteReal::from_i64_exact(value)
            .map(Value::Number)
            .map_err(|_| {
                self.number_error::<E>(
                    "JSON integer cannot be represented exactly as a canonical Boon Number",
                )
            })
    }

    fn number_error<E: de::Error>(&mut self, message: &str) -> E {
        self.state.pending = Some(PendingDiagnostic {
            code: DiagnosticCode::NumberOutOfRange,
            message: message.to_owned(),
        });
        E::custom("bounded JSON boundary rejected number")
    }
}

fn parser_diagnostic(
    input: &str,
    error: serde_json::Error,
    limits: &Limits,
    pending: Option<PendingDiagnostic>,
) -> Diagnostic {
    let offset = line_column_offset(input, error.line(), error.column());
    if let Some(pending) = pending {
        return make_diagnostic(pending.code, offset, pending.message, limits);
    }
    let message = error.to_string();
    let code = if message.contains("number out of range") {
        DiagnosticCode::NumberOutOfRange
    } else {
        DiagnosticCode::InvalidSyntax
    };
    make_diagnostic(code, offset, message, limits)
}

fn line_column_offset(input: &str, line: usize, column: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut current_line = 1;
    let mut line_start = 0;
    for (index, byte) in input.bytes().enumerate() {
        if current_line == line {
            break;
        }
        if byte == b'\n' {
            current_line += 1;
            line_start = index + 1;
        }
    }
    if current_line != line {
        return input.len();
    }
    line_start.saturating_add(column).min(input.len())
}
