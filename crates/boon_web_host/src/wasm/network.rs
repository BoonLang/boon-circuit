use super::{js_error, js_message, window};
use crate::{
    BrowserFetchCapabilities, BrowserFetchOrigin, BrowserFetchRequest, BrowserFetchResponse,
    BrowserWebSocketCapabilities, BrowserWebSocketRequest, HeaderValue, SocketFrame,
    ValidatedFetchRequest, ValidatedWebSocketRequest, WebHostError, WebHostResult,
};
use js_sys::{Array, Reflect, Uint8Array};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, BinaryType, CloseEvent, Event, EventTarget, MessageEvent, Request,
    RequestCredentials, RequestInit, RequestMode, RequestRedirect, Response, WebSocket,
};

pub struct BrowserFetchAdapter {
    capabilities: BrowserFetchCapabilities,
    active: Rc<Cell<usize>>,
    max_in_flight: usize,
}

impl BrowserFetchAdapter {
    pub fn new(
        capabilities: BrowserFetchCapabilities,
        max_in_flight: usize,
    ) -> WebHostResult<Self> {
        if max_in_flight == 0 {
            return Err(WebHostError::InvalidInput {
                field: "fetch max_in_flight".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        Ok(Self {
            capabilities,
            active: Rc::new(Cell::new(0)),
            max_in_flight,
        })
    }

    pub async fn execute(
        &self,
        request: BrowserFetchRequest,
    ) -> WebHostResult<BrowserFetchResponse> {
        let validated = self.capabilities.validate_request(request)?;
        let _permit = acquire_permit(&self.active, self.max_in_flight, "browser fetches")?;
        let cancellation = BrowserFetchCancellation::new()?;
        execute_validated_fetch(validated, cancellation.controller.clone()).await
    }

    pub async fn execute_cancellable(
        &self,
        request: BrowserFetchRequest,
        cancellation: &BrowserFetchCancellation,
    ) -> WebHostResult<BrowserFetchResponse> {
        if cancellation.is_cancelled() {
            return Err(WebHostError::platform(
                "browser fetch",
                "request was cancelled before admission",
            ));
        }
        let validated = self.capabilities.validate_request(request)?;
        let _permit = acquire_permit(&self.active, self.max_in_flight, "browser fetches")?;
        execute_validated_fetch(validated, cancellation.controller.clone()).await
    }

    pub fn active_request_count(&self) -> usize {
        self.active.get()
    }
}

struct HostResourcePermit {
    active: Rc<Cell<usize>>,
}

impl Drop for HostResourcePermit {
    fn drop(&mut self) {
        self.active.set(self.active.get().saturating_sub(1));
    }
}

fn acquire_permit(
    active: &Rc<Cell<usize>>,
    limit: usize,
    resource: &str,
) -> WebHostResult<HostResourcePermit> {
    if active.get() >= limit {
        return Err(WebHostError::QueueOverflow {
            queue: resource.to_owned(),
            capacity: limit,
        });
    }
    active.set(active.get() + 1);
    Ok(HostResourcePermit {
        active: Rc::clone(active),
    })
}

#[derive(Clone)]
pub struct BrowserFetchCancellation {
    controller: AbortController,
}

impl BrowserFetchCancellation {
    pub fn new() -> WebHostResult<Self> {
        Ok(Self {
            controller: AbortController::new()
                .map_err(|error| js_error("create fetch cancellation", error))?,
        })
    }

    pub fn cancel(&self) {
        self.controller.abort();
    }

    pub fn is_cancelled(&self) -> bool {
        self.controller.signal().aborted()
    }
}

async fn execute_validated_fetch(
    validated: ValidatedFetchRequest,
    controller: AbortController,
) -> WebHostResult<BrowserFetchResponse> {
    let timeout = FetchTimeout::start(controller.clone(), validated.timeout_ms)?;
    let init = RequestInit::new();
    init.set_method(validated.request.method.as_str());
    match &validated.origin {
        BrowserFetchOrigin::SameOrigin => {
            init.set_mode(RequestMode::SameOrigin);
            init.set_credentials(RequestCredentials::SameOrigin);
        }
        BrowserFetchOrigin::Https { .. } => {
            init.set_mode(RequestMode::Cors);
            init.set_credentials(RequestCredentials::Omit);
        }
    }
    init.set_redirect(RequestRedirect::Manual);
    init.set_signal(Some(&controller.signal()));
    if !validated.request.body.is_empty() {
        let mut body = validated.request.body.clone();
        init.set_body_opt_u8_slice(Some(body.as_mut_slice()));
    }
    let request_url = validated
        .origin
        .request_url(&validated.request.path_and_query);
    let request = Request::new_with_str_and_init(&request_url, &init)
        .map_err(|error| js_error("construct same-origin fetch request", error))?;
    let headers = request.headers();
    for header in &validated.request.headers {
        headers
            .append(&header.name, &header.value)
            .map_err(|error| js_error("append fetch request header", error))?;
    }
    let response = JsFuture::from(window()?.fetch_with_request(&request))
        .await
        .map_err(|error| {
            if controller.signal().aborted() {
                WebHostError::platform("same-origin fetch", "request timed out or was cancelled")
            } else {
                js_error("same-origin fetch", error)
            }
        })?
        .dyn_into::<Response>()
        .map_err(|error| js_error("decode fetch Response", error))?;
    let response_headers = collect_response_headers(
        &response,
        validated.max_response_header_count,
        validated.max_response_header_bytes,
    )?;
    let body = read_bounded_response_body(&response, validated.max_response_bytes).await?;
    timeout.cancel();
    Ok(BrowserFetchResponse {
        request_id: validated.request.request_id,
        status: response.status(),
        headers: response_headers,
        body,
    })
}

struct FetchTimeout {
    window: web_sys::Window,
    timer_id: i32,
    _callback: Closure<dyn FnMut()>,
}

impl FetchTimeout {
    fn start(controller: AbortController, timeout_ms: u32) -> WebHostResult<Self> {
        let window = window()?;
        let callback = Closure::wrap(Box::new(move || controller.abort()) as Box<dyn FnMut()>);
        let timer_id = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                i32::try_from(timeout_ms).unwrap_or(i32::MAX),
            )
            .map_err(|error| js_error("schedule fetch timeout", error))?;
        Ok(Self {
            window,
            timer_id,
            _callback: callback,
        })
    }

    fn cancel(self) {
        self.window.clear_timeout_with_handle(self.timer_id);
    }
}

impl Drop for FetchTimeout {
    fn drop(&mut self) {
        self.window.clear_timeout_with_handle(self.timer_id);
    }
}

fn collect_response_headers(
    response: &Response,
    max_count: usize,
    max_bytes: usize,
) -> WebHostResult<Vec<HeaderValue>> {
    let mut headers = Vec::new();
    let mut total_bytes = 0usize;
    let iterator = response.headers().entries();
    for entry in iterator {
        let entry = entry.map_err(|error| js_error("iterate fetch response headers", error))?;
        let pair = Array::from(&entry);
        let name = pair.get(0).as_string().ok_or_else(|| {
            WebHostError::platform("decode fetch response headers", "header name is not text")
        })?;
        let value = pair.get(1).as_string().ok_or_else(|| {
            WebHostError::platform("decode fetch response headers", "header value is not text")
        })?;
        if headers.len() >= max_count {
            return Err(WebHostError::LimitExceeded {
                resource: "fetch response header count".to_owned(),
                limit: max_count,
            });
        }
        total_bytes = total_bytes
            .checked_add(name.len() + value.len())
            .ok_or_else(|| WebHostError::LimitExceeded {
                resource: "fetch response header bytes".to_owned(),
                limit: max_bytes,
            })?;
        if total_bytes > max_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "fetch response header bytes".to_owned(),
                limit: max_bytes,
            });
        }
        headers.push(HeaderValue { name, value });
    }
    headers.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.value.cmp(&right.value))
    });
    Ok(headers)
}

