//! Runtime adapter for Boon's typed, transient outbound HTTP effect.

#![forbid(unsafe_code)]

use boon_http_client::{
    CancellationToken, EndpointName, ExecuteError, Header, HttpClient, HttpMethod, HttpRequest,
    HttpResponse, LimitKind, QueryParameter, RequestTimeouts, RequestViolation, TimeoutKind,
};
use boon_plan::{EffectId, EffectInvocationId, FiniteReal};
use boon_runtime::{
    ProgramSession, RuntimeTurn, TransientEffectCallId, TransientEffectInvocation, Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const MAX_DIAGNOSTIC_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationIntent {
    Independent,
    CancelPrevious,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpEffectCompletion {
    pub call_id: TransientEffectCallId,
    pub invocation_id: EffectInvocationId,
    pub outcome: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpEffectSubmission {
    pub call_id: TransientEffectCallId,
    pub cancelled_calls: Vec<TransientEffectCallId>,
    pub immediate_completion: Option<HttpEffectCompletion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterErrorKind {
    NotOwned,
    InvalidIntent,
    Capacity,
    Runtime,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterError {
    kind: AdapterErrorKind,
    diagnostic: String,
}

impl AdapterError {
    fn new(kind: AdapterErrorKind, diagnostic: impl fmt::Display) -> Self {
        Self {
            kind,
            diagnostic: bounded_diagnostic(diagnostic.to_string()),
        }
    }

    pub fn kind(&self) -> AdapterErrorKind {
        self.kind.clone()
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl std::error::Error for AdapterError {}

struct ActiveRequest {
    invocation_id: EffectInvocationId,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

/// Bounded async host lane for only the stable `Http/request` effect ID.
///
/// It never owns Boon authority. The runtime owner submits transient calls,
/// receives typed completions, then applies them through `ProgramSession`.
pub struct OutboundHttpEffectAdapter {
    client: HttpClient,
    effect_id: EffectId,
    max_active: usize,
    active: BTreeMap<TransientEffectCallId, ActiveRequest>,
    latest_by_invocation: BTreeMap<EffectInvocationId, TransientEffectCallId>,
    completions_tx: mpsc::Sender<HttpEffectCompletion>,
    completions_rx: mpsc::Receiver<HttpEffectCompletion>,
}

impl OutboundHttpEffectAdapter {
    pub fn new(client: HttpClient, max_active: usize) -> Result<Self, AdapterError> {
        if max_active == 0 {
            return Err(AdapterError::new(
                AdapterErrorKind::Capacity,
                "outbound HTTP active-request limit must be positive",
            ));
        }
        let effect_id =
            EffectId::from_host_operation(boon_effect_schema::OUTBOUND_HTTP_REQUEST_OPERATION)
                .map_err(|error| AdapterError::new(AdapterErrorKind::NotOwned, error))?;
        let (completions_tx, completions_rx) = mpsc::channel(max_active);
        Ok(Self {
            client,
            effect_id,
            max_active,
            active: BTreeMap::new(),
            latest_by_invocation: BTreeMap::new(),
            completions_tx,
            completions_rx,
        })
    }

    pub fn effect_id(&self) -> EffectId {
        self.effect_id
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn submit(
        &mut self,
        invocation: TransientEffectInvocation,
    ) -> Result<HttpEffectSubmission, AdapterError> {
        if invocation.effect_id != self.effect_id {
            return Err(AdapterError::new(
                AdapterErrorKind::NotOwned,
                format_args!(
                    "outbound HTTP adapter does not own effect {}",
                    invocation.effect_id
                ),
            ));
        }
        let decoded = match decode_intent(&invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => {
                return Ok(HttpEffectSubmission {
                    call_id: invocation.call_id,
                    cancelled_calls: Vec::new(),
                    immediate_completion: Some(HttpEffectCompletion {
                        call_id: invocation.call_id,
                        invocation_id: invocation.invocation_id,
                        outcome: failure_outcome(failure),
                    }),
                });
            }
        };

        let mut cancelled_calls = Vec::new();
        if decoded.cancellation == CancellationIntent::CancelPrevious
            && let Some(previous) = self
                .latest_by_invocation
                .get(&invocation.invocation_id)
                .copied()
            && self.cancel(previous)
        {
            cancelled_calls.push(previous);
        }
        if self.active.len() >= self.max_active {
            return Ok(HttpEffectSubmission {
                call_id: invocation.call_id,
                cancelled_calls,
                immediate_completion: Some(HttpEffectCompletion {
                    call_id: invocation.call_id,
                    invocation_id: invocation.invocation_id,
                    outcome: failure_outcome(Failure::new(
                        decoded.request.endpoint.as_str(),
                        "host_busy",
                        "outbound HTTP host is at its configured active-request limit",
                        true,
                        false,
                        false,
                    )),
                }),
            });
        }

        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let client = self.client.clone();
        let sender = self.completions_tx.clone();
        let call_id = invocation.call_id;
        let invocation_id = invocation.invocation_id;
        let task = tokio::spawn(async move {
            let endpoint = decoded.request.endpoint.as_str().to_owned();
            let outcome = match client
                .execute_with_timeouts(decoded.request, &task_cancellation, decoded.timeouts)
                .await
            {
                Ok(response) => success_outcome(response),
                Err(error) => failure_outcome(failure_from_execute_error(&endpoint, error)),
            };
            let _ = sender
                .send(HttpEffectCompletion {
                    call_id,
                    invocation_id,
                    outcome,
                })
                .await;
        });
        self.active.insert(
            call_id,
            ActiveRequest {
                invocation_id,
                cancellation,
                task,
            },
        );
        self.latest_by_invocation.insert(invocation_id, call_id);
        Ok(HttpEffectSubmission {
            call_id,
            cancelled_calls,
            immediate_completion: None,
        })
    }

    pub async fn next_completion(&mut self) -> Result<HttpEffectCompletion, AdapterError> {
        loop {
            let completion = self.completions_rx.recv().await.ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::Closed,
                    "outbound HTTP completion lane closed",
                )
            })?;
            let Some(active) = self.active.remove(&completion.call_id) else {
                continue;
            };
            if self.latest_by_invocation.get(&active.invocation_id) == Some(&completion.call_id) {
                self.latest_by_invocation.remove(&active.invocation_id);
            }
            return Ok(completion);
        }
    }

    pub fn cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        let Some(active) = self.active.remove(&call_id) else {
            return false;
        };
        active.cancellation.cancel();
        active.task.abort();
        if self.latest_by_invocation.get(&active.invocation_id) == Some(&call_id) {
            self.latest_by_invocation.remove(&active.invocation_id);
        }
        true
    }

    pub fn cancel_all(&mut self) -> Vec<TransientEffectCallId> {
        let calls = self.active.keys().copied().collect::<Vec<_>>();
        for call in &calls {
            self.cancel(*call);
        }
        calls
    }
}

impl Drop for OutboundHttpEffectAdapter {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

pub fn apply_submission(
    program: &mut ProgramSession,
    submission: HttpEffectSubmission,
) -> Result<Option<RuntimeTurn>, AdapterError> {
    for cancelled in submission.cancelled_calls {
        program
            .cancel_transient_effect(cancelled)
            .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))?;
    }
    submission
        .immediate_completion
        .map(|completion| apply_completion(program, completion))
        .transpose()
}

pub fn apply_completion(
    program: &mut ProgramSession,
    completion: HttpEffectCompletion,
) -> Result<RuntimeTurn, AdapterError> {
    program
        .complete_transient_effect(completion.call_id, completion.outcome)
        .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))
}

struct DecodedIntent {
    request: HttpRequest,
    timeouts: RequestTimeouts,
    cancellation: CancellationIntent,
}

fn decode_intent(value: &Value) -> Result<DecodedIntent, Failure> {
    let fields = record(value, "effect intent")?;
    require_exact_fields(
        fields,
        &[
            "body",
            "cancellation",
            "connect_timeout_ms",
            "endpoint",
            "headers",
            "method",
            "overall_timeout_ms",
            "path_segments",
            "query",
        ],
        "effect intent",
    )?;
    let endpoint_text = text_field(fields, "endpoint")?;
    let endpoint = EndpointName::new(endpoint_text.to_owned()).map_err(|_| {
        Failure::invalid(
            endpoint_text,
            "invalid_endpoint",
            "endpoint capability name is invalid",
        )
    })?;
    let method = match text_field(fields, "method")? {
        "Get" => HttpMethod::Get,
        "Head" => HttpMethod::Head,
        "Post" => HttpMethod::Post,
        "Put" => HttpMethod::Put,
        "Patch" => HttpMethod::Patch,
        "Delete" => HttpMethod::Delete,
        "Options" => HttpMethod::Options,
        _ => {
            return Err(Failure::invalid(
                endpoint.as_str(),
                "invalid_method",
                "HTTP method is outside the typed contract",
            ));
        }
    };
    let path_segments = text_list_field(fields, "path_segments")?;
    let query = text_pairs_field(fields, "query")?
        .into_iter()
        .map(|(name, value)| QueryParameter::new(name, value))
        .collect();
    let headers = byte_pairs_field(fields, "headers")?
        .into_iter()
        .map(|(name, value)| Header::new(name, value))
        .collect();
    let body = bytes_field(fields, "body")?.to_vec();
    let connect = duration_ms(
        number_field(fields, "connect_timeout_ms")?,
        endpoint.as_str(),
    )?;
    let overall = duration_ms(
        number_field(fields, "overall_timeout_ms")?,
        endpoint.as_str(),
    )?;
    if connect > overall {
        return Err(Failure::invalid(
            endpoint.as_str(),
            "invalid_timeouts",
            "connect timeout exceeds overall timeout",
        ));
    }
    let cancellation = match text_field(fields, "cancellation")? {
        "Independent" => CancellationIntent::Independent,
        "CancelPrevious" => CancellationIntent::CancelPrevious,
        _ => {
            return Err(Failure::invalid(
                endpoint.as_str(),
                "invalid_cancellation",
                "cancellation intent is outside the typed contract",
            ));
        }
    };
    let mut request = HttpRequest::new(endpoint, method);
    request.path_segments = path_segments;
    request.query = query;
    request.headers = headers;
    request.body = body;
    Ok(DecodedIntent {
        request,
        timeouts: RequestTimeouts { connect, overall },
        cancellation,
    })
}

fn success_outcome(response: HttpResponse) -> Value {
    tagged(
        "HttpSucceeded",
        BTreeMap::from([
            (
                "endpoint".to_owned(),
                Value::Text(response.final_endpoint.as_str().to_owned()),
            ),
            ("status".to_owned(), number(i64::from(response.status))),
            (
                "headers".to_owned(),
                Value::List(
                    response
                        .headers
                        .into_iter()
                        .map(|header| {
                            Value::Record(BTreeMap::from([
                                ("name".to_owned(), Value::Text(header.name)),
                                ("value".to_owned(), Value::Bytes(header.value)),
                            ]))
                        })
                        .collect(),
                ),
            ),
            ("body".to_owned(), Value::Bytes(response.body)),
            (
                "redirects_followed".to_owned(),
                number(i64::from(response.redirects_followed)),
            ),
        ]),
    )
}

#[derive(Clone, Debug)]
struct Failure {
    endpoint: String,
    code: &'static str,
    diagnostic: String,
    retryable: bool,
    timed_out: bool,
    cancelled: bool,
}

impl Failure {
    fn new(
        endpoint: impl Into<String>,
        code: &'static str,
        diagnostic: impl Into<String>,
        retryable: bool,
        timed_out: bool,
        cancelled: bool,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            code,
            diagnostic: bounded_diagnostic(diagnostic.into()),
            retryable,
            timed_out,
            cancelled,
        }
    }

    fn invalid(endpoint: &str, code: &'static str, diagnostic: &'static str) -> Self {
        Self::new(endpoint, code, diagnostic, false, false, false)
    }
}

fn failure_outcome(failure: Failure) -> Value {
    tagged(
        "HttpFailed",
        BTreeMap::from([
            ("endpoint".to_owned(), Value::Text(failure.endpoint)),
            ("code".to_owned(), Value::Text(failure.code.to_owned())),
            ("diagnostic".to_owned(), Value::Text(failure.diagnostic)),
            ("retryable".to_owned(), Value::Bool(failure.retryable)),
            ("timed_out".to_owned(), Value::Bool(failure.timed_out)),
            ("cancelled".to_owned(), Value::Bool(failure.cancelled)),
        ]),
    )
}

fn failure_from_execute_error(endpoint: &str, error: ExecuteError) -> Failure {
    match error {
        ExecuteError::Cancelled => Failure::new(
            endpoint,
            "cancelled",
            "outbound HTTP request was cancelled",
            false,
            false,
            true,
        ),
        ExecuteError::OverallTimeout => Failure::new(
            endpoint,
            "overall_timeout",
            "outbound HTTP request exceeded its overall timeout",
            true,
            true,
            false,
        ),
        ExecuteError::ConnectTimeout { .. } => Failure::new(
            endpoint,
            "connect_timeout",
            "outbound HTTP request exceeded its connection timeout",
            true,
            true,
            false,
        ),
        ExecuteError::InvalidTimeouts => Failure::invalid(
            endpoint,
            "invalid_timeouts",
            "outbound HTTP timeouts are invalid",
        ),
        ExecuteError::TimeoutLimitExceeded { kind, limit_ms } => Failure::new(
            endpoint,
            match kind {
                TimeoutKind::Connect => "connect_timeout_limit",
                TimeoutKind::Overall => "overall_timeout_limit",
            },
            format!("requested timeout exceeds configured maximum of {limit_ms} ms"),
            false,
            false,
            false,
        ),
        ExecuteError::UnknownEndpoint { .. } => Failure::invalid(
            endpoint,
            "unknown_endpoint",
            "named endpoint capability is not configured",
        ),
        ExecuteError::InvalidRequest(violation) => Failure::invalid(
            endpoint,
            request_violation_code(violation),
            "outbound HTTP request violates the configured transport contract",
        ),
        ExecuteError::LimitExceeded { kind, limit } => Failure::new(
            endpoint,
            limit_code(kind),
            format!("outbound HTTP request exceeded configured limit {limit}"),
            false,
            false,
            false,
        ),
        ExecuteError::RedirectDenied => Failure::invalid(
            endpoint,
            "redirect_denied",
            "endpoint capability denies redirects",
        ),
        ExecuteError::RedirectLimitExceeded { limit } => Failure::new(
            endpoint,
            "redirect_limit",
            format!("outbound HTTP redirect limit {limit} was exceeded"),
            false,
            false,
            false,
        ),
        ExecuteError::InvalidRedirect => Failure::invalid(
            endpoint,
            "invalid_redirect",
            "remote endpoint returned an invalid redirect",
        ),
        ExecuteError::RedirectDestinationNotAllowed => Failure::invalid(
            endpoint,
            "redirect_destination_denied",
            "redirect destination is outside named endpoint capabilities",
        ),
        ExecuteError::RedirectCredentialLeakPrevented => Failure::invalid(
            endpoint,
            "redirect_credential_leak_prevented",
            "redirect would move credentials or a body across capability boundaries",
        ),
        ExecuteError::DnsResolutionFailed { .. } => Failure::new(
            endpoint,
            "dns_failed",
            "named endpoint DNS resolution failed",
            true,
            false,
            false,
        ),
        ExecuteError::AddressPolicyDenied { .. } => Failure::invalid(
            endpoint,
            "address_denied",
            "resolved endpoint address is denied by capability policy",
        ),
        ExecuteError::ConnectFailed { .. } => Failure::new(
            endpoint,
            "connect_failed",
            "outbound HTTP connection failed",
            true,
            false,
            false,
        ),
        ExecuteError::TransportFailed { .. } => Failure::new(
            endpoint,
            "transport_failed",
            "outbound HTTP transport failed",
            true,
            false,
            false,
        ),
    }
}

fn request_violation_code(violation: RequestViolation) -> &'static str {
    match violation {
        RequestViolation::InvalidPathSegment => "invalid_path_segment",
        RequestViolation::InvalidHeaderName => "invalid_header_name",
        RequestViolation::InvalidHeaderValue => "invalid_header_value",
        RequestViolation::ForbiddenHeader => "forbidden_header",
        RequestViolation::BodyNotAllowedForMethod => "body_not_allowed",
    }
}