async fn read_bounded_response_body(
    response: &Response,
    max_bytes: usize,
) -> WebHostResult<Vec<u8>> {
    let Some(stream) = response.body() else {
        return Ok(Vec::new());
    };
    let reader = stream
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|error| js_error("acquire fetch body reader", error))?;
    let mut body = Vec::new();
    loop {
        let result = JsFuture::from(reader.read())
            .await
            .map_err(|error| js_error("read fetch response body", error))?;
        let done = Reflect::get(&result, &JsValue::from_str("done"))
            .map_err(|error| js_error("read fetch body completion", error))?
            .as_bool()
            .unwrap_or(false);
        if done {
            break;
        }
        let value = Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|error| js_error("read fetch body chunk", error))?;
        let chunk = Uint8Array::new(&value);
        let chunk_len = usize::try_from(chunk.length()).unwrap_or(usize::MAX);
        if body
            .len()
            .checked_add(chunk_len)
            .is_none_or(|total| total > max_bytes)
        {
            let _ = reader.cancel();
            return Err(WebHostError::LimitExceeded {
                resource: "fetch response body".to_owned(),
                limit: max_bytes,
            });
        }
        let start = body.len();
        body.resize(start + chunk_len, 0);
        chunk.copy_to(&mut body[start..]);
    }
    Ok(body)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserWebSocketEvent {
    Open {
        protocol: String,
    },
    Message {
        frame: SocketFrame,
    },
    Close {
        code: u16,
        reason: String,
        clean: bool,
    },
    Error {
        message: String,
    },
}