fn limit_code(kind: LimitKind) -> &'static str {
    match kind {
        LimitKind::RequestBodyBytes => "request_body_limit",
        LimitKind::RequestHeaderCount => "request_header_count_limit",
        LimitKind::RequestHeaderBytes => "request_header_bytes_limit",
        LimitKind::ResponseBodyBytes => "response_body_limit",
        LimitKind::ResponseHeaderCount => "response_header_count_limit",
        LimitKind::ResponseHeaderBytes => "response_header_bytes_limit",
        LimitKind::UrlBytes => "url_limit",
    }
}

fn tagged(tag: &str, mut fields: BTreeMap<String, Value>) -> Value {
    fields.insert("$tag".to_owned(), Value::Text(tag.to_owned()));
    Value::Record(fields)
}

fn number(value: i64) -> Value {
    Value::Number(FiniteReal::from_i64_exact(value).expect("small HTTP integer is exact"))
}

fn duration_ms(value: FiniteReal, endpoint: &str) -> Result<Duration, Failure> {
    let milliseconds = value
        .to_i64_exact()
        .ok()
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            Failure::invalid(
                endpoint,
                "invalid_timeout",
                "timeout must be a positive whole millisecond count",
            )
        })?;
    Ok(Duration::from_millis(milliseconds))
}

fn record<'a>(value: &'a Value, path: &str) -> Result<&'a BTreeMap<String, Value>, Failure> {
    let Value::Record(fields) = value else {
        return Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            "outbound HTTP effect intent is not a record",
        ));
    };
    let _ = path;
    Ok(fields)
}

fn require_exact_fields(
    fields: &BTreeMap<String, Value>,
    expected: &[&str],
    path: &str,
) -> Result<(), Failure> {
    let actual = fields.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            if path.is_empty() {
                "outbound HTTP record fields differ from the typed contract"
            } else {
                "outbound HTTP effect intent fields differ from the typed contract"
            },
        ))
    }
}

fn text_field<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a str, Failure> {
    match fields.get(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            "outbound HTTP text field has the wrong type",
        )),
    }
}

fn number_field(fields: &BTreeMap<String, Value>, name: &str) -> Result<FiniteReal, Failure> {
    match fields.get(name) {
        Some(Value::Number(value)) => Ok(*value),
        _ => Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            "outbound HTTP number field has the wrong type",
        )),
    }
}

fn bytes_field<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a [u8], Failure> {
    match fields.get(name) {
        Some(Value::Bytes(value)) => Ok(value),
        _ => Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            "outbound HTTP bytes field has the wrong type",
        )),
    }
}