struct SocketEventQueue {
    events: VecDeque<BrowserWebSocketEvent>,
    bytes: usize,
    max_messages: usize,
    max_bytes: usize,
    overflowed: bool,
}

impl SocketEventQueue {
    fn push(&mut self, event: BrowserWebSocketEvent) -> bool {
        let bytes = socket_event_bytes(&event);
        if self.events.len() >= self.max_messages
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.max_bytes)
        {
            self.overflowed = true;
            return false;
        }
        self.bytes += bytes;
        self.events.push_back(event);
        true
    }

    fn drain(&mut self) -> Vec<BrowserWebSocketEvent> {
        self.bytes = 0;
        self.events.drain(..).collect()
    }
}

struct SocketListener {
    target: EventTarget,
    event: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            self.event,
            self.callback.as_ref().unchecked_ref(),
        );
    }
}

pub struct BrowserWebSocketAdapter {
    socket: WebSocket,
    queue: Rc<RefCell<SocketEventQueue>>,
    listeners: Vec<SocketListener>,
    max_message_bytes: usize,
    max_queue_messages: usize,
    max_queue_bytes: usize,
    sends_while_buffered: usize,
    _permit: HostResourcePermit,
}

pub struct BrowserWebSocketConnector {
    capabilities: BrowserWebSocketCapabilities,
    active: Rc<Cell<usize>>,
    max_connections: usize,
    event_wake: Rc<dyn Fn()>,
}

impl BrowserWebSocketConnector {
    pub fn new(
        capabilities: BrowserWebSocketCapabilities,
        max_connections: usize,
    ) -> WebHostResult<Self> {
        Self::new_with_event_wake(capabilities, max_connections, Rc::new(|| {}))
    }