fn text_list_field(fields: &BTreeMap<String, Value>, name: &str) -> Result<Vec<String>, Failure> {
    let Some(Value::List(values)) = fields.get(name) else {
        return Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            "outbound HTTP text-list field has the wrong type",
        ));
    };
    values
        .iter()
        .map(|value| match value {
            Value::Text(value) => Ok(value.clone()),
            _ => Err(Failure::invalid(
                "unresolved",
                "invalid_intent",
                "outbound HTTP text-list item has the wrong type",
            )),
        })
        .collect()
}

fn text_pairs_field(
    fields: &BTreeMap<String, Value>,
    name: &str,
) -> Result<Vec<(String, String)>, Failure> {
    pair_records(fields, name, |fields| {
        Ok((
            text_field(fields, "name")?.to_owned(),
            text_field(fields, "value")?.to_owned(),
        ))
    })
}

fn byte_pairs_field(
    fields: &BTreeMap<String, Value>,
    name: &str,
) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    pair_records(fields, name, |fields| {
        Ok((
            text_field(fields, "name")?.to_owned(),
            bytes_field(fields, "value")?.to_vec(),
        ))
    })
}

fn pair_records<T>(
    fields: &BTreeMap<String, Value>,
    name: &str,
    decode: impl Fn(&BTreeMap<String, Value>) -> Result<T, Failure>,
) -> Result<Vec<T>, Failure> {
    let Some(Value::List(values)) = fields.get(name) else {
        return Err(Failure::invalid(
            "unresolved",
            "invalid_intent",
            "outbound HTTP pair-list field has the wrong type",
        ));
    };
    values
        .iter()
        .map(|value| {
            let fields = record(value, name)?;
            require_exact_fields(fields, &["name", "value"], name)?;
            decode(fields)
        })
        .collect()
}

fn bounded_diagnostic(mut diagnostic: String) -> String {
    if diagnostic.len() <= MAX_DIAGNOSTIC_BYTES {
        return diagnostic;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !diagnostic.is_char_boundary(end) {
        end -= 1;
    }
    diagnostic.truncate(end);
    diagnostic
}