    pub fn new_with_event_wake(
        capabilities: BrowserWebSocketCapabilities,
        max_connections: usize,
        event_wake: Rc<dyn Fn()>,
    ) -> WebHostResult<Self> {
        if max_connections == 0 {
            return Err(WebHostError::InvalidInput {
                field: "WebSocket max_connections".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }
        Ok(Self {
            capabilities,
            active: Rc::new(Cell::new(0)),
            max_connections,
            event_wake,
        })
    }

    pub fn connect(
        &self,
        request: BrowserWebSocketRequest,
    ) -> WebHostResult<BrowserWebSocketAdapter> {
        let validated = self.capabilities.validate_request(request)?;
        let permit = acquire_permit(
            &self.active,
            self.max_connections,
            "browser WebSocket connections",
        )?;
        BrowserWebSocketAdapter::connect_validated(validated, permit, Rc::clone(&self.event_wake))
    }

    pub fn active_connection_count(&self) -> usize {
        self.active.get()
    }
}

impl BrowserWebSocketAdapter {
    fn connect_validated(
        validated: ValidatedWebSocketRequest,
        permit: HostResourcePermit,
        event_wake: Rc<dyn Fn()>,
    ) -> WebHostResult<Self> {
        let location = window()?.location();
        let scheme = match location
            .protocol()
            .map_err(|error| js_error("read browser URL scheme", error))?
            .as_str()
        {
            "https:" => "wss:",
            "http:" => "ws:",
            _ => {
                return Err(WebHostError::unsupported(
                    "same-origin WebSocket",
                    "page URL must use HTTP or HTTPS",
                ));
            }
        };
        let host = location
            .host()
            .map_err(|error| js_error("read browser URL host", error))?;
        let url = format!("{scheme}//{host}{}", validated.request.path_and_query);
        let socket = if validated.request.protocols.is_empty() {
            WebSocket::new(&url)
        } else {
            let protocols = Array::new();
            for protocol in &validated.request.protocols {
                protocols.push(&JsValue::from_str(protocol));
            }
            WebSocket::new_with_str_sequence(&url, protocols.as_ref())
        }
        .map_err(|error| js_error("open same-origin WebSocket", error))?;
        socket.set_binary_type(BinaryType::Arraybuffer);
        let queue = Rc::new(RefCell::new(SocketEventQueue {
            events: VecDeque::new(),
            bytes: 0,
            max_messages: validated.max_queue_messages,
            max_bytes: validated.max_queue_bytes,
            overflowed: false,
        }));
        let target: EventTarget = socket.clone().into();
        let mut listeners = Vec::new();

        {
            let queue = Rc::clone(&queue);
            let socket_for_callback = socket.clone();
            let event_wake = Rc::clone(&event_wake);
            listeners.push(socket_listener(&target, "open", move |_event| {
                queue.borrow_mut().push(BrowserWebSocketEvent::Open {
                    protocol: socket_for_callback.protocol(),
                });
                event_wake();
            })?);
        }
        {
            let queue = Rc::clone(&queue);
            let socket_for_callback = socket.clone();
            let max_message_bytes = validated.max_message_bytes;
            let event_wake = Rc::clone(&event_wake);
            listeners.push(socket_listener(&target, "message", move |event| {
                let Some(message) = event.dyn_ref::<MessageEvent>() else {
                    return;
                };
                let data = message.data();
                let frame = if let Some(text) = data.as_string() {
                    SocketFrame::Text { text }
                } else if data.is_instance_of::<js_sys::ArrayBuffer>() {
                    SocketFrame::Binary {
                        bytes: Uint8Array::new(&data).to_vec(),
                    }
                } else {
                    queue.borrow_mut().push(BrowserWebSocketEvent::Error {
                        message: "unsupported WebSocket frame value".to_owned(),
                    });
                    event_wake();
                    let _ = socket_for_callback
                        .close_with_code_and_reason(1003, "unsupported frame value");
                    return;
                };
                if frame.byte_len() > max_message_bytes {
                    queue.borrow_mut().push(BrowserWebSocketEvent::Error {
                        message: "WebSocket message exceeded the declared byte limit".to_owned(),
                    });
                    event_wake();
                    let _ =
                        socket_for_callback.close_with_code_and_reason(1009, "message too large");
                    return;
                }
                let admitted = queue
                    .borrow_mut()
                    .push(BrowserWebSocketEvent::Message { frame });
                event_wake();
                if !admitted {
                    let _ =
                        socket_for_callback.close_with_code_and_reason(1009, "receive queue full");
                }
            })?);
        }
        {
            let queue = Rc::clone(&queue);
            let event_wake = Rc::clone(&event_wake);
            listeners.push(socket_listener(&target, "close", move |event| {
                let Some(close) = event.dyn_ref::<CloseEvent>() else {
                    return;
                };
                queue.borrow_mut().push(BrowserWebSocketEvent::Close {
                    code: close.code(),
                    reason: close.reason(),
                    clean: close.was_clean(),
                });
                event_wake();
            })?);
        }
        {
            let queue = Rc::clone(&queue);
            let event_wake = Rc::clone(&event_wake);
            listeners.push(socket_listener(&target, "error", move |event| {
                queue.borrow_mut().push(BrowserWebSocketEvent::Error {
                    message: js_message(event.as_ref()),
                });
                event_wake();
            })?);
        }

        Ok(Self {
            socket,
            queue,
            listeners,
            max_message_bytes: validated.max_message_bytes,
            max_queue_messages: validated.max_queue_messages,
            max_queue_bytes: validated.max_queue_bytes,
            sends_while_buffered: 0,
            _permit: permit,
        })
    }

    pub fn send(&mut self, frame: &SocketFrame) -> WebHostResult<()> {
        let bytes = frame.byte_len();
        if bytes > self.max_message_bytes {
            return Err(WebHostError::LimitExceeded {
                resource: "WebSocket outbound message".to_owned(),
                limit: self.max_message_bytes,
            });
        }
        if self.socket.ready_state() != WebSocket::OPEN {
            return Err(WebHostError::platform(
                "send WebSocket message",
                "socket is not open",
            ));
        }
        let buffered = usize::try_from(self.socket.buffered_amount()).unwrap_or(usize::MAX);
        if buffered == 0 {
            self.sends_while_buffered = 0;
        }
        if self.sends_while_buffered >= self.max_queue_messages
            || buffered
                .checked_add(bytes)
                .is_none_or(|total| total > self.max_queue_bytes)
        {
            return Err(WebHostError::QueueOverflow {
                queue: "WebSocket outbound messages".to_owned(),
                capacity: self.max_queue_messages,
            });
        }
        match frame {
            SocketFrame::Text { text } => self.socket.send_with_str(text),
            SocketFrame::Binary { bytes } => self.socket.send_with_u8_array(bytes),
        }
        .map_err(|error| js_error("send WebSocket message", error))?;
        self.sends_while_buffered = self.sends_while_buffered.saturating_add(1);
        Ok(())
    }

    pub fn take_events(&self) -> Vec<BrowserWebSocketEvent> {
        self.queue.borrow_mut().drain()
    }

    pub fn overflowed(&self) -> bool {
        self.queue.borrow().overflowed
    }

    pub fn buffered_amount(&self) -> u32 {
        self.socket.buffered_amount()
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    pub fn close(&self, code: u16, reason: &str) -> WebHostResult<()> {
        if code != 1000 && !(3000..=4999).contains(&code) {
            return Err(WebHostError::InvalidInput {
                field: "WebSocket close code".to_owned(),
                reason: "browser callers may use 1000 or an application code in 3000..=4999"
                    .to_owned(),
            });
        }
        if reason.len() > 123 {
            return Err(WebHostError::LimitExceeded {
                resource: "WebSocket close reason".to_owned(),
                limit: 123,
            });
        }
        self.socket
            .close_with_code_and_reason(code, reason)
            .map_err(|error| js_error("close WebSocket", error))
    }
}

impl Drop for BrowserWebSocketAdapter {
    fn drop(&mut self) {
        let _ = self.socket.close_with_code_and_reason(1000, "host dropped");
    }
}

fn socket_listener(
    target: &EventTarget,
    event: &'static str,
    mut callback: impl FnMut(&Event) + 'static,
) -> WebHostResult<SocketListener> {
    let closure =
        Closure::wrap(Box::new(move |event: Event| callback(&event)) as Box<dyn FnMut(Event)>);
    target
        .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
        .map_err(|error| js_error("install WebSocket listener", error))?;
    Ok(SocketListener {
        target: target.clone(),
        event,
        callback: closure,
    })
}

fn socket_event_bytes(event: &BrowserWebSocketEvent) -> usize {
    match event {
        BrowserWebSocketEvent::Open { protocol } => protocol.len(),
        BrowserWebSocketEvent::Message { frame } => frame.byte_len(),
        BrowserWebSocketEvent::Close { reason, .. }
        | BrowserWebSocketEvent::Error { message: reason } => reason.len(),
    }
}
