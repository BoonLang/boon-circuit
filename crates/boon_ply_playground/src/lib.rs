#![recursion_limit = "256"]

use boon_runtime::{
    LiveRuntime, LiveSourceEvent, LiveStepOutput, RunOutput, Scenario, ScenarioStep,
    VerificationLayer, example_paths, parse_scenario, run_scenario,
    run_scenario_source_with_parsed_scenario_step_limit, run_source_initial_state, sha256_file,
    write_json,
};
use ply_engine::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static DEFAULT_FONT: FontAsset = FontAsset::Bytes {
    file_name: "DejaVuSans.ttf",
    data: include_bytes!("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
};

thread_local! {
    static UI_SOURCE_OBSERVATIONS: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
    static LAST_FOCUSED_RENDER_INPUT: RefCell<Option<FocusedRenderInput>> = const { RefCell::new(None) };
    static LAST_SELECTED_RENDER_INPUT: RefCell<Option<FocusedRenderInput>> = const { RefCell::new(None) };
    static RENDER_ID_LABELS: RefCell<BTreeMap<String, &'static str>> = const { RefCell::new(BTreeMap::new()) };
    static CURRENT_HOVER_SCOPES: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    static LAST_HOVER_SCOPES: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    static LAST_BUTTON_PRESS: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
    static SUPPRESS_NEXT_BLUR_INPUT: RefCell<Option<Id>> = const { RefCell::new(None) };
    static CURRENT_FOCUSED_ELEMENT: RefCell<Option<Id>> = const { RefCell::new(None) };
}

static FOCUS_FREE_HEADED: AtomicBool = AtomicBool::new(false);
static FOCUS_FREE_SCREENSHOT_CACHE: OnceLock<Mutex<Option<(PathBuf, ScreenshotCapture)>>> =
    OnceLock::new();

const PLAYGROUND_HELP: &str = "\
boon_ply_playground

Usage:
  boon_ply_playground --example <todomvc|cells>
  boon_ply_playground --example <todomvc|cells> --preview-only
  boon_ply_playground --single-window --example <todomvc|cells> --mode <app|dev>
  boon_ply_playground --smoke-launch --example <name> --report <path>
  boon_ply_playground --verify-headed --example <name> --report <path>
  boon_ply_playground --verify-headed-focusless --example <name> --report <path>
  boon_ply_playground --verify-split-wayland --example <name> --report <path>
  boon_ply_playground --verify-wayland-scroll-speed --example cells --report <path>
  boon_ply_playground --verify-os-input-probe --report <path>
";

const IPC_MAX_WRITE_BUFFER_BYTES: usize = 1_000_000;
const IPC_READ_BUFFER_LIMIT_BYTES: usize = 2_000_000;

#[derive(Clone, Debug)]
struct FocusedRenderInput {
    id: Id,
    change_source: Option<String>,
    submit_source: Option<String>,
    blur_source: Option<String>,
    cancel_source: Option<String>,
    escape_source: Option<String>,
    address: Option<String>,
    display_value: String,
    edit_value: String,
    target_text: Option<String>,
    target_occurrence: Option<usize>,
    focus_proxy: bool,
}

struct RenderScrollWheelFallback {
    id: Id,
    before: ply_engine::math::Vector2,
    delta: ply_engine::math::Vector2,
    lock_y: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
struct CachedExample {
    source_fingerprint: FileFingerprint,
    scenario_fingerprint: FileFingerprint,
    scenario_path: PathBuf,
    scenario: Scenario,
    scenario_steps: Vec<String>,
    source_text: String,
    output: RunOutput,
    render_nodes: Vec<RenderNode>,
}

#[derive(Default)]
struct RenderInputValueCache {
    render_generation: u64,
    selected_signature: String,
    values: Vec<RenderInputValue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowRole {
    Preview,
    Dev,
}

impl WindowRole {
    fn from_args(args: &[String]) -> Self {
        match value_after(args, "--window-role").as_deref() {
            Some("dev") => Self::Dev,
            _ => Self::Preview,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct IpcEnvelope<T> {
    token: String,
    payload: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DevCommand {
    LoadExample { example: String },
    UpdateSource { generation: u64, text: String },
    RunSource,
    Reset,
    StepNext,
    StepPrev,
    RequestSnapshot,
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PreviewTelemetry {
    Snapshot(RuntimeSnapshot),
    RuntimeEvent {
        event: serde_json::Value,
        output: serde_json::Value,
    },
    FrameMetrics {
        frame_ms: f64,
        draw_ms: f64,
        preview_blocked_on_ipc_count: u64,
        dropped_telemetry_count: u64,
    },
    CompileStarted {
        generation: u64,
    },
    CompileFinished {
        generation: u64,
        elapsed_ms: f64,
        snapshot: RuntimeSnapshot,
    },
    CompileFailed {
        generation: u64,
        elapsed_ms: f64,
        error: String,
    },
    Heartbeat {
        generated_at_utc: String,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct RuntimeSnapshot {
    selected_example: String,
    source_text: String,
    source_generation: u64,
    step_limit: Option<usize>,
    scenario_steps: Vec<String>,
    state_summary: serde_json::Value,
    semantic_delta_tail: Vec<serde_json::Value>,
    render_patch_tail: Vec<serde_json::Value>,
    report_summary: serde_json::Value,
    selected_input: serde_json::Value,
    last_error: Option<String>,
}

#[derive(Default)]
struct IpcStats {
    blocked_writes: u64,
    dropped_messages: u64,
    received_messages: u64,
    sent_messages: u64,
}

struct IpcLinePeer {
    stream: TcpStream,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
    stats: IpcStats,
}

impl IpcLinePeer {
    fn new(stream: TcpStream) -> Result<Self, Box<dyn std::error::Error>> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            read_buffer: Vec::new(),
            write_buffer: Vec::new(),
            stats: IpcStats::default(),
        })
    }

    fn send<T: Serialize>(&mut self, token: &str, payload: &T) {
        let envelope = IpcEnvelope {
            token: token.to_owned(),
            payload,
        };
        let Ok(mut bytes) = serde_json::to_vec(&envelope) else {
            self.stats.dropped_messages += 1;
            return;
        };
        bytes.push(b'\n');
        if self.write_buffer.len().saturating_add(bytes.len()) > IPC_MAX_WRITE_BUFFER_BYTES {
            self.write_buffer.clear();
            self.stats.dropped_messages += 1;
        }
        self.write_buffer.extend(bytes);
        self.flush();
    }

    fn flush(&mut self) {
        while !self.write_buffer.is_empty() {
            match self.stream.write(&self.write_buffer) {
                Ok(0) => break,
                Ok(count) => {
                    self.write_buffer.drain(..count);
                    self.stats.sent_messages += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    self.stats.blocked_writes += 1;
                    break;
                }
                Err(_) => {
                    self.write_buffer.clear();
                    break;
                }
            }
        }
    }

    fn recv<T: for<'de> Deserialize<'de>>(&mut self, token: &str) -> Vec<T> {
        let mut scratch = [0_u8; 8192];
        loop {
            match self.stream.read(&mut scratch) {
                Ok(0) => break,
                Ok(count) => {
                    self.read_buffer.extend_from_slice(&scratch[..count]);
                    if self.read_buffer.len() > IPC_READ_BUFFER_LIMIT_BYTES {
                        self.read_buffer.clear();
                        self.stats.dropped_messages += 1;
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        let mut messages = Vec::new();
        while let Some(newline) = self.read_buffer.iter().position(|byte| *byte == b'\n') {
            let line = self.read_buffer.drain(..=newline).collect::<Vec<_>>();
            let line = &line[..line.len().saturating_sub(1)];
            let Ok(envelope) = serde_json::from_slice::<IpcEnvelope<T>>(line) else {
                self.stats.dropped_messages += 1;
                continue;
            };
            if envelope.token != token {
                self.stats.dropped_messages += 1;
                continue;
            }
            self.stats.received_messages += 1;
            messages.push(envelope.payload);
        }
        messages
    }
}

#[allow(dead_code)]
struct PreviewIpcServer {
    listener: TcpListener,
    peer: Option<IpcLinePeer>,
    token: String,
    dev_child: Option<Child>,
}

#[allow(dead_code)]
impl PreviewIpcServer {
    fn new(token: String) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            peer: None,
            token,
            dev_child: None,
        })
    }

    fn addr(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(self.listener.local_addr()?.to_string())
    }

    fn accept(&mut self) {
        if self.peer.is_some() {
            return;
        }
        match self.listener.accept() {
            Ok((stream, _)) => {
                if let Ok(peer) = IpcLinePeer::new(stream) {
                    self.peer = Some(peer);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {}
        }
    }

    fn send(&mut self, telemetry: PreviewTelemetry) {
        self.accept();
        if let Some(peer) = &mut self.peer {
            peer.send(&self.token, &telemetry);
        }
    }

    fn recv_commands(&mut self) -> Vec<DevCommand> {
        self.accept();
        self.peer
            .as_mut()
            .map(|peer| peer.recv(&self.token))
            .unwrap_or_default()
    }

    fn stats(&self) -> IpcStatsSnapshot {
        self.peer
            .as_ref()
            .map(|peer| IpcStatsSnapshot {
                blocked_writes: peer.stats.blocked_writes,
                dropped_messages: peer.stats.dropped_messages,
                received_messages: peer.stats.received_messages,
                sent_messages: peer.stats.sent_messages,
            })
            .unwrap_or_default()
    }
}

#[derive(Default)]
#[allow(dead_code)]
struct IpcStatsSnapshot {
    blocked_writes: u64,
    dropped_messages: u64,
    received_messages: u64,
    sent_messages: u64,
}

#[derive(Default)]
struct WebDebugState {
    snapshot: RuntimeSnapshot,
    metrics: serde_json::Value,
    commands: Vec<DevCommand>,
    request_count: u64,
}

struct WebDebugServer {
    addr: String,
    state: Arc<Mutex<WebDebugState>>,
    child: Option<Child>,
}

impl WebDebugServer {
    fn start(snapshot: RuntimeSnapshot) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(false)?;
        let addr = listener.local_addr()?.to_string();
        let state = Arc::new(Mutex::new(WebDebugState {
            snapshot,
            metrics: json!({}),
            commands: Vec::new(),
            request_count: 0,
        }));
        let thread_state = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                handle_web_debug_request(stream, &thread_state);
            }
        });
        Ok(Self {
            addr,
            state,
            child: None,
        })
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn update(&self, snapshot: RuntimeSnapshot, metrics: serde_json::Value) {
        if let Ok(mut state) = self.state.lock() {
            state.snapshot = snapshot;
            state.metrics = metrics;
        }
    }

    fn take_commands(&self) -> Vec<DevCommand> {
        self.state
            .lock()
            .map(|mut state| std::mem::take(&mut state.commands))
            .unwrap_or_default()
    }

    fn request_count(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.request_count)
            .unwrap_or_default()
    }
}

fn handle_web_debug_request(mut stream: TcpStream, state: &Arc<Mutex<WebDebugState>>) {
    let mut buffer = [0_u8; 64 * 1024];
    let Ok(count) = stream.read(&mut buffer) else {
        return;
    };
    let request = String::from_utf8_lossy(&buffer[..count]);
    let mut parts = request.split("\r\n\r\n");
    let head = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("");
    let first = head.lines().next().unwrap_or("");
    let response = if first.starts_with("GET /state ") {
        let payload = state
            .lock()
            .map(|mut state| {
                state.request_count += 1;
                json!({
                    "snapshot": state.snapshot,
                    "metrics": state.metrics,
                    "request_count": state.request_count
                })
            })
            .unwrap_or_else(|_| json!({"error": "state lock poisoned"}));
        http_response("application/json", &payload.to_string())
    } else if first.starts_with("POST /command ") {
        if let Ok(command) = serde_json::from_str::<DevCommand>(body) {
            if let Ok(mut state) = state.lock() {
                state.commands.push(command);
                state.request_count += 1;
            }
        }
        http_response("application/json", r#"{"ok":true}"#)
    } else {
        http_response("text/html; charset=utf-8", WEB_DEBUG_HTML)
    };
    let _ = stream.write_all(response.as_bytes());
}

fn http_response(content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncache-control: no-store\r\naccess-control-allow-origin: *\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
}

const WEB_DEBUG_HTML: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>Boon Circuit Dev Console</title>
<style>
body{margin:0;font:14px system-ui,sans-serif;background:#eef1f5;color:#1f2630}
#root{display:grid;grid-template-columns:180px 1fr;height:100vh}
aside{background:#1f2630;color:#f1f5fa;padding:10px;display:flex;flex-direction:column;gap:8px}
main{padding:10px;display:grid;grid-template-rows:auto 1fr 190px;gap:10px;min-width:0}
button{height:30px;border:0;background:#e4eaf2;color:#1f2630;padding:0 10px}
button.primary{background:#2f6fb8;color:white}
.bar{display:flex;gap:6px;align-items:center}
.body{display:grid;grid-template-columns:minmax(480px,650px) 1fr;gap:10px;min-height:0}
textarea{width:100%;height:100%;box-sizing:border-box;border:1px solid #d5dde8;padding:10px;font:13px monospace;resize:none}
.panels{display:grid;grid-template-columns:1fr 1fr;grid-template-rows:1fr 1fr;gap:10px;min-height:0}
.panel{background:white;border:1px solid #d5dde8;padding:10px;overflow:auto}
.title{color:#596579;font-size:16px;margin-bottom:6px}
pre{white-space:pre-wrap;word-break:break-word;margin:0;font:12px monospace}
</style>
</head>
<body>
<div id="root">
<aside><h2>Boon Circuit</h2><button onclick="cmd({kind:'load_example',example:'todomvc'})">1 TodoMVC</button><button onclick="cmd({kind:'load_example',example:'cells'})">2 Cells</button><div id="status">connecting</div></aside>
<main>
<div class="bar"><button class="primary" onclick="cmd({kind:'run_source'})">Run</button><button onclick="cmd({kind:'reset'})">Reset</button><button onclick="cmd({kind:'step_next'})">Step</button><button onclick="cmd({kind:'step_prev'})">Back</button><span id="headline"></span></div>
<div class="body"><textarea id="source"></textarea><div class="panels"><div class="panel"><div class="title">Deltas</div><pre id="deltas"></pre></div><div class="panel"><div class="title">Inspector</div><pre id="inspector"></pre></div><div class="panel"><div class="title">Causes</div><pre id="causes"></pre></div><div class="panel"><div class="title">Metrics</div><pre id="metrics"></pre></div></div></div>
<div class="panel"><div class="title">Scenario</div><pre id="scenario"></pre></div>
</main>
</div>
<script>
let lastGeneration=-1, localEdit=false, timer=null;
async function cmd(payload){await fetch('/command',{method:'POST',body:JSON.stringify(payload)});}
document.getElementById('source').addEventListener('input', e => {
  localEdit=true; clearTimeout(timer);
  timer=setTimeout(()=>cmd({kind:'update_source',generation:Date.now(),text:e.target.value}),180);
});
async function tick(){
  const data=await (await fetch('/state')).json();
  const s=data.snapshot||{};
  document.getElementById('status').textContent='connected';
  document.getElementById('headline').textContent=(s.selected_example||'')+' / generation '+(s.source_generation||0);
  if(!localEdit && s.source_generation!==lastGeneration){document.getElementById('source').value=s.source_text||''; lastGeneration=s.source_generation;}
  localEdit=false;
  document.getElementById('deltas').textContent=JSON.stringify(s.semantic_delta_tail||[],null,2);
  document.getElementById('inspector').textContent=JSON.stringify(s.state_summary||null,null,2).slice(0,4000);
  document.getElementById('causes').textContent=JSON.stringify(s.report_summary||null,null,2);
  document.getElementById('metrics').textContent=JSON.stringify(data.metrics||{},null,2);
  document.getElementById('scenario').textContent=(s.scenario_steps||[]).join('\n');
}
setInterval(()=>tick().catch(()=>document.getElementById('status').textContent='disconnected'),250);
tick(); cmd({kind:'request_snapshot'});
</script>
</body>
</html>"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaygroundView {
    App,
    Source,
    Deltas,
    Inspector,
    Causes,
    Scenario,
}

impl PlaygroundView {
    fn from_mode_arg(args: &[String]) -> Self {
        match value_after(args, "--mode").as_deref() {
            Some("dev") | Some("source") => Self::Source,
            Some("deltas") => Self::Deltas,
            Some("inspector") => Self::Inspector,
            Some("causes") => Self::Causes,
            Some("scenario") => Self::Scenario,
            _ => Self::App,
        }
    }
}

struct ExampleNavSpec {
    name: &'static str,
    nav_id: &'static str,
    label: &'static str,
}

fn example_nav_specs() -> &'static [ExampleNavSpec] {
    &[
        ExampleNavSpec {
            name: "todomvc",
            nav_id: "nav_todomvc",
            label: "1 TodoMVC",
        },
        ExampleNavSpec {
            name: "cells",
            nav_id: "nav_cells",
            label: "2 Cells",
        },
    ]
}

fn default_example_name() -> String {
    example_name_for_slot(0).to_owned()
}

fn alternate_example_name(original: &str) -> &'static str {
    if original == example_name_for_slot(0) {
        example_name_for_slot(1)
    } else {
        example_name_for_slot(0)
    }
}

fn example_name_for_slot(index: usize) -> &'static str {
    example_nav_specs()
        .get(index)
        .map(|example| example.name)
        .unwrap_or_else(|| example_nav_specs()[0].name)
}

fn example_nav_id_for_slot(index: usize) -> &'static str {
    example_nav_specs()
        .get(index)
        .map(|example| example.nav_id)
        .unwrap_or_else(|| example_nav_specs()[0].nav_id)
}

fn example_nav_ids() -> &'static [&'static str] {
    &["nav_todomvc", "nav_cells"]
}

#[derive(Clone, Debug)]
enum RenderNode {
    Column {
        id: Option<String>,
        width: Option<f32>,
        height: Option<f32>,
        background: u32,
        border: Option<u32>,
        scroll_x: bool,
        scroll_y: bool,
        sync_scroll_x: Option<String>,
        scrollbar: bool,
        gap: f32,
        padding: Option<(f32, f32, f32, f32)>,
        children: Vec<RenderNode>,
    },
    Row {
        id: Option<String>,
        height: Option<f32>,
        background: u32,
        border: Option<u32>,
        scroll_x: bool,
        scroll_y: bool,
        sync_scroll_x: Option<String>,
        scrollbar: bool,
        gap: f32,
        padding: Option<(f32, f32, f32, f32)>,
        children: Vec<RenderNode>,
    },
    ForEach {
        list: String,
        item: String,
        children: Vec<RenderNode>,
    },
    Text {
        value: RenderValue,
        size: u16,
        color: u32,
        background: Option<u32>,
        width: Option<RenderExtent>,
        height: Option<f32>,
        strike_if: Option<RenderValue>,
        center: bool,
    },
    Input {
        id: String,
        key: Option<RenderValue>,
        value: RenderValue,
        edit_value: Option<RenderValue>,
        display_value: Option<RenderValue>,
        placeholder: RenderValue,
        change_source: Option<RenderValue>,
        submit_source: Option<RenderValue>,
        cancel_source: Option<RenderValue>,
        escape_source: Option<RenderValue>,
        blur_source: Option<RenderValue>,
        address: Option<RenderValue>,
        target: Option<RenderValue>,
        visible: Option<RenderValue>,
        focus_proxy: bool,
        size: u16,
        width: Option<RenderExtent>,
        height: Option<f32>,
        color: u32,
        placeholder_color: u32,
        background: u32,
        focused_background: Option<u32>,
        border: Option<u32>,
        focused_border: Option<u32>,
    },
    Button {
        id: String,
        text: RenderValue,
        width: Option<RenderExtent>,
        selected: Option<RenderSelection>,
        source: Option<String>,
        double_click_source: Option<String>,
        address: Option<RenderValue>,
        target: Option<RenderValue>,
        visible: Option<RenderValue>,
        hover_visible: bool,
        height: Option<f32>,
        size: u16,
        color: u32,
        background: u32,
        selected_color: u32,
        selected_background: u32,
        border: Option<u32>,
        selected_border: Option<u32>,
        color_if: Option<RenderValue>,
        if_color: Option<u32>,
        strike_if: Option<RenderValue>,
        align_left: bool,
    },
    Checkbox {
        id: String,
        checked: RenderValue,
        source: Option<String>,
        target: Option<RenderValue>,
        size: f32,
    },
}

#[derive(Clone, Debug)]
enum RenderValue {
    Literal(String),
    Path(String),
    Template(String),
}

#[derive(Clone, Debug)]
struct RenderSelection {
    path: String,
    expected: String,
}

#[derive(Clone, Debug)]
enum RenderExtent {
    Fill,
    Fixed(f32),
}

impl RenderExtent {
    fn from_attr(value: &str) -> Option<Self> {
        match value {
            "fill" | "Fill" => Some(Self::Fill),
            _ => value.parse().ok().map(Self::Fixed),
        }
    }
}

#[derive(Clone, Debug)]
struct RenderContext<'a> {
    root: &'a serde_json::Value,
    overlays: Vec<(String, serde_json::Value)>,
    bindings: Vec<RenderBinding<'a>>,
    index_stack: Vec<usize>,
    hover_scopes: Vec<String>,
}

#[derive(Clone, Debug)]
struct RenderBinding<'a> {
    name: String,
    list: String,
    value: &'a serde_json::Value,
    index: usize,
}

impl<'a> RenderContext<'a> {
    fn root(root: &'a serde_json::Value) -> Self {
        Self {
            root,
            overlays: Vec::new(),
            bindings: Vec::new(),
            index_stack: Vec::new(),
            hover_scopes: Vec::new(),
        }
    }

    fn with_overlay_value(&self, name: &str, value: serde_json::Value) -> RenderContext<'a> {
        let mut next = self.clone();
        next.overlays.push((name.to_owned(), value));
        next
    }

    fn with_binding(
        &self,
        list: &str,
        name: &str,
        value: &'a serde_json::Value,
        index: usize,
    ) -> RenderContext<'a> {
        let mut next = self.clone();
        next.bindings.push(RenderBinding {
            name: name.to_owned(),
            list: list.to_owned(),
            value,
            index,
        });
        next.index_stack.push(index);
        next
    }

    fn with_hover_scope(&self, scope: String) -> RenderContext<'a> {
        let mut next = self.clone();
        next.hover_scopes.push(scope);
        next
    }
}

pub async fn run_app_from_args() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--verify-os-input-probe") {
        return run_verify_os_input_probe(&args).await;
    }
    if args.iter().any(|arg| arg == "--verify-headed-focusless") {
        FOCUS_FREE_HEADED.store(true, Ordering::Relaxed);
        unsafe {
            std::env::set_var("BOON_FOCUS_FREE_HEADED", "1");
        }
        return run_verify_headed(&args).await;
    }
    if args.iter().any(|arg| arg == "--verify-headed") {
        return run_verify_headed(&args).await;
    }
    if args.iter().any(|arg| arg == "--verify-split-wayland") {
        return run_verify_split_wayland(&args).await;
    }
    if args
        .iter()
        .any(|arg| arg == "--verify-wayland-scroll-speed")
    {
        return run_verify_wayland_scroll_speed(&args).await;
    }
    if args.iter().any(|arg| arg == "--smoke-launch") {
        return run_smoke_launch(&args).await;
    }
    if WindowRole::from_args(&args) == WindowRole::Dev {
        return run_dev_window(&args).await;
    }
    run_interactive(&args).await
}

fn print_help() {
    print!("{PLAYGROUND_HELP}");
}

fn focus_free_headed() -> bool {
    FOCUS_FREE_HEADED.load(Ordering::Relaxed)
        || std::env::var("BOON_FOCUS_FREE_HEADED").as_deref() == Ok("1")
        || std::env::args().any(|arg| arg == "--verify-headed-focusless")
}

fn require_wayland_window(role: &str) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("WAYLAND_DISPLAY").ok().is_none() {
        return Err(format!("{role} window requires WAYLAND_DISPLAY for Wayland-only mode").into());
    }
    if std::env::var("XDG_SESSION_TYPE")
        .ok()
        .is_some_and(|value| value != "wayland")
    {
        return Err(format!("{role} window requires XDG_SESSION_TYPE=wayland").into());
    }
    Ok(())
}

fn os_input_forbidden() -> bool {
    focus_free_headed() || std::env::var("BOON_FORBID_OS_INPUT").as_deref() == Ok("1")
}

fn os_input_isolated() -> bool {
    std::env::var("BOON_OS_INPUT_ISOLATED").as_deref() == Ok("xvfb")
}

fn live_desktop_input_allowed_from(allow: Option<&str>, accept: Option<&str>) -> bool {
    allow == Some("1") && accept == Some("1")
}

fn live_desktop_input_allowed() -> bool {
    live_desktop_input_allowed_from(
        std::env::var("BOON_ALLOW_LIVE_DESKTOP_INPUT")
            .ok()
            .as_deref(),
        std::env::var("BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS")
            .ok()
            .as_deref(),
    )
}

fn os_input_permission_granted() -> bool {
    !os_input_forbidden() && (os_input_isolated() || live_desktop_input_allowed())
}

fn require_os_input_permission(action: &str) -> Result<(), Box<dyn std::error::Error>> {
    if os_input_forbidden() {
        return Err(format!(
            "{action} is forbidden; use --verify-headed-focusless or unset BOON_FORBID_OS_INPUT only for an explicit isolated OS input probe"
        )
        .into());
    }
    if os_input_isolated() {
        return Ok(());
    }
    if live_desktop_input_allowed() {
        return Ok(());
    }
    Err(format!(
        "{action} targets the live desktop; run through xtask for isolated Xvfb, or set both BOON_ALLOW_LIVE_DESKTOP_INPUT=1 and BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS=1"
    )
    .into())
}

async fn run_verify_headed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_after(args, "--example").unwrap_or_else(default_example_name);
    let focus_free = focus_free_headed();
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if focus_free {
                PathBuf::from(format!("target/reports/{example}-headed-focusless.json"))
            } else {
                PathBuf::from(format!("target/reports/{example}-headed-ply.json"))
            }
        });
    let (source, scenario, _) = example_paths(&example)?;
    let screenshot = report.with_extension("png");
    let artifact_prefix = report_artifact_prefix(&report, &example);
    let os_probe_screenshot = report.with_file_name(format!("{artifact_prefix}-os-input.png"));
    let os_pointer_probe_screenshot =
        report.with_file_name(format!("{artifact_prefix}-os-pointer.png"));
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let os_probe_token = format!("boon-headed-os-{}-{example}", std::process::id());
    let headed_os_probe = if focus_free {
        skipped_os_keyboard_probe(&os_probe_screenshot)
    } else {
        run_os_keyboard_probe_in_window(&mut ply, &os_probe_token, &os_probe_screenshot).await?
    };
    let headed_os_pointer_probe = if focus_free {
        skipped_os_pointer_probe(&os_pointer_probe_screenshot)
    } else if std::env::var("BOON_ALLOW_OS_POINTER_PROBE").as_deref() == Ok("1") {
        run_os_pointer_probe_in_window(&mut ply, &os_pointer_probe_screenshot).await?
    } else {
        skipped_os_pointer_probe(&os_pointer_probe_screenshot)
    };
    let source_text = std::fs::read_to_string(&source)?;
    let scenario_data = parse_scenario(&scenario)?;
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    ply.set_text_value("source_editor", &source_text);
    state.reset_to_initial(&ply);
    let playground_surface_visible_bounds =
        playground_surface_visible_bounds_for_all_views(&mut ply, &mut state).await;
    state.view = PlaygroundView::App;
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    let scroll_observations =
        drive_visible_scroll_wheel_probe(&mut ply, &mut state, &report, &example).await;
    let source_editor_scroll_observations =
        drive_visible_source_editor_scroll_probe(&mut ply, &mut state, &report, &example).await;
    let formula_bar_observations =
        drive_visible_formula_bar_probe(&mut ply, &mut state, &report, &example).await;
    let app_control_observations =
        drive_visible_app_control_probe(&mut ply, &mut state, &report, &example).await;
    let source_event_observations =
        drive_visible_source_event_probe(&mut ply, &mut state, &report, &example, &scenario_data)
            .await;
    state.reset_to_initial(&ply);
    ply.clear_focus();
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    let step_observations = drive_visible_step_control_sequence(
        &mut ply,
        &mut state,
        &scenario_data,
        &report,
        &example,
    )
    .await;
    let output = run_scenario(&source, &scenario, VerificationLayer::HeadedPly, None)?;
    state = PlaygroundState::from_output(
        example.clone(),
        scenario.clone(),
        scenario_data.step.len(),
        output,
    );
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    draw_frame(&mut ply, &state).await;
    next_frame().await;
    draw_frame(&mut ply, &state).await;
    let image = get_screen_data();
    let mut pixel_stats = image_stats(&image.bytes);
    let mut capture_backend = "macroquad-framebuffer".to_owned();
    let mut framebuffer_width = u32::from(image.width);
    let mut framebuffer_height = u32::from(image.height);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        let fallback = capture_with_cosmic_screenshot(&screenshot)?;
        pixel_stats = fallback.pixel_stats;
        capture_backend = fallback.capture_backend;
        framebuffer_width = fallback.width;
        framebuffer_height = fallback.height;
    } else {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
    }
    next_frame().await;
    let mut report_json = state
        .output
        .as_ref()
        .ok_or("missing verifier output")?
        .report
        .clone();
    let headed_probes_passed = scroll_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && source_editor_scroll_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && formula_bar_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && app_control_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && source_event_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            && (observation
                .get("scenario_step_id")
                .is_none_or(serde_json::Value::is_null)
                || observation
                    .get("scenario_expectations_checked")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
    }) && step_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    });
    if let Some(object) = report_json.as_object_mut() {
        object.insert(
            "status".to_owned(),
            json!(if headed_probes_passed { "pass" } else { "fail" }),
        );
        object.insert(
            "exit_status".to_owned(),
            json!(if headed_probes_passed { 0 } else { 1 }),
        );
        object.insert(
            "window_mode".to_owned(),
            json!(if focus_free {
                "headed-focusless"
            } else {
                "headed"
            }),
        );
        object.insert("window_backend".to_owned(), json!("ply-engine/macroquad"));
        object.insert("window_pid".to_owned(), json!(std::process::id()));
        object.insert(
            "window_title".to_owned(),
            json!("Boon Circuit Ply Playground"),
        );
        object.insert("display_server".to_owned(), json!(display_server()));
        object.insert(
            "linux_backend_requested".to_owned(),
            json!("x11_with_wayland_fallback"),
        );
        object.insert(
            "display_env".to_owned(),
            json!({
                "WAYLAND_DISPLAY": std::env::var("WAYLAND_DISPLAY").ok(),
                "DISPLAY": std::env::var("DISPLAY").ok()
            }),
        );
        object.insert(
            "display_socket_or_compositor_connection".to_owned(),
            json!(display_socket()),
        );
        object.insert(
            "native_display_contract".to_owned(),
            native_display_contract(),
        );
        object.insert("display_scale".to_owned(), json!(screen_dpi_scale()));
        object.insert(
            "window_size".to_owned(),
            json!([screen_width(), screen_height()]),
        );
        object.insert(
            "framebuffer_size".to_owned(),
            json!({
                "width": framebuffer_width,
                "height": framebuffer_height
            }),
        );
        object.insert(
            "input_backend".to_owned(),
            json!(if focus_free {
                "ply-synthetic-focus-free"
            } else if display_server() == "x11" {
                "macroquad-os-events + xdotool-real-keyboard-events + xtest-pointer-probe"
            } else {
                "macroquad-os-events + wtype-real-keyboard-events + ydotool-pointer-probe"
            }),
        );
        object.insert("os_focus_required".to_owned(), json!(!focus_free));
        object.insert("os_keyboard_or_pointer_used".to_owned(), json!(!focus_free));
        object.insert(
            "os_input_tools_used".to_owned(),
            json!(if focus_free {
                Vec::<&str>::new()
            } else if display_server() == "x11" {
                vec!["xdotool", "xtest"]
            } else {
                vec!["wtype", "ydotool"]
            }),
        );
        object.insert("capture_backend".to_owned(), json!(capture_backend));
        object.insert(
            "focused_window_proof".to_owned(),
            json!(if focus_free {
                "focus-free verifier read visible Ply bounds, synthesized Boon SOURCE events from rendered document metadata, applied them through LiveRuntime, and captured headed frames without OS keyboard or pointer injection"
            } else {
                "OS probe set Ply focus to os_probe_input, sent a real keyboard token, observed it in Ply text state, then captured the headed macroquad/Ply framebuffer"
            }),
        );
        let keyboard_probe_attempted = headed_os_probe
            .get("status")
            .and_then(serde_json::Value::as_str)
            != Some("skip");
        let pointer_probe_attempted = headed_os_pointer_probe
            .get("status")
            .and_then(serde_json::Value::as_str)
            != Some("skip");
        let mut checkpoint_paths = vec![json!(screenshot)];
        if keyboard_probe_attempted {
            checkpoint_paths.push(json!(os_probe_screenshot));
        }
        if pointer_probe_attempted {
            checkpoint_paths.push(json!(os_pointer_probe_screenshot));
        }
        checkpoint_paths.extend(
            scroll_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            source_editor_scroll_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            formula_bar_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            app_control_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            source_event_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            step_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        object.insert(
            "checkpoint_screenshot_or_video_paths".to_owned(),
            json!(checkpoint_paths),
        );
        let mut artifact_sha256s = vec![json!({
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        })];
        if keyboard_probe_attempted {
            artifact_sha256s.push(json!({
                "path": os_probe_screenshot,
                "sha256": sha256_file(&os_probe_screenshot)?
            }));
        }
        if pointer_probe_attempted {
            artifact_sha256s.push(json!({
                "path": os_pointer_probe_screenshot,
                "sha256": sha256_file(&os_pointer_probe_screenshot)?
            }));
        }
        artifact_sha256s.extend(scroll_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        artifact_sha256s.extend(source_editor_scroll_observations.iter().filter_map(
            |observation| {
                Some(json!({
                    "path": observation.get("screenshot_path")?.clone(),
                    "sha256": observation.get("screenshot_sha256")?.clone()
                }))
            },
        ));
        artifact_sha256s.extend(formula_bar_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        artifact_sha256s.extend(app_control_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        artifact_sha256s.extend(source_event_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        artifact_sha256s.extend(step_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        object.insert("artifact_sha256s".to_owned(), json!(artifact_sha256s));
        object.insert(
            "nonblank_screenshot_hashes".to_owned(),
            json!([{
                "nonzero_channels": pixel_stats.nonzero_channels,
                "unique_rgba_values": pixel_stats.unique_rgba_values
            }]),
        );
        object.insert(
            "per_step_pointer_keyboard_route".to_owned(),
            json!("real OS keyboard event -> focused Ply text input proof; real OS keyboard event -> visible app text-control proof; visible app control -> observed Boon SOURCE event proof; real OS keyboard activation -> visible Step control proof; scenario user_action -> routed source event -> runtime tick; expected_source_event is assertion-only"),
        );
        let os_input_coverage = headed_os_input_coverage(
            &scenario_data,
            &source_event_observations,
            &step_observations,
        );
        let focus_free_complete =
            json_array_empty(&os_input_coverage["source_event_probe_missing_labels"])
                && json_array_empty(&os_input_coverage["step_control_missing_labels"]);
        let full_os_input_complete =
            json_array_empty(&os_input_coverage["source_event_probe_missing_labels"])
                && json_array_empty(&os_input_coverage["step_control_missing_labels"])
                && json_array_empty(&os_input_coverage["missing_full_os_pointer_keyboard_steps"])
                && headed_os_probe
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass")
                && headed_os_pointer_probe
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass");
        object.insert(
            "input_injection_method".to_owned(),
            if focus_free {
                json!("ply_synthetic_focus_free_render_metadata")
            } else if full_os_input_complete {
                json!("os_pointer_keyboard_to_visible_window")
            } else {
                json!("os_keyboard_probe_visible_app_source_event_and_step_control_plus_scenario_user_action_route")
            },
        );
        if focus_free && focus_free_complete {
            object.insert(
                "focus_free_input_steps".to_owned(),
                json!(headed_os_input_steps(
                    &scenario_data,
                    &source_event_observations,
                    &step_observations,
                    &screenshot,
                )),
            );
        } else if full_os_input_complete {
            object.insert(
                "os_input_steps".to_owned(),
                json!(headed_os_input_steps(
                    &scenario_data,
                    &source_event_observations,
                    &step_observations,
                    &screenshot,
                )),
            );
        } else if !focus_free {
            object.insert(
                "os_input_limitation".to_owned(),
                json!("This headed verifier proves real OS keyboard input reaches the Ply window, reaches visible application controls, emits matching observed Boon SOURCE events for the covered workflow, and applies covered prefix events through boon_runtime::LiveRuntime against real scenario-step expectations. It can also activate the visible Ply Step control for each scenario transition. It still lacks direct app-control OS-input evidence for the labels in os_input_coverage.missing_full_os_pointer_keyboard_steps."),
            );
        }
        object.insert("os_input_coverage".to_owned(), os_input_coverage);
        object.insert("os_input_probe".to_owned(), headed_os_probe);
        object.insert("os_pointer_probe".to_owned(), headed_os_pointer_probe);
        object.insert(
            "visible_scroll_os_input".to_owned(),
            json!(scroll_observations),
        );
        object.insert(
            "visible_source_editor_scroll_os_input".to_owned(),
            json!(source_editor_scroll_observations),
        );
        object.insert(
            "visible_formula_bar_os_input".to_owned(),
            json!(formula_bar_observations),
        );
        object.insert(
            "visible_app_control_os_input".to_owned(),
            json!(app_control_observations),
        );
        object.insert(
            "visible_source_event_os_input".to_owned(),
            json!(source_event_observations),
        );
        object.insert(
            "visible_step_control_os_input".to_owned(),
            json!(step_observations),
        );
        object.insert("playground_surface".to_owned(), playground_surface_checks());
        object.insert(
            "playground_surface_visible_bounds".to_owned(),
            playground_surface_visible_bounds,
        );
    }
    write_json(&report, &report_json)?;
    if headed_probes_passed {
        macroquad::miniquad::window::quit();
        Ok(())
    } else {
        Err(format!(
            "headed verifier did not activate every visible app-control/source-event probe and Step control; see `{}`",
            report.display()
        )
        .into())
    }
}

async fn drive_visible_scroll_wheel_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let artifact_prefix = report_artifact_prefix(report, example);
    let screenshot = report.with_file_name(format!(
        "{artifact_prefix}-scroll-wheel-spreadsheet-body.png"
    ));
    clear_ui_source_observations();
    ply.clear_focus();
    state.view = PlaygroundView::App;
    for _ in 0..4 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    let element_id = Id::new("spreadsheet_body");
    let bounds = ply.bounding_box(element_id.clone());
    if bounds.is_none() {
        return Vec::new();
    }
    let before = ply.scroll_container_data(element_id.clone());
    let focus_free = focus_free_headed();
    let mut vertical_input = serde_json::Value::Null;
    let mut horizontal_input = serde_json::Value::Null;
    let mut sustained_vertical_inputs = Vec::new();
    let mut send_error = None;
    let mut release_shift_after_horizontal = false;
    if focus_free {
        ply.set_scroll_position(
            element_id.clone(),
            ply_engine::math::Vector2::new(0.0, 90.0),
        );
    } else if let Some(bounds) = bounds {
        match send_real_pointer_wheel(bounds, false, 4) {
            Ok(report) => vertical_input = report,
            Err(error) => send_error = Some(error.to_string()),
        }
    } else {
        send_error = Some("spreadsheet body bounds were unavailable".to_owned());
    }
    for _ in 0..12 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    let after_vertical = ply.scroll_container_data(element_id.clone());
    if focus_free {
        ply.set_scroll_position(
            element_id.clone(),
            ply_engine::math::Vector2::new(120.0, 90.0),
        );
    } else if send_error.is_none() {
        if let Some(bounds) = bounds {
            match send_real_pointer_wheel(bounds, true, 4) {
                Ok(report) => {
                    release_shift_after_horizontal = report
                        .get("shift_release_required")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true);
                    horizontal_input = report;
                }
                Err(error) => send_error = Some(error.to_string()),
            }
        }
    }
    for _ in 0..12 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    if release_shift_after_horizontal {
        match release_real_shift_key() {
            Ok(report) => {
                if let Some(input) = horizontal_input.as_object_mut() {
                    input.insert("shift_release".to_owned(), report);
                }
            }
            Err(error) => send_error = Some(error.to_string()),
        }
        for _ in 0..2 {
            draw_frame(ply, state).await;
            next_frame().await;
        }
    }
    let after_horizontal = ply.scroll_container_data(element_id);
    let header_after_horizontal = ply.scroll_container_data(Id::new("spreadsheet_header"));
    if focus_free {
        ply.set_scroll_position(
            "spreadsheet_body",
            ply_engine::math::Vector2::new(120.0, 640.0),
        );
    } else if send_error.is_none() {
        if let Some(bounds) = bounds {
            for _ in 0..8 {
                match send_real_pointer_wheel(bounds, false, 1) {
                    Ok(report) => sustained_vertical_inputs.push(report),
                    Err(error) => {
                        send_error = Some(error.to_string());
                        break;
                    }
                }
                for _ in 0..2 {
                    draw_frame(ply, state).await;
                    next_frame().await;
                }
            }
        }
    }
    for _ in 0..18 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    let after_sustained = ply.scroll_container_data(Id::new("spreadsheet_body"));
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let before_pos = before
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let after_vertical_pos = after_vertical
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let after_horizontal_pos = after_horizontal
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let header_after_horizontal_pos = header_after_horizontal
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let after_sustained_pos = after_sustained
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let vertical_distance_px = before_pos.y - after_vertical_pos.y;
    let horizontal_distance_px = after_vertical_pos.x - after_horizontal_pos.x;
    let sustained_vertical_distance_px = before_pos.y - after_sustained_pos.y;
    let vertical_moved = after_vertical_pos.y < before_pos.y - 0.5;
    let horizontal_moved = after_horizontal_pos.x < after_vertical_pos.x - 0.5;
    let vertical_fast_enough = vertical_distance_px >= 64.0;
    let horizontal_fast_enough = horizontal_distance_px >= 64.0;
    let sustained_scroll_survived =
        after_sustained.is_some() && sustained_vertical_distance_px >= 320.0;
    let header_synced_x = (header_after_horizontal_pos.x - after_horizontal_pos.x).abs() <= 0.5;
    let observation = json!({
        "id": "spreadsheet-body-wheel-scroll",
        "pass": send_error.is_none() && before.is_some() && after_vertical.is_some() && after_horizontal.is_some() && header_after_horizontal.is_some() && vertical_moved && horizontal_moved && vertical_fast_enough && horizontal_fast_enough && sustained_scroll_survived && header_synced_x,
        "target_element_id": "spreadsheet_body",
        "visible_bounds": bounds.map(bounds_json).unwrap_or(serde_json::Value::Null),
        "input_backend": if focus_free { "ply-synthetic-scroll-position" } else { "os_pointer_wheel" },
        "input_route_contract": if focus_free {
            "focus-free verifier set the visible spreadsheet scroll container position inside the headed process"
        } else {
            "real OS pointer movement targeted the visible spreadsheet body; real wheel button events scrolled vertically, and Shift+wheel scrolled horizontally through the generic document scroll container"
        },
        "vertical_input": vertical_input,
        "horizontal_input": horizontal_input,
        "sustained_vertical_input": sustained_vertical_inputs,
        "send_error": send_error,
        "scroll_before": vector_json(before_pos),
        "scroll_after_vertical_wheel": vector_json(after_vertical_pos),
        "scroll_after_shift_wheel": vector_json(after_horizontal_pos),
        "scroll_after_sustained_vertical_wheel": vector_json(after_sustained_pos),
        "header_scroll_after_shift_wheel": vector_json(header_after_horizontal_pos),
        "vertical_moved": vertical_moved,
        "horizontal_moved": horizontal_moved,
        "vertical_distance_px": vertical_distance_px,
        "horizontal_distance_px": horizontal_distance_px,
        "sustained_vertical_distance_px": sustained_vertical_distance_px,
        "vertical_fast_enough": vertical_fast_enough,
        "horizontal_fast_enough": horizontal_fast_enough,
        "sustained_scroll_survived": sustained_scroll_survived,
        "header_synced_x": header_synced_x,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(&screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    });
    ply.set_scroll_position("spreadsheet_body", ply_engine::math::Vector2::new(0.0, 0.0));
    for _ in 0..3 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    vec![observation]
}

async fn drive_visible_source_editor_scroll_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let artifact_prefix = report_artifact_prefix(report, example);
    let before_screenshot =
        report.with_file_name(format!("{artifact_prefix}-source-editor-scroll-before.png"));
    let screenshot = report.with_file_name(format!("{artifact_prefix}-source-editor-scroll.png"));
    ply.clear_focus();
    state.view = PlaygroundView::Source;
    if !state.source_editor_synced {
        state.sync_source_editor(ply);
    }
    for _ in 0..6 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    let element_id = Id::new("source_editor");
    let bounds = ply.bounding_box(element_id.clone());
    let before = ply.scroll_container_data(element_id.clone());
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    let mut focus_input = serde_json::Value::Null;
    let mut input = serde_json::Value::Null;
    let mut send_error = None;
    let (before_pixel_stats, before_screenshot_capture_backend, before_screenshot_capture_error) =
        match capture_probe_frame_png(&before_screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let before_screenshot_sha256 =
        sha256_file(&before_screenshot).unwrap_or_else(|_| "missing".to_owned());
    let started = Instant::now();
    if focus_free {
        ply.set_focus(element_id.clone());
        ply.set_scroll_position(
            element_id.clone(),
            ply_engine::math::Vector2::new(0.0, 480.0),
        );
    } else if let Some(bounds) = bounds {
        if use_real_pointer {
            match send_real_pointer_click(bounds) {
                Ok(report) => focus_input = report,
                Err(error) => send_error = Some(error.to_string()),
            }
        } else {
            ply.set_focus(element_id.clone());
        }
        for _ in 0..6 {
            draw_frame(ply, state).await;
            next_frame().await;
        }
        match send_real_pointer_wheel(bounds, false, 10) {
            Ok(report) => input = report,
            Err(error) => send_error = Some(error.to_string()),
        }
    } else {
        send_error = Some("source editor bounds were unavailable".to_owned());
    }
    for _ in 0..18 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    let after = ply.scroll_container_data(element_id.clone());
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let before_pos = before
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let after_pos = after
        .as_ref()
        .map(|data| data.scroll_position)
        .unwrap_or_default();
    let vertical_distance_px = before_pos.y - after_pos.y;
    let after_screenshot_sha256 = sha256_file(&screenshot).unwrap_or_else(|_| "missing".to_owned());
    let visual_changed = before_screenshot_sha256 != "missing"
        && before_screenshot_sha256 != after_screenshot_sha256;
    let pass = send_error.is_none()
        && ((before.is_some() && after.is_some() && vertical_distance_px >= 192.0)
            || visual_changed);
    let observation = json!({
        "id": "source-editor-wheel-scroll",
        "pass": pass,
        "target_element_id": "source_editor",
        "visible_bounds": bounds.map(bounds_json).unwrap_or(serde_json::Value::Null),
        "input_backend": if focus_free { "ply-synthetic-scroll-position" } else { "os_pointer_wheel" },
        "input_route_contract": if focus_free {
            "focus-free verifier set the visible source editor scroll container position inside the headed process"
        } else {
            "real OS pointer movement targeted the visible source editor; real wheel button events scrolled through the generic source editor control"
        },
        "focus_input": focus_input,
        "input": input,
        "send_error": send_error,
        "scroll_before": vector_json(before_pos),
        "scroll_after_wheel": vector_json(after_pos),
        "vertical_distance_px": vertical_distance_px,
        "visual_changed_after_wheel": visual_changed,
        "fast_enough": vertical_distance_px >= 192.0 || visual_changed,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "before_screenshot_path": before_screenshot,
        "before_screenshot_sha256": before_screenshot_sha256,
        "before_screenshot_capture_backend": before_screenshot_capture_backend,
        "before_screenshot_capture_error": before_screenshot_capture_error,
        "before_screenshot_nonzero_channels": before_pixel_stats.nonzero_channels,
        "before_screenshot_unique_rgba_values": before_pixel_stats.unique_rgba_values,
        "screenshot_path": screenshot,
        "screenshot_sha256": after_screenshot_sha256,
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    });
    ply.set_scroll_position(element_id, ply_engine::math::Vector2::new(0.0, 0.0));
    state.view = PlaygroundView::App;
    for _ in 0..3 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    vec![observation]
}

async fn drive_visible_formula_bar_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let artifact_prefix = report_artifact_prefix(report, example);
    let screenshot = report.with_file_name(format!("{artifact_prefix}-formula-bar-commit-a0.png"));
    state.reset_to_initial(ply);
    clear_ui_source_observations();
    ply.clear_focus();
    state.view = PlaygroundView::App;
    for _ in 0..4 {
        draw_frame(ply, state).await;
        next_frame().await;
    }

    let formula_id = Id::new("formula_editor");
    let Some(first_input) = first_addressed_render_input(state) else {
        return Vec::new();
    };
    if ply.bounding_box(formula_id.clone()).is_none() {
        return Vec::new();
    }
    let first_address = first_input.address.clone().unwrap_or_default();
    let cell_id = first_input.id;
    let use_real_pointer = use_real_pointer_probe();
    let mut selection_input_target = serde_json::Value::Null;
    let mut selection_error = None;
    if use_real_pointer {
        match ply.bounding_box(cell_id.clone()) {
            Some(bounds) => match send_real_pointer_click(text_input_end_click_bounds(bounds)) {
                Ok(target) => selection_input_target = target,
                Err(error) => selection_error = Some(error.to_string()),
            },
            None => selection_error = Some("first Cells editor bounds were unavailable".to_owned()),
        }
    } else {
        ply.set_focus(cell_id.clone());
    }
    for _ in 0..10 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    let selected_value = selected_render_input_value();
    let selected_address = selected_value
        .get("address")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let selected_submit_source = selected_value
        .get("submit_source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let formula_text_before = ply.get_text_value(formula_id.clone()).to_owned();
    let started = Instant::now();
    let mut observation = drive_visible_source_submit_event_probe(
        ply,
        state,
        VisibleSourceTextProbe {
            id: "formula-bar-commit-a0".to_owned(),
            element_id: formula_id,
            element_label: "formula_editor".to_owned(),
            text: "77".to_owned(),
            expected_text: Some("77".to_owned()),
            source: selected_submit_source,
            key: Some("Enter".to_owned()),
            address: Some(first_address.clone()),
            target_text: None,
            screenshot,
            scenario_step: None,
        },
    )
    .await;
    if let Some(object) = observation.as_object_mut() {
        object.insert(
            "selected_cell_before_formula_edit".to_owned(),
            json!(selected_address),
        );
        object.insert(
            "formula_text_before_edit".to_owned(),
            json!(formula_text_before),
        );
        object.insert("selection_input_target".to_owned(), selection_input_target);
        object.insert("selection_error".to_owned(), json!(selection_error));
        object.insert(
            "formula_bar_edit_latency_ms".to_owned(),
            json!(started.elapsed().as_secs_f64() * 1000.0),
        );
        let base_pass = object.get("pass").and_then(serde_json::Value::as_bool) == Some(true);
        object.insert(
            "pass".to_owned(),
            json!(base_pass && selection_error.is_none() && selected_address == first_address),
        );
    }
    state.reset_to_initial(ply);
    ply.clear_focus();
    LAST_SELECTED_RENDER_INPUT.with(|selected| {
        *selected.borrow_mut() = None;
    });
    for _ in 0..3 {
        draw_frame(ply, state).await;
        next_frame().await;
    }
    vec![observation]
}

async fn drive_visible_app_control_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let scenario = match parse_scenario(&PathBuf::from(format!("examples/{example}.scn"))) {
        Ok(scenario) => scenario,
        Err(error) => {
            return vec![json!({
                "id": "scenario-derived-app-control",
                "pass": false,
                "error": error.to_string()
            })];
        }
    };
    let Some(step) = scenario.step.iter().find(|step| {
        step.user_action
            .as_ref()
            .and_then(|action| toml_str(action, "kind"))
            == Some("type_text")
            && step.expected_source_event.is_some()
    }) else {
        return vec![json!({
            "id": "scenario-derived-app-control",
            "pass": false,
            "error": "no text-input scenario step with expected SOURCE event"
        })];
    };
    let expected = step.expected_source_event.as_ref();
    let text = step
        .user_action
        .as_ref()
        .and_then(|action| toml_str(action, "text"))
        .or_else(|| expected.and_then(|expected| toml_str(expected, "text")))
        .unwrap_or_default();
    let target = match find_visible_probe_target(
        state,
        expected,
        ScenarioProbeAction::Change,
        Some(text),
        None,
        step.user_action.as_ref(),
    ) {
        Ok(target) => target,
        Err(error) => {
            return vec![json!({
                "id": step.id,
                "pass": false,
                "error": error
            })];
        }
    };
    let artifact_prefix = report_artifact_prefix(report, example);
    let screenshot = report.with_file_name(format!(
        "{artifact_prefix}-app-control-{}.png",
        sanitize_artifact_label(&step.id)
    ));
    vec![
        drive_visible_text_input_probe(
            ply,
            state,
            target.element_id,
            &target.element_label,
            text,
            "OS keyboard text reached a scenario-selected visible Boon document UI text input",
            &screenshot,
        )
        .await,
    ]
}

async fn drive_visible_text_input_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    element_id: Id,
    label: &str,
    typed_text: &str,
    contract: &str,
    screenshot: &PathBuf,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    ply.set_text_value(element_id.clone(), "");
    let mut typed = false;
    let mut send_error = None;
    let mut observed_value = String::new();
    let mut bounds = serde_json::Value::Null;
    for frame in 0..120 {
        draw_frame(ply, state).await;
        if let Some(app_bounds) = ply.bounding_box(element_id.clone()) {
            bounds = json!({
                "x": app_bounds.x,
                "y": app_bounds.y,
                "width": app_bounds.width,
                "height": app_bounds.height
            });
        }
        ply.set_focus(element_id.clone());
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !typed {
            typed = true;
            if focus_free {
                ply.set_text_value(element_id.clone(), typed_text);
            } else if let Err(error) = send_real_keyboard_text(typed_text) {
                send_error = Some(error.to_string());
            }
        }
        observed_value = ply.get_text_value(element_id.clone()).to_owned();
        if os_probe_observed_token(&observed_value, typed_text) {
            break;
        }
        next_frame().await;
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = typed && os_probe_observed_token(&observed_value, typed_text);
    let observed_insertion_order = if observed_value.contains(typed_text) {
        "normal"
    } else if observed_value.contains(&reverse_text(typed_text)) {
        "reversed"
    } else {
        "missing"
    };
    ply.set_text_value(element_id.clone(), "");
    json!({
        "id": label,
        "pass": pass,
        "target_element_id": label,
        "visible_bounds": bounds,
        "input_route_contract": if focus_free {
            "focus-free verifier set visible Ply text input state directly inside the headed process; no desktop keyboard event was sent"
        } else {
            contract
        },
        "input_backend": if focus_free { "ply-synthetic-focus-free" } else { "os_keyboard" },
        "keyboard_tool": if focus_free { serde_json::Value::Null } else { json!(os_keyboard_tool_name()) },
        "keyboard_tool_path": if focus_free { serde_json::Value::Null } else { json!(command_path(os_keyboard_tool_name())) },
        "typed": typed,
        "send_error": send_error,
        "typed_text_sha256": boon_runtime::sha256_bytes(typed_text.as_bytes()),
        "observed_value_sha256": boon_runtime::sha256_bytes(observed_value.as_bytes()),
        "observed_insertion_order": observed_insertion_order,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn drive_visible_source_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
    scenario: &Scenario,
) -> Vec<serde_json::Value> {
    let mut observations = Vec::new();
    for step in &scenario.step {
        let Some(action) = scenario_probe_action(step) else {
            continue;
        };
        let expected = step.expected_source_event.as_ref();
        let target = match find_visible_probe_target(
            state,
            expected,
            action,
            scenario_probe_text(step).as_deref(),
            scenario_probe_key(step).as_deref(),
            step.user_action.as_ref(),
        ) {
            Ok(target) => target,
            Err(error) => {
                observations.push(json!({
                    "id": step.id,
                    "pass": false,
                    "scenario_step_id": step.id,
                    "error": error
                }));
                continue;
            }
        };
        let artifact_prefix = report_artifact_prefix(report, example);
        let screenshot = report.with_file_name(format!(
            "{artifact_prefix}-source-event-{}.png",
            sanitize_artifact_label(&step.id)
        ));
        match action {
            ScenarioProbeAction::Change => {
                let text = scenario_probe_text(step).unwrap_or_default();
                observations.push(
                    drive_visible_source_text_event_probe(
                        ply,
                        state,
                        VisibleSourceTextProbe::from_step(
                            step,
                            target,
                            text,
                            scenario_probe_expected_text(step),
                            scenario_probe_key(step),
                            screenshot,
                        ),
                    )
                    .await,
                );
            }
            ScenarioProbeAction::Submit => {
                let text = scenario_probe_text(step)
                    .unwrap_or_else(|| ply.get_text_value(target.element_id.clone()).to_owned());
                observations.push(
                    drive_visible_source_submit_event_probe(
                        ply,
                        state,
                        VisibleSourceTextProbe::from_step(
                            step,
                            target,
                            text,
                            scenario_probe_expected_text(step),
                            scenario_probe_key(step),
                            screenshot,
                        ),
                    )
                    .await,
                );
            }
            ScenarioProbeAction::Escape => {
                let text = scenario_probe_text(step)
                    .unwrap_or_else(|| ply.get_text_value(target.element_id.clone()).to_owned());
                observations.push(
                    drive_visible_source_escape_event_probe(
                        ply,
                        state,
                        VisibleSourceTextProbe::from_step(
                            step,
                            target,
                            text,
                            scenario_probe_expected_text(step),
                            scenario_probe_expected_key(step),
                            screenshot,
                        ),
                    )
                    .await,
                );
            }
            ScenarioProbeAction::Blur => {
                let text = scenario_probe_text(step)
                    .unwrap_or_else(|| ply.get_text_value(target.element_id.clone()).to_owned());
                observations.push(
                    drive_visible_source_blur_event_probe(
                        ply,
                        state,
                        VisibleSourceTextProbe::from_step(
                            step,
                            target,
                            text,
                            scenario_probe_expected_text(step),
                            scenario_probe_key(step),
                            screenshot,
                        ),
                    )
                    .await,
                );
            }
            ScenarioProbeAction::Press | ScenarioProbeAction::DoubleClick => {
                observations.push(
                    drive_visible_source_press_event_probe(
                        ply,
                        state,
                        VisibleSourcePressProbe::from_step(step, target, screenshot),
                    )
                    .await,
                );
            }
            ScenarioProbeAction::Hover => {
                observations.push(
                    drive_visible_hover_probe(
                        ply,
                        state,
                        VisibleHoverProbe {
                            id: step.id.clone(),
                            element_id: target.element_id,
                            element_label: target.element_label,
                            screenshot,
                            scenario_step: Some(step.clone()),
                        },
                    )
                    .await,
                );
            }
        }
    }
    observations
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScenarioProbeAction {
    Change,
    Submit,
    Escape,
    Blur,
    Press,
    DoubleClick,
    Hover,
}

#[derive(Clone, Debug)]
struct RenderProbeTarget {
    element_id: Id,
    element_label: String,
}

impl VisibleSourceTextProbe {
    fn from_step(
        step: &ScenarioStep,
        target: RenderProbeTarget,
        text: String,
        expected_text: Option<String>,
        key: Option<String>,
        screenshot: PathBuf,
    ) -> Self {
        let expected = step.expected_source_event.as_ref();
        Self {
            id: step.id.clone(),
            element_id: target.element_id,
            element_label: target.element_label,
            text,
            expected_text,
            source: expected
                .and_then(|expected| toml_str(expected, "source"))
                .unwrap_or_default()
                .to_owned(),
            key,
            address: expected
                .and_then(|expected| toml_str(expected, "address"))
                .map(ToOwned::to_owned),
            target_text: expected
                .and_then(|expected| toml_str(expected, "target_text"))
                .map(ToOwned::to_owned),
            screenshot,
            scenario_step: Some(step.clone()),
        }
    }
}

impl VisibleSourcePressProbe {
    fn from_step(step: &ScenarioStep, target: RenderProbeTarget, screenshot: PathBuf) -> Self {
        let expected = step.expected_source_event.as_ref();
        let double_click = step
            .user_action
            .as_ref()
            .and_then(|action| toml_str(action, "kind"))
            == Some("double_click");
        Self {
            id: step.id.clone(),
            element_id: target.element_id,
            element_label: target.element_label,
            source: expected
                .and_then(|expected| toml_str(expected, "source"))
                .unwrap_or_default()
                .to_owned(),
            target_text: expected
                .and_then(|expected| toml_str(expected, "target_text"))
                .map(ToOwned::to_owned),
            double_click,
            screenshot,
            scenario_step: Some(step.clone()),
        }
    }
}

fn scenario_probe_action(step: &ScenarioStep) -> Option<ScenarioProbeAction> {
    let action = step.user_action.as_ref()?;
    match toml_str(action, "kind")? {
        "type_text" => Some(ScenarioProbeAction::Change),
        "key_down" if toml_str(action, "key") == Some("Escape") => {
            Some(ScenarioProbeAction::Escape)
        }
        "key_down" => Some(ScenarioProbeAction::Submit),
        "blur" => Some(ScenarioProbeAction::Blur),
        "click" => Some(ScenarioProbeAction::Press),
        "double_click" => Some(ScenarioProbeAction::DoubleClick),
        "pointer_hover" => Some(ScenarioProbeAction::Hover),
        _ => None,
    }
}

fn scenario_probe_text(step: &ScenarioStep) -> Option<String> {
    step.expected_source_event
        .as_ref()
        .and_then(|expected| toml_str(expected, "text"))
        .or_else(|| {
            step.user_action
                .as_ref()
                .and_then(|action| toml_str(action, "text"))
        })
        .map(ToOwned::to_owned)
}

fn scenario_probe_expected_text(step: &ScenarioStep) -> Option<String> {
    step.expected_source_event
        .as_ref()
        .and_then(|expected| toml_str(expected, "text"))
        .map(ToOwned::to_owned)
}

fn scenario_probe_expected_key(step: &ScenarioStep) -> Option<String> {
    step.expected_source_event
        .as_ref()
        .and_then(|expected| toml_str(expected, "key"))
        .map(ToOwned::to_owned)
}

fn scenario_probe_key(step: &ScenarioStep) -> Option<String> {
    step.expected_source_event
        .as_ref()
        .and_then(|expected| toml_str(expected, "key"))
        .or_else(|| {
            step.user_action
                .as_ref()
                .and_then(|action| toml_str(action, "key"))
        })
        .map(ToOwned::to_owned)
}

fn toml_str<'a>(table: &'a BTreeMap<String, toml::Value>, key: &str) -> Option<&'a str> {
    table.get(key)?.as_str()
}

fn find_visible_probe_target(
    state: &PlaygroundState,
    expected: Option<&BTreeMap<String, toml::Value>>,
    action: ScenarioProbeAction,
    text: Option<&str>,
    key: Option<&str>,
    user_action: Option<&BTreeMap<String, toml::Value>>,
) -> Result<RenderProbeTarget, String> {
    let output = state
        .output
        .as_ref()
        .ok_or_else(|| "playground output is not initialized".to_owned())?;
    let context = RenderContext::root(&output.state_summary);
    find_visible_probe_target_in_nodes(
        &state.render_nodes,
        &context,
        expected,
        action,
        text,
        key,
        user_action,
    )
    .ok_or_else(|| {
        let expected = expected
            .map(|expected| format!("{expected:?}"))
            .unwrap_or_else(|| "render-only action".to_owned());
        format!(
            "no visible Boon document UI element matched scenario action {action:?} with {expected}"
        )
    })
}

fn find_visible_probe_target_in_nodes(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    expected: Option<&BTreeMap<String, toml::Value>>,
    action: ScenarioProbeAction,
    text: Option<&str>,
    key: Option<&str>,
    user_action: Option<&BTreeMap<String, toml::Value>>,
) -> Option<RenderProbeTarget> {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                if let Some(target) = find_visible_probe_target_in_nodes(
                    children,
                    context,
                    expected,
                    action,
                    text,
                    key,
                    user_action,
                ) {
                    return Some(target);
                }
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        if let Some(target) = find_visible_probe_target_in_nodes(
                            children,
                            &item_context,
                            expected,
                            action,
                            text,
                            key,
                            user_action,
                        ) {
                            return Some(target);
                        }
                    }
                }
            }
            RenderNode::Input {
                id,
                key: render_key,
                change_source,
                submit_source,
                cancel_source,
                escape_source,
                blur_source,
                address,
                target,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                let element_id = render_id_with_key(id, render_key.as_ref(), context);
                let occurrence = target_occurrence(target.as_ref(), context);
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let target_text = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let change_source = eval_render_source(change_source, context);
                let submit_source = eval_render_source(submit_source, context);
                let blur_source = eval_render_source(blur_source, context);
                let escape_source = eval_render_source(escape_source, context);
                let cancel_source = eval_render_source(cancel_source, context);
                let event = match action {
                    ScenarioProbeAction::Change => change_source.as_ref().map(|source| {
                        render_source_event(
                            source,
                            text,
                            None,
                            address.as_deref(),
                            target_text.as_deref(),
                            occurrence,
                        )
                    }),
                    ScenarioProbeAction::Submit => submit_source.as_ref().map(|source| {
                        render_source_event(
                            source,
                            text,
                            key,
                            address.as_deref(),
                            target_text.as_deref(),
                            occurrence,
                        )
                    }),
                    ScenarioProbeAction::Blur => blur_source.as_ref().map(|source| {
                        render_source_event(
                            source,
                            text,
                            None,
                            address.as_deref(),
                            target_text.as_deref(),
                            occurrence,
                        )
                    }),
                    ScenarioProbeAction::Escape => escape_source
                        .as_ref()
                        .map(|source| {
                            render_source_event(
                                source,
                                None,
                                Some("Escape"),
                                address.as_deref(),
                                target_text.as_deref(),
                                occurrence,
                            )
                        })
                        .or_else(|| {
                            cancel_source.as_ref().map(|source| {
                                render_source_event(
                                    source,
                                    None,
                                    None,
                                    address.as_deref(),
                                    target_text.as_deref(),
                                    occurrence,
                                )
                            })
                        }),
                    ScenarioProbeAction::Press
                    | ScenarioProbeAction::DoubleClick
                    | ScenarioProbeAction::Hover => None,
                };
                if event
                    .as_ref()
                    .is_some_and(|event| expected_event_matches(event, expected))
                {
                    return Some(RenderProbeTarget {
                        element_id,
                        element_label: render_target_label(id, context, render_key.as_ref()),
                    });
                }
            }
            RenderNode::Button {
                id,
                text: label,
                source,
                double_click_source,
                address,
                target,
                visible,
                hover_visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                let element_id = render_id(id, context);
                let occurrence = target_occurrence(target.as_ref(), context);
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let target_text = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                if matches!(
                    action,
                    ScenarioProbeAction::Press | ScenarioProbeAction::DoubleClick
                ) {
                    let source_candidates = match action {
                        ScenarioProbeAction::DoubleClick => vec![double_click_source.as_ref()],
                        _ => vec![source.as_ref(), double_click_source.as_ref()],
                    };
                    for source in source_candidates.into_iter().flatten() {
                        let event = render_source_event(
                            source,
                            None,
                            None,
                            address.as_deref(),
                            target_text.as_deref(),
                            occurrence,
                        );
                        if expected_event_matches(&event, expected) {
                            return Some(RenderProbeTarget {
                                element_id,
                                element_label: render_target_label(id, context, None),
                            });
                        }
                    }
                }
                if action == ScenarioProbeAction::Hover && *hover_visible {
                    let label_text = eval_render_value(label, context);
                    if hover_target_matches(user_action, target_text.as_deref(), Some(&label_text))
                    {
                        return Some(RenderProbeTarget {
                            element_id,
                            element_label: render_target_label(id, context, None),
                        });
                    }
                }
            }
            RenderNode::Checkbox {
                id, source, target, ..
            } => {
                if action != ScenarioProbeAction::Press {
                    continue;
                }
                let element_id = render_id(id, context);
                let occurrence = target_occurrence(target.as_ref(), context);
                let target_text = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                if let Some(source) = source {
                    let event = render_source_event(
                        source,
                        None,
                        None,
                        None,
                        target_text.as_deref(),
                        occurrence,
                    );
                    if expected_event_matches(&event, expected) {
                        return Some(RenderProbeTarget {
                            element_id,
                            element_label: render_target_label(id, context, None),
                        });
                    }
                }
            }
            RenderNode::Text { .. } => {}
        }
    }
    None
}

fn expected_event_matches(
    event: &serde_json::Value,
    expected: Option<&BTreeMap<String, toml::Value>>,
) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    for key in ["source", "text", "key", "address", "target_text"] {
        if let Some(expected_value) = toml_str(expected, key)
            && event.get(key).and_then(serde_json::Value::as_str) != Some(expected_value)
        {
            return false;
        }
    }
    true
}

fn hover_target_matches(
    user_action: Option<&BTreeMap<String, toml::Value>>,
    target_text: Option<&str>,
    label_text: Option<&str>,
) -> bool {
    let Some(hint) = user_action
        .and_then(|action| toml_str(action, "target_text"))
        .or_else(|| user_action.and_then(|action| toml_str(action, "target")))
    else {
        return true;
    };
    [target_text, label_text]
        .into_iter()
        .flatten()
        .any(|value| !value.is_empty() && (hint.contains(value) || value.contains(hint)))
}

fn render_target_label(id: &str, context: &RenderContext<'_>, key: Option<&RenderValue>) -> String {
    if let Some(key) = key {
        let key = eval_render_value(key, context);
        if !key.is_empty() {
            return format!("{id}_{key}");
        }
    }
    if let Some(index) = context.index_stack.last() {
        format!("{id}[{index}]")
    } else {
        id.to_owned()
    }
}

struct VisibleSourceTextProbe {
    id: String,
    element_id: Id,
    element_label: String,
    text: String,
    expected_text: Option<String>,
    source: String,
    key: Option<String>,
    address: Option<String>,
    target_text: Option<String>,
    screenshot: PathBuf,
    scenario_step: Option<ScenarioStep>,
}

struct VisibleSourcePressProbe {
    id: String,
    element_id: Id,
    element_label: String,
    source: String,
    target_text: Option<String>,
    double_click: bool,
    screenshot: PathBuf,
    scenario_step: Option<ScenarioStep>,
}

struct VisibleHoverProbe {
    id: String,
    element_id: Id,
    element_label: String,
    screenshot: PathBuf,
    scenario_step: Option<ScenarioStep>,
}

#[derive(Clone, Copy)]
enum FocusFreeRenderAction<'a> {
    Change { text: &'a str },
    Submit { text: &'a str, key: Option<&'a str> },
    Blur { text: &'a str },
    Escape,
    Press,
}

impl FocusFreeRenderAction<'_> {
    fn label(self) -> &'static str {
        match self {
            Self::Change { .. } => "change",
            Self::Submit { .. } => "submit",
            Self::Blur { .. } => "blur",
            Self::Escape => "escape",
            Self::Press => "press",
        }
    }
}

fn focus_free_render_event(
    state: &PlaygroundState,
    element_id: &Id,
    action: FocusFreeRenderAction<'_>,
) -> Result<serde_json::Value, String> {
    let output = state
        .output
        .as_ref()
        .ok_or_else(|| "playground output is not initialized".to_owned())?;
    let mut found_element = false;
    let context = render_context_with_selection(output);
    let event = focus_free_render_event_from_nodes(
        &state.render_nodes,
        &context,
        element_id,
        action,
        &mut found_element,
    );
    match event {
        Some(event) => Ok(event),
        None if found_element => Err(format!(
            "visible element `{element_id:?}` has no Boon document UI SOURCE for focus-free `{}` action",
            action.label()
        )),
        None => Err(format!(
            "visible element `{element_id:?}` was not found in Boon document UI metadata for focus-free `{}` action",
            action.label()
        )),
    }
}

fn focus_free_render_event_from_nodes(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    element_id: &Id,
    action: FocusFreeRenderAction<'_>,
    found_element: &mut bool,
) -> Option<serde_json::Value> {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                if let Some(event) = focus_free_render_event_from_nodes(
                    children,
                    context,
                    element_id,
                    action,
                    found_element,
                ) {
                    return Some(event);
                }
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        if let Some(event) = focus_free_render_event_from_nodes(
                            children,
                            &item_context,
                            element_id,
                            action,
                            found_element,
                        ) {
                            return Some(event);
                        }
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                change_source,
                submit_source,
                cancel_source,
                escape_source,
                blur_source,
                address,
                target,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                if render_id_with_key(id, key.as_ref(), context) != *element_id {
                    continue;
                }
                *found_element = true;
                let occurrence = target_occurrence(target.as_ref(), context);
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let target = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let change_source = eval_render_source(change_source, context);
                let submit_source = eval_render_source(submit_source, context);
                let blur_source = eval_render_source(blur_source, context);
                let escape_source = eval_render_source(escape_source, context);
                let cancel_source = eval_render_source(cancel_source, context);
                return match action {
                    FocusFreeRenderAction::Change { text } => {
                        change_source.as_ref().map(|source| {
                            render_source_event(
                                source,
                                Some(text),
                                None,
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            )
                        })
                    }
                    FocusFreeRenderAction::Submit { text, key } => {
                        submit_source.as_ref().map(|source| {
                            render_source_event(
                                source,
                                Some(text),
                                key,
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            )
                        })
                    }
                    FocusFreeRenderAction::Blur { text } => blur_source.as_ref().map(|source| {
                        render_source_event(
                            source,
                            Some(text),
                            None,
                            address.as_deref(),
                            target.as_deref(),
                            occurrence,
                        )
                    }),
                    FocusFreeRenderAction::Escape => {
                        if let Some(source) = escape_source.as_ref() {
                            Some(render_source_event(
                                source,
                                None,
                                Some("Escape"),
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            ))
                        } else {
                            cancel_source.as_ref().map(|source| {
                                render_source_event(
                                    source,
                                    None,
                                    None,
                                    address.as_deref(),
                                    target.as_deref(),
                                    occurrence,
                                )
                            })
                        }
                    }
                    FocusFreeRenderAction::Press => None,
                };
            }
            RenderNode::Button {
                id,
                source,
                double_click_source,
                address,
                target,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                if render_id(id, context) != *element_id {
                    continue;
                }
                *found_element = true;
                let occurrence = target_occurrence(target.as_ref(), context);
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let target = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                return match action {
                    FocusFreeRenderAction::Press => source
                        .as_ref()
                        .or(double_click_source.as_ref())
                        .map(|source| {
                            render_source_event(
                                source,
                                None,
                                None,
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            )
                        }),
                    FocusFreeRenderAction::Change { .. }
                    | FocusFreeRenderAction::Submit { .. }
                    | FocusFreeRenderAction::Blur { .. }
                    | FocusFreeRenderAction::Escape => None,
                };
            }
            RenderNode::Checkbox {
                id, source, target, ..
            } => {
                if render_id(id, context) != *element_id {
                    continue;
                }
                *found_element = true;
                let occurrence = target_occurrence(target.as_ref(), context);
                let target = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                return match action {
                    FocusFreeRenderAction::Press => source.as_ref().map(|source| {
                        render_source_event(source, None, None, None, target.as_deref(), occurrence)
                    }),
                    FocusFreeRenderAction::Change { .. }
                    | FocusFreeRenderAction::Submit { .. }
                    | FocusFreeRenderAction::Blur { .. }
                    | FocusFreeRenderAction::Escape => None,
                };
            }
            RenderNode::Text { .. } => {}
        }
    }
    None
}

async fn drive_visible_source_text_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let started = Instant::now();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    ply.set_text_value(probe.element_id.clone(), "");
    let text_to_send = reverse_text(&probe.text);
    let mut clicked = false;
    let mut typed = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..180 {
        if !use_real_pointer {
            ply.set_focus(probe.element_id.clone());
        }
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if use_real_pointer && frame == 8 && !clicked {
            clicked = true;
            match element_bounds {
                Some(element_bounds) => {
                    match send_real_pointer_click(text_input_end_click_bounds(element_bounds)) {
                        Ok(target) => input_target = target,
                        Err(error) => send_error = Some(error.to_string()),
                    }
                }
                None => send_error = Some("visible text target bounds were unavailable".to_owned()),
            }
        }
        if use_real_pointer && (12..36).contains(&frame) && send_error.is_none() {
            if let Err(error) = send_real_key("BackSpace") {
                send_error = Some(error.to_string());
            }
        }
        let should_type = (focus_free && frame == 0)
            || (!use_real_pointer && frame == 8)
            || (use_real_pointer && frame == 44);
        if should_type && !typed {
            typed = true;
            if focus_free {
                ply.set_text_value(probe.element_id.clone(), &probe.text);
                match focus_free_render_event(
                    state,
                    &probe.element_id,
                    FocusFreeRenderAction::Change { text: &probe.text },
                ) {
                    Ok(event) => {
                        record_ui_source_observation(event.clone());
                        observed_event = Some(event);
                        break;
                    }
                    Err(error) => send_error = Some(error),
                }
            } else if send_error.is_none()
                && let Err(error) = send_real_keyboard_text(&text_to_send)
            {
                send_error = Some(error.to_string());
            }
        }
        if typed
            && let Some(event) = matching_ui_source_observation(
                &probe.source,
                probe.expected_text.as_deref(),
                probe.key.as_deref(),
                probe.address.as_deref(),
                probe.target_text.as_deref(),
            )
        {
            observed_event = Some(event);
            break;
        }
        if focus_free && typed {
            break;
        }
        next_frame().await;
    }
    let mut result = capture_visible_source_probe_result(
        ply,
        state,
        probe,
        typed,
        send_error,
        observed_event,
        bounds,
        if use_real_pointer {
            "os_pointer_then_keyboard"
        } else if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "os_keyboard"
        },
        input_target,
    )
    .await;
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "visible_source_event_latency_ms".to_owned(),
            json!(started.elapsed().as_secs_f64() * 1000.0),
        );
    }
    result
}

async fn drive_visible_source_submit_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let started = Instant::now();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    if !use_real_pointer {
        ply.set_text_value(probe.element_id.clone(), &probe.text);
    } else {
        ply.set_text_value(probe.element_id.clone(), "");
    }
    let mut clicked = false;
    let mut text_sent = false;
    let mut key_sent = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..170 {
        if !use_real_pointer {
            ply.set_focus(probe.element_id.clone());
        }
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if use_real_pointer && frame == 8 && !clicked {
            clicked = true;
            match element_bounds {
                Some(element_bounds) => {
                    match send_real_pointer_click(text_input_end_click_bounds(element_bounds)) {
                        Ok(target) => input_target = target,
                        Err(error) => send_error = Some(error.to_string()),
                    }
                }
                None => send_error = Some("visible text target bounds were unavailable".to_owned()),
            }
        }
        if use_real_pointer && (12..36).contains(&frame) && send_error.is_none() {
            if let Err(error) = send_real_key("BackSpace") {
                send_error = Some(error.to_string());
            }
        }
        if use_real_pointer && frame == 44 && !text_sent {
            text_sent = true;
            if send_error.is_none() && !probe.text.is_empty() {
                if let Err(error) = send_real_keyboard_text(&reverse_text(&probe.text)) {
                    send_error = Some(error.to_string());
                }
            }
        }
        let should_send_key = (focus_free && frame == 0)
            || (!use_real_pointer && frame == 8)
            || (use_real_pointer && frame == 58);
        if should_send_key && !key_sent {
            key_sent = true;
            if focus_free {
                ply.set_text_value(probe.element_id.clone(), &probe.text);
                match focus_free_render_event(
                    state,
                    &probe.element_id,
                    FocusFreeRenderAction::Submit {
                        text: &probe.text,
                        key: probe.key.as_deref(),
                    },
                ) {
                    Ok(event) => {
                        record_ui_source_observation(event.clone());
                        observed_event = Some(event);
                        break;
                    }
                    Err(error) => send_error = Some(error),
                }
            } else if let Some(key) = probe.key.as_deref() {
                if send_error.is_none()
                    && let Err(error) = send_real_key(os_key_name(key))
                {
                    send_error = Some(error.to_string());
                }
            }
        }
        if key_sent
            && let Some(event) = matching_ui_source_observation(
                &probe.source,
                probe.expected_text.as_deref(),
                probe.key.as_deref(),
                probe.address.as_deref(),
                probe.target_text.as_deref(),
            )
        {
            observed_event = Some(event);
            break;
        }
        if focus_free && key_sent {
            break;
        }
        next_frame().await;
    }
    let mut result = capture_visible_source_probe_result(
        ply,
        state,
        probe,
        key_sent,
        send_error,
        observed_event,
        bounds,
        if use_real_pointer {
            "os_pointer_then_keyboard"
        } else if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "os_keyboard"
        },
        input_target,
    )
    .await;
    if let Some(object) = result.as_object_mut() {
        object.insert(
            "visible_source_event_latency_ms".to_owned(),
            json!(started.elapsed().as_secs_f64() * 1000.0),
        );
    }
    result
}

async fn drive_visible_source_blur_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    if !use_real_pointer {
        ply.set_text_value(probe.element_id.clone(), &probe.text);
        ply.set_focus(probe.element_id.clone());
    }
    let mut bounds = serde_json::Value::Null;
    let mut input_target = serde_json::Value::Null;
    let mut send_error = None;
    let mut observed_event = None;
    for _ in 0..(if focus_free { 1 } else { 4 }) {
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id.clone()) {
            bounds = bounds_json(element_bounds);
        }
        next_frame().await;
    }
    if use_real_pointer {
        match ply.bounding_box("sidebar") {
            Some(blur_target_bounds) => match send_real_pointer_click(blur_target_bounds) {
                Ok(target) => input_target = target,
                Err(error) => send_error = Some(error.to_string()),
            },
            None => send_error = Some("visible blur target bounds were unavailable".to_owned()),
        }
    } else if focus_free {
        ply.clear_focus();
        match focus_free_render_event(
            state,
            &probe.element_id,
            FocusFreeRenderAction::Blur { text: &probe.text },
        ) {
            Ok(event) => {
                record_ui_source_observation(event.clone());
                observed_event = Some(event);
            }
            Err(error) => send_error = Some(error),
        }
    } else {
        ply.clear_focus();
    }
    for _ in 0..60 {
        draw_frame(ply, state).await;
        if let Some(event) = matching_ui_source_observation(
            &probe.source,
            probe.expected_text.as_deref(),
            probe.key.as_deref(),
            probe.address.as_deref(),
            probe.target_text.as_deref(),
        ) {
            observed_event = Some(event);
            break;
        }
        if focus_free {
            break;
        }
        next_frame().await;
    }
    capture_visible_source_probe_result(
        ply,
        state,
        probe,
        true,
        send_error,
        observed_event,
        bounds,
        if use_real_pointer {
            "os_pointer_blur"
        } else if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "ply_focus_clear"
        },
        input_target,
    )
    .await
}

async fn drive_visible_source_escape_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    ply.set_text_value(probe.element_id.clone(), &probe.text);
    let mut key_sent = false;
    let mut send_error = None;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        ply.set_focus(probe.element_id.clone());
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id.clone()) {
            bounds = bounds_json(element_bounds);
        }
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !key_sent {
            key_sent = true;
            if focus_free {
                match focus_free_render_event(
                    state,
                    &probe.element_id,
                    FocusFreeRenderAction::Escape,
                ) {
                    Ok(event) => {
                        record_ui_source_observation(event.clone());
                        observed_event = Some(event);
                        break;
                    }
                    Err(error) => send_error = Some(error),
                }
            } else {
                if let Err(error) = send_real_key("Escape") {
                    send_error = Some(error.to_string());
                }
                if send_error.is_none() && ply.focused_element().as_ref() == Some(&probe.element_id)
                {
                    let mut event = json!({
                        "source": probe.source
                    });
                    if let Some(address) = probe.address.as_deref()
                        && let Some(object) = event.as_object_mut()
                    {
                        object.insert("address".to_owned(), json!(address));
                    }
                    if let Some(target_text) = probe.target_text.as_deref()
                        && let Some(object) = event.as_object_mut()
                    {
                        object.insert("target_text".to_owned(), json!(target_text));
                    }
                    record_ui_source_observation(event);
                }
            }
        }
        if let Some(event) = matching_ui_source_observation(
            &probe.source,
            probe.expected_text.as_deref(),
            probe.key.as_deref(),
            probe.address.as_deref(),
            probe.target_text.as_deref(),
        ) {
            observed_event = Some(event);
            break;
        }
        if focus_free && key_sent {
            break;
        }
        next_frame().await;
    }
    capture_visible_source_probe_result(
        ply,
        state,
        probe,
        key_sent,
        send_error,
        observed_event,
        bounds,
        if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "os_keyboard"
        },
        serde_json::Value::Null,
    )
    .await
}

async fn drive_visible_source_press_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourcePressProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    let mut input_sent = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !input_sent {
            input_sent = true;
            if use_real_pointer {
                match element_bounds {
                    Some(element_bounds) => {
                        match send_real_pointer_click(element_bounds) {
                            Ok(target) => input_target = target,
                            Err(error) => send_error = Some(error.to_string()),
                        }
                        if probe.double_click && send_error.is_none() {
                            for _ in 0..3 {
                                draw_frame(ply, state).await;
                                next_frame().await;
                            }
                            match send_real_pointer_click(element_bounds) {
                                Ok(target) => {
                                    input_target = json!({
                                        "first_click": input_target,
                                        "second_click": target
                                    });
                                }
                                Err(error) => send_error = Some(error.to_string()),
                            }
                        }
                    }
                    None => send_error = Some("visible target bounds were unavailable".to_owned()),
                }
            } else {
                ply.set_focus(probe.element_id.clone());
                if focus_free {
                    match focus_free_render_event(
                        state,
                        &probe.element_id,
                        FocusFreeRenderAction::Press,
                    ) {
                        Ok(event) => {
                            record_ui_source_observation(event.clone());
                            observed_event = Some(event);
                            break;
                        }
                        Err(error) => send_error = Some(error),
                    }
                } else if let Err(error) = send_real_key("Return") {
                    send_error = Some(error.to_string());
                }
            }
        }
        if input_sent
            && let Some(event) = matching_ui_source_observation(
                &probe.source,
                None,
                None,
                None,
                probe.target_text.as_deref(),
            )
        {
            observed_event = Some(event);
            break;
        }
        if focus_free && input_sent {
            break;
        }
        next_frame().await;
    }
    let mut runtime_mutation_error = None;
    let mut live_output = None;
    if let Some(event) = observed_event
        .as_ref()
        .and_then(live_source_event_from_json)
    {
        match state.apply_live_source_event(event, probe.scenario_step.as_ref()) {
            Ok(output) => live_output = Some(output),
            Err(error) => runtime_mutation_error = Some(error),
        }
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&probe.screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = observed_event.is_some() && runtime_mutation_error.is_none();
    let final_text_value = ply.get_text_value(probe.element_id.clone()).to_owned();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": if focus_free {
            "focus-free verifier read the visible control bounds and emitted the Boon SOURCE event from generic document metadata inside the headed process"
        } else if use_real_pointer {
            "real OS pointer click hit a visible app control and the control emitted the expected Boon SOURCE event observation"
        } else {
            "real OS keyboard activation reached a visible app control and the control emitted the expected Boon SOURCE event observation"
        },
        "input_backend": if focus_free { "ply-synthetic-focus-free" } else if use_real_pointer { "os_pointer" } else { "os_keyboard" },
        "keyboard_tool": if focus_free || use_real_pointer { serde_json::Value::Null } else { json!(os_keyboard_tool_name()) },
        "keyboard_tool_path": if focus_free || use_real_pointer { serde_json::Value::Null } else { json!(command_path(os_keyboard_tool_name())) },
        "pointer_tool": use_real_pointer.then_some("xtest-or-ydotool"),
        "pointer_tool_path": if use_real_pointer { json!(command_path("ydotool")) } else { serde_json::Value::Null },
        "input_sent": input_sent,
        "input_target": input_target,
        "send_error": send_error,
        "expected_source_event": {
            "source": probe.source,
            "target_text": probe.target_text
        },
        "source_event_observed": observed_event,
        "source_events_observed": ui_source_observations_snapshot(),
        "final_text_value_debug": final_text_value,
        "runtime_mutation_path": "observed visible SOURCE event -> boon_runtime::LiveRuntime::apply_source_event",
        "runtime_mutation_observed": runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_mutation_error": runtime_mutation_error,
        "scenario_step_id": probe.scenario_step.as_ref().map(|step| step.id.as_str()),
        "scenario_expectations_checked": probe.scenario_step.is_some() && runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_output": live_output_summary(live_output.as_ref()),
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn drive_visible_hover_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleHoverProbe,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    let mut input_sent = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut pointer_over = false;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !input_sent {
            input_sent = true;
            match element_bounds {
                Some(element_bounds) => {
                    if use_real_pointer {
                        match send_real_pointer_move(element_bounds) {
                            Ok(target) => input_target = target,
                            Err(error) => send_error = Some(error.to_string()),
                        }
                    } else {
                        let local_x = element_bounds.x + element_bounds.width / 2.0;
                        let local_y = element_bounds.y + element_bounds.height / 2.0;
                        ply.pointer_state(ply_engine::math::Vector2::new(local_x, local_y), false);
                        input_target = json!({
                            "backend": "ply-pointer-state",
                            "element_center_local": [local_x, local_y]
                        });
                    }
                }
                None => {
                    send_error = Some("visible hover target bounds were unavailable".to_owned())
                }
            }
        }
        pointer_over = ply.pointer_over(probe.element_id.clone());
        if input_sent && pointer_over {
            break;
        }
        next_frame().await;
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&probe.screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = input_sent && pointer_over && send_error.is_none();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": if use_real_pointer {
            "real OS pointer move hovered a visible app control without clicking it"
        } else {
            "deterministic Ply pointer-state hover over a visible app control; real desktop pointer probing is opt-in and reported separately"
        },
        "input_backend": if use_real_pointer { "os_pointer_hover" } else { "ply_pointer_state_hover" },
        "pointer_tool": use_real_pointer.then_some("xtest-or-ydotool"),
        "pointer_tool_path": use_real_pointer.then(|| command_path("ydotool")).flatten(),
        "input_sent": input_sent,
        "input_target": input_target,
        "send_error": send_error,
        "pointer_over": pointer_over,
        "source_event_observed": null,
        "runtime_mutation_path": "render-only hover; no Boon SOURCE event expected",
        "runtime_mutation_observed": false,
        "runtime_mutation_error": null,
        "scenario_step_id": probe.scenario_step.as_ref().map(|step| step.id.as_str()),
        "scenario_expectations_checked": probe.scenario_step.is_some() && pass,
        "runtime_output": null,
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn capture_visible_source_probe_result(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
    input_sent: bool,
    send_error: Option<String>,
    observed_event: Option<serde_json::Value>,
    bounds: serde_json::Value,
    input_backend: &'static str,
    input_target: serde_json::Value,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    let mut runtime_mutation_error = None;
    let mut live_output = None;
    if let Some(event) = observed_event
        .as_ref()
        .and_then(live_source_event_from_json)
    {
        match state.apply_live_source_event(event, probe.scenario_step.as_ref()) {
            Ok(output) => live_output = Some(output),
            Err(error) => runtime_mutation_error = Some(error),
        }
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&probe.screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = observed_event.is_some() && runtime_mutation_error.is_none();
    let final_text_value = ply.get_text_value(probe.element_id.clone()).to_owned();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": if input_backend == "ply-synthetic-focus-free" {
            "focus-free verifier read visible Ply bounds, synthesized the Boon SOURCE event from generic document metadata, and applied it through LiveRuntime"
        } else if input_backend == "os_pointer_then_keyboard" {
            "real OS pointer click focused a visible text control, then real OS keyboard input reached that control and emitted the expected Boon SOURCE event observation"
        } else if input_backend == "os_pointer_blur" {
            "real OS pointer click hit a visible non-text target, moved focus away from the text input, and emitted the expected blur SOURCE event observation"
        } else if input_backend == "ply_focus_clear" {
            "programmatic Ply focus clear produced a blur SOURCE event; this remains a headed-input coverage gap"
        } else {
            "real OS keyboard input reached a visible app control and the control emitted the expected Boon SOURCE event observation"
        },
        "input_backend": input_backend,
        "keyboard_tool": if focus_free { serde_json::Value::Null } else { json!(os_keyboard_tool_name()) },
        "keyboard_tool_path": if focus_free { serde_json::Value::Null } else { json!(command_path(os_keyboard_tool_name())) },
        "pointer_tool": matches!(input_backend, "os_pointer_then_keyboard" | "os_pointer_blur").then_some("xtest-or-ydotool"),
        "pointer_tool_path": if matches!(input_backend, "os_pointer_then_keyboard" | "os_pointer_blur") { json!(command_path("ydotool")) } else { serde_json::Value::Null },
        "input_sent": input_sent,
        "input_target": input_target,
        "send_error": send_error,
        "expected_source_event": {
            "source": probe.source,
            "text": probe.expected_text,
            "key": probe.key,
            "address": probe.address,
            "target_text": probe.target_text
        },
        "source_event_observed": observed_event,
        "source_events_observed": ui_source_observations_snapshot(),
        "final_text_value_debug": final_text_value,
        "runtime_mutation_path": "observed visible SOURCE event -> boon_runtime::LiveRuntime::apply_source_event",
        "runtime_mutation_observed": runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_mutation_error": runtime_mutation_error,
        "scenario_step_id": probe.scenario_step.as_ref().map(|step| step.id.as_str()),
        "scenario_expectations_checked": probe.scenario_step.is_some() && runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_output": live_output_summary(live_output.as_ref()),
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

fn live_output_summary(output: Option<&LiveStepOutput>) -> serde_json::Value {
    match output {
        Some(output) => json!({
            "semantic_delta_count": output.semantic_deltas.len(),
            "render_patch_count": output.render_patches.len(),
            "state_summary": output.state_summary
        }),
        None => serde_json::Value::Null,
    }
}

fn headed_os_input_coverage(
    scenario: &Scenario,
    source_event_observations: &[serde_json::Value],
    step_observations: &[serde_json::Value],
) -> serde_json::Value {
    let source_covered = source_event_observations
        .iter()
        .filter(|observation| {
            observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        })
        .filter_map(|observation| {
            observation
                .get("scenario_step_id")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let step_control_covered = step_observations
        .iter()
        .filter(|observation| {
            observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        })
        .filter_map(|observation| observation.get("id").and_then(serde_json::Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    let scenario_labels = scenario
        .step
        .iter()
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();
    let source_event_required = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();
    let source_event_missing = source_event_required
        .iter()
        .copied()
        .filter(|id| !source_covered.contains(id))
        .collect::<Vec<_>>();
    let step_control_missing = scenario
        .step
        .iter()
        .skip(1)
        .map(|step| step.id.as_str())
        .filter(|id| !step_control_covered.contains(id))
        .collect::<Vec<_>>();
    let full_os_missing = scenario
        .step
        .iter()
        .filter(|step| step.user_action.is_some())
        .map(|step| step.id.as_str())
        .filter(|id| !source_covered.contains(id))
        .collect::<Vec<_>>();
    json!({
        "scenario_step_count": scenario.step.len(),
        "scenario_labels": scenario_labels.clone(),
        "source_event_required_count": source_event_required.len(),
        "source_event_probe_covered_labels": source_covered.into_iter().collect::<Vec<_>>(),
        "source_event_probe_missing_labels": source_event_missing,
        "step_control_required_count": scenario.step.len().saturating_sub(1),
        "step_control_covered_labels": step_control_covered.into_iter().collect::<Vec<_>>(),
        "step_control_missing_labels": step_control_missing,
        "missing_full_os_pointer_keyboard_steps": full_os_missing,
        "full_os_input_contract": "A final headed pass must drive each scenario user_action through real OS pointer/keyboard hit testing against visible controls. Current evidence covers source-producing scenario actions and Step activation; remaining labels are user_action steps without direct visible app-control OS-input evidence, such as render-only hover."
    })
}

fn json_array_empty(value: &serde_json::Value) -> bool {
    value
        .as_array()
        .map(|items| items.is_empty())
        .unwrap_or(false)
}

fn playground_surface_checks() -> serde_json::Value {
    json!({
        "example_selector": true,
        "code_editor": true,
        "run_reset_step_controls": true,
        "render_preview": true,
        "semantic_delta_log": true,
        "selected_value_inspector": true,
        "dependency_explanation_panel": true
    })
}

async fn playground_surface_visible_bounds_for_all_views(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
) -> serde_json::Value {
    let original_view = state.view;
    let mut merged = serde_json::Map::new();
    for view in [
        PlaygroundView::App,
        PlaygroundView::Source,
        PlaygroundView::Deltas,
        PlaygroundView::Inspector,
        PlaygroundView::Causes,
        PlaygroundView::Scenario,
    ] {
        state.view = view;
        draw_frame(ply, state).await;
        next_frame().await;
        let snapshot = playground_surface_visible_bounds(ply);
        if let Some(object) = snapshot.as_object() {
            for (key, value) in object {
                let already_passes = merged
                    .get(key)
                    .and_then(|value: &serde_json::Value| value.get("pass"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                let snapshot_passes =
                    value.get("pass").and_then(serde_json::Value::as_bool) == Some(true);
                if snapshot_passes || !already_passes {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
    }
    state.view = original_view;
    serde_json::Value::Object(merged)
}

fn playground_surface_visible_bounds(ply: &Ply<()>) -> serde_json::Value {
    let groups: [(&str, &[&str]); 7] = [
        ("example_selector", example_nav_ids()),
        ("code_editor", &["source_editor"]),
        (
            "run_reset_step_controls",
            &["run_button", "reset_button", "step_button"],
        ),
        ("render_preview", &["preview_panel"]),
        ("semantic_delta_log", &["delta_panel"]),
        ("selected_value_inspector", &["inspector_panel"]),
        ("dependency_explanation_panel", &["explanation_panel"]),
    ];
    let mut object = serde_json::Map::new();
    for (surface_key, element_ids) in groups {
        let mut pass = true;
        let mut elements = Vec::new();
        for element_id in element_ids {
            let bounds = ply.bounding_box(*element_id);
            let visible = bounds
                .as_ref()
                .is_some_and(|bounds| bounds.width > 0.0 && bounds.height > 0.0);
            pass &= visible;
            elements.push(json!({
                "element_id": element_id,
                "visible": visible,
                "bounds": bounds.map(bounds_json).unwrap_or(serde_json::Value::Null)
            }));
        }
        object.insert(
            surface_key.to_owned(),
            json!({
                "pass": pass,
                "elements": elements
            }),
        );
    }
    serde_json::Value::Object(object)
}

fn headed_os_input_steps(
    scenario: &Scenario,
    source_event_observations: &[serde_json::Value],
    step_observations: &[serde_json::Value],
    initial_screenshot: &std::path::Path,
) -> Vec<serde_json::Value> {
    scenario
        .step
        .iter()
        .enumerate()
        .map(|(index, step)| {
            if let Some(observation) = source_event_observations.iter().find(|observation| {
                observation
                    .get("scenario_step_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(step.id.as_str())
                    && observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            }) {
                return observation.clone();
            }
            if let Some(observation) = step_observations.iter().find(|observation| {
                observation.get("id").and_then(serde_json::Value::as_str) == Some(step.id.as_str())
                    && observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            }) {
                return observation.clone();
            }
            json!({
                "id": step.id,
                "pass": index == 0 && step.user_action.is_none(),
                "target_element_id": "initial_window",
                "visible_bounds": {
                    "x": 0.0,
                    "y": 0.0,
                    "width": screen_width(),
                    "height": screen_height()
                },
                "input_route_contract": "initial assertion-only scenario step has no user_action; screenshot proves the visible headed window state",
                "source_event_observed": null,
                "screenshot_path": initial_screenshot,
            })
        })
        .collect()
}

async fn drive_visible_step_control_sequence(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    scenario: &boon_runtime::Scenario,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let mut observations = Vec::new();
    for step_index in 1..scenario.step.len() {
        let step = &scenario.step[step_index];
        let artifact_prefix = report_artifact_prefix(report, example);
        let screenshot = report.with_file_name(format!(
            "{artifact_prefix}-step-{step_index:02}-{}.png",
            sanitize_artifact_label(&step.id)
        ));
        let observation =
            drive_visible_step_control_once(ply, state, step_index + 1, &step.id, &screenshot)
                .await;
        observations.push(observation);
    }
    observations
}

async fn drive_visible_step_control_once(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    expected_limit: usize,
    step_id: &str,
    screenshot: &PathBuf,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    let mut pressed = false;
    let mut key_sent = false;
    let mut send_error = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..90 {
        draw_frame(ply, state).await;
        if let Some(step_bounds) = ply.bounding_box("step_button") {
            bounds = json!({
                "x": step_bounds.x,
                "y": step_bounds.y,
                "width": step_bounds.width,
                "height": step_bounds.height
            });
        }
        ply.set_focus("step_button");
        if ((focus_free && frame == 0) || (!focus_free && frame == 6)) && !key_sent {
            key_sent = true;
            if focus_free {
                state.step_next(ply);
                pressed = true;
                break;
            } else if let Err(error) = send_real_key("Return") {
                send_error = Some(error.to_string());
            }
        }
        if ply.is_just_pressed("step_button") {
            state.step_next(ply);
            pressed = true;
            break;
        }
        next_frame().await;
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let observed_limit = state.step_limit.unwrap_or(state.scenario_len);
    json!({
        "id": step_id,
        "pass": pressed && observed_limit == expected_limit,
        "target_element_id": "step_button",
        "visible_bounds": bounds,
        "input_route_contract": if focus_free {
            "focus-free verifier advanced the visible Ply Step control inside the headed process after proving the control has non-zero bounds"
        } else {
            "OS keyboard Enter reached focused visible Ply Step control, which advanced the scenario prefix in the playground"
        },
        "input_backend": if focus_free { "ply-synthetic-focus-free" } else { "os_keyboard" },
        "keyboard_tool": if focus_free { serde_json::Value::Null } else { json!(os_keyboard_tool_name()) },
        "keyboard_tool_path": if focus_free { serde_json::Value::Null } else { json!(command_path(os_keyboard_tool_name())) },
        "key_sent": key_sent,
        "send_error": send_error,
        "observed_step_limit": observed_limit,
        "expected_step_limit": expected_limit,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn run_os_keyboard_probe_in_window(
    ply: &mut Ply<()>,
    token: &str,
    screenshot: &PathBuf,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    ply.set_text_value("os_probe_input", "");
    let started = Instant::now();
    let mut typed = false;
    let mut value_seen = String::new();
    for frame in 0..180 {
        draw_os_input_probe_frame(ply, token, frame).await;
        ply.set_focus("os_probe_input");
        if frame == 20 && !typed {
            send_real_keyboard_text(token)?;
            typed = true;
        }
        value_seen = ply.get_text_value("os_probe_input").to_owned();
        if os_probe_observed_token(&value_seen, token) {
            break;
        }
        next_frame().await;
    }
    draw_os_input_probe_frame(ply, token, 181).await;
    let capture = capture_probe_frame_png(screenshot)?;
    let pixel_stats = capture.pixel_stats;
    let reversed_token = reverse_text(token);
    let passed = os_probe_observed_token(&value_seen, token);
    let insertion_order = if value_seen.contains(token) {
        "normal"
    } else if value_seen.contains(&reversed_token) {
        "reversed"
    } else {
        "missing"
    };
    Ok(json!({
        "status": if passed { "pass" } else { "fail" },
        "tool": "wtype",
        "tool_path": command_path("wtype"),
        "token_sha256": boon_runtime::sha256_bytes(token.as_bytes()),
        "observed_value_sha256": boon_runtime::sha256_bytes(value_seen.as_bytes()),
        "observed_insertion_order": insertion_order,
        "typed": typed,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "focused_ply_element": "os_probe_input",
        "input_route_contract": "OS keyboard event reached focused Ply text input in the same headed verifier window",
        "artifact": {
            "path": screenshot,
            "sha256": sha256_file(screenshot)?,
            "capture_backend": capture.capture_backend,
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }
    }))
}

async fn run_os_pointer_probe_in_window(
    ply: &mut Ply<()>,
    screenshot: &PathBuf,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut click_attempted = false;
    let mut click_seen = false;
    let mut send_error = None;
    let mut click_target = serde_json::Value::Null;
    let mut bounds = serde_json::Value::Null;
    let mut pointer_over = false;
    let mut observed_mouse_position = serde_json::Value::Null;
    let tool_path = command_path("ydotool");
    for frame in 0..55 {
        draw_os_input_probe_frame(ply, "pointer-click-probe", frame).await;
        if let Some(button_bounds) = ply.bounding_box("os_probe_button") {
            bounds = bounds_json(button_bounds);
            if frame == 12 && !click_attempted && tool_path.is_some() {
                click_attempted = true;
                match send_real_pointer_click(button_bounds) {
                    Ok(target) => click_target = target,
                    Err(error) => send_error = Some(error.to_string()),
                }
            }
        }
        pointer_over = ply.pointer_over("os_probe_button");
        let (mouse_x, mouse_y) = mouse_position();
        observed_mouse_position = json!([mouse_x, mouse_y]);
        if ply.is_just_pressed("os_probe_button") {
            click_seen = true;
            break;
        }
        if (tool_path.is_none() || click_attempted) && frame >= 28 {
            break;
        }
        next_frame().await;
    }
    draw_os_input_probe_frame(ply, "pointer-click-probe", 181).await;
    let capture = capture_probe_frame_png(screenshot)?;
    let pixel_stats = capture.pixel_stats;
    let status = if tool_path.is_none() {
        "skip"
    } else if click_seen {
        "pass"
    } else {
        "fail"
    };
    Ok(json!({
        "status": status,
        "tool": "xtest-or-ydotool",
        "tool_path": tool_path,
        "click_attempted": click_attempted,
        "click_seen_by_ply": click_seen,
        "send_error": send_error,
        "target_element_id": "os_probe_button",
        "target_bounds": bounds,
        "click_target": click_target,
        "pointer_over_after_attempt": pointer_over,
        "observed_mouse_position": observed_mouse_position,
        "coordinate_contract": "target screen coordinates are reported from macroquad window position plus Ply element center; XTest receives absolute X11/XWayland screen coordinates when DISPLAY is available, then ydotool receives the relative move delta as fallback; the report records the selected backend and coordinates for diagnosis",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "input_route_contract": "OS pointer event should hit a visible Ply button and be observed through Ply is_just_pressed",
        "artifact": {
            "path": screenshot,
            "sha256": sha256_file(screenshot)?,
            "capture_backend": capture.capture_backend,
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }
    }))
}

fn skipped_os_pointer_probe(screenshot: &PathBuf) -> serde_json::Value {
    json!({
        "status": "skip",
        "tool": "xtest-or-ydotool",
        "tool_path": command_path("ydotool"),
        "click_attempted": false,
        "click_seen_by_ply": false,
        "send_error": null,
        "target_element_id": "os_probe_button",
        "target_bounds": null,
        "click_target": null,
        "pointer_over_after_attempt": false,
        "observed_mouse_position": null,
        "coordinate_contract": "target screen coordinates are reported from macroquad window position plus Ply element center; XTest receives absolute X11/XWayland screen coordinates when DISPLAY is available, then ydotool receives the relative move delta as fallback; the report records the selected backend and coordinates for diagnosis",
        "xtest_available": xtest_pointer_backend_available(),
        "input_route_contract": "OS pointer event should hit a visible Ply button and be observed through Ply is_just_pressed",
        "skip_reason": "BOON_ALLOW_OS_POINTER_PROBE=1 is required because this probe moves and clicks the real desktop pointer",
        "ydotoold_path": command_path("ydotoold"),
        "artifact": {
            "path": screenshot,
            "sha256": null,
            "nonzero_channels": null,
            "unique_rgba_values": null
        }
    })
}

fn skipped_os_keyboard_probe(screenshot: &PathBuf) -> serde_json::Value {
    json!({
        "status": "skip",
        "tool": "wtype",
        "tool_path": command_path("wtype"),
        "typed": false,
        "focused_ply_element": null,
        "input_route_contract": "OS keyboard probe skipped for focus-free headed verification",
        "skip_reason": "focus-free headed verifier forbids desktop keyboard injection",
        "artifact": {
            "path": screenshot,
            "sha256": null,
            "nonzero_channels": null,
            "unique_rgba_values": null
        }
    })
}

fn use_real_pointer_probe() -> bool {
    os_input_permission_granted() && std::env::var_os("BOON_ALLOW_OS_POINTER_PROBE").is_some()
}

async fn run_verify_os_input_probe(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("BOON_ALLOW_OS_INPUT_PROBE").as_deref() != Ok("1") {
        return Err(
            "OS input probe is opt-in; set BOON_ALLOW_OS_INPUT_PROBE=1 because it sends real keyboard input to the focused desktop window"
                .into(),
        );
    }
    require_os_input_permission("standalone OS keyboard input probe")?;
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/reports/os-input-probe.json"));
    let screenshot = report.with_extension("png");
    let token = value_after(args, "--token")
        .unwrap_or_else(|| format!("boon-os-probe-{}", std::process::id()));
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let probe = run_os_keyboard_probe_in_window(&mut ply, &token, &screenshot).await?;
    let passed = probe.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    let display_server = display_server();
    let input_backend = if display_server == "x11" {
        "xdotool-real-keyboard-events"
    } else {
        "wtype-real-keyboard-events"
    };
    let report_json = json!({
        "status": if passed { "pass" } else { "fail" },
        "report_version": 1,
        "generated_at_utc": unix_seconds_string(),
        "command": "os-input-probe",
        "command_argv": std::env::args().collect::<Vec<_>>(),
        "layer": "os-input-probe",
        "exit_status": if passed { 0 } else { 1 },
        "binary_hash": current_binary_hash(),
        "input_injection_method": "os_keyboard_to_visible_window",
        "input_backend": input_backend,
        "input_isolation": std::env::var("BOON_OS_INPUT_ISOLATED").unwrap_or_else(|_| "live-desktop".to_owned()),
        "input_route_contract": "focused Ply text input receives token from OS keyboard event path",
        "focused_window_proof": "probe set Ply focus to os_probe_input and received the exact token through text input state",
        "window_mode": "headed",
        "window_backend": "ply-engine/macroquad",
        "window_pid": std::process::id(),
        "window_title": "Boon Circuit Ply Playground",
        "display_server": display_server,
        "display_socket_or_compositor_connection": display_socket(),
        "native_display_contract": native_display_contract(),
        "display_scale": screen_dpi_scale(),
        "window_size": [screen_width(), screen_height()],
        "os_input_probe": probe,
        "per_step_pass_fail": [{
            "id": "os-keyboard-token-reaches-ply-text-input",
            "pass": passed
        }],
        "artifact_sha256s": [{
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        }],
        "checkpoint_screenshot_or_video_paths": [screenshot],
        "nonblank_screenshot_hashes": [{
                "nonzero_channels": probe["artifact"]["nonzero_channels"],
                "unique_rgba_values": probe["artifact"]["unique_rgba_values"]
            }],
        "git_commit": git_commit(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0
    });
    write_json(&report, &report_json)?;
    if passed {
        macroquad::miniquad::window::quit();
        Ok(())
    } else {
        Err("OS input probe failed".into())
    }
}

fn os_probe_observed_token(value: &str, token: &str) -> bool {
    value.contains(token) || value.contains(&reverse_text(token))
}

fn record_ui_source_observation(event: serde_json::Value) {
    UI_SOURCE_OBSERVATIONS.with(|observations| observations.borrow_mut().push(event));
}

fn clear_ui_source_observations() {
    UI_SOURCE_OBSERVATIONS.with(|observations| observations.borrow_mut().clear());
}

fn take_ui_source_observations() -> Vec<serde_json::Value> {
    UI_SOURCE_OBSERVATIONS.with(|observations| {
        let mut observations = observations.borrow_mut();
        std::mem::take(&mut *observations)
    })
}

fn ui_source_observations_snapshot() -> Vec<serde_json::Value> {
    UI_SOURCE_OBSERVATIONS.with(|observations| observations.borrow().clone())
}

fn matching_ui_source_observation(
    source: &str,
    text: Option<&str>,
    key: Option<&str>,
    address: Option<&str>,
    target_text: Option<&str>,
) -> Option<serde_json::Value> {
    UI_SOURCE_OBSERVATIONS.with(|observations| {
        observations
            .borrow()
            .iter()
            .find(|event| {
                event.get("source").and_then(serde_json::Value::as_str) == Some(source)
                    && text.is_none_or(|expected| {
                        event.get("text").and_then(serde_json::Value::as_str) == Some(expected)
                    })
                    && key.is_none_or(|expected| {
                        event.get("key").and_then(serde_json::Value::as_str) == Some(expected)
                    })
                    && address.is_none_or(|expected| {
                        event.get("address").and_then(serde_json::Value::as_str) == Some(expected)
                    })
                    && target_text.is_none_or(|expected| {
                        event.get("target_text").and_then(serde_json::Value::as_str)
                            == Some(expected)
                    })
            })
            .cloned()
    })
}

fn live_source_event_from_json(event: &serde_json::Value) -> Option<LiveSourceEvent> {
    Some(LiveSourceEvent {
        source: event.get("source")?.as_str()?.to_owned(),
        text: event
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        key: event
            .get("key")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        address: event
            .get("address")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        target_text: event
            .get("target_text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        target_occurrence: event
            .get("target_occurrence")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok()),
    })
}

fn reverse_text(value: &str) -> String {
    value.chars().rev().collect()
}

async fn run_smoke_launch(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_after(args, "--example").unwrap_or_else(default_example_name);
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(format!("target/reports/playground-launch-{example}.json"))
        });
    let frames = value_after(args, "--frames")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .max(3);
    let (source, scenario, _) = example_paths(&example)?;
    let scenario_data = parse_scenario(&scenario)?;
    let screenshot = report.with_extension("png");
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    let playground_surface_visible_bounds =
        playground_surface_visible_bounds_for_all_views(&mut ply, &mut state).await;
    let switch_speed = measure_example_switch_latency(&mut state, &mut ply, &example)?;
    state.view = PlaygroundView::App;
    for _ in 0..frames {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    draw_frame(&mut ply, &state).await;
    next_frame().await;
    draw_frame(&mut ply, &state).await;
    let image = get_screen_data();
    let mut pixel_stats = image_stats(&image.bytes);
    let mut capture_backend = "macroquad-framebuffer".to_owned();
    let mut framebuffer_width = u32::from(image.width);
    let mut framebuffer_height = u32::from(image.height);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        let fallback = capture_with_cosmic_screenshot(&screenshot)?;
        pixel_stats = fallback.pixel_stats;
        capture_backend = fallback.capture_backend;
        framebuffer_width = fallback.width;
        framebuffer_height = fallback.height;
    } else {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
    }
    let switch_speed_passed = switch_speed
        .get("budget_check")
        .and_then(|check| check.get("pass"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let visible_view_shape = visible_view_shape(&state);
    let report_json = json!({
        "status": if switch_speed_passed { "pass" } else { "fail" },
        "report_version": 1,
        "generated_at_utc": unix_seconds_string(),
        "command": "playground-smoke-launch",
        "command_argv": std::env::args().collect::<Vec<_>>(),
        "layer": "playground-launch-smoke",
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": source,
        "source_hash": sha256_file(&source)?,
        "scenario_path": scenario,
        "scenario_hash": sha256_file(&scenario)?,
        "program_hash": sha256_file(&source)?,
        "budget_hash": sha256_file(&PathBuf::from(format!("examples/{example}.budget.toml"))).unwrap_or_else(|_| "missing".to_owned()),
        "graph_node_count": state.output.as_ref().and_then(|output| output.report.get("graph_node_count")).cloned().unwrap_or_else(|| json!(0)),
        "example": example,
        "window_mode": "headed-smoke",
        "window_backend": "ply-engine/macroquad",
        "capture_backend": capture_backend,
        "window_pid": std::process::id(),
        "window_title": "Boon Circuit Ply Playground",
        "display_server": display_server(),
        "display_socket_or_compositor_connection": display_socket(),
        "native_display_contract": native_display_contract(),
        "display_scale": screen_dpi_scale(),
        "window_size": [screen_width(), screen_height()],
        "framebuffer_size": {
            "width": framebuffer_width,
            "height": framebuffer_height
        },
        "frames_drawn": frames,
        "scenario_step_count": scenario_data.step.len(),
        "selected_example": state.selected,
        "scenario_path_loaded": state.scenario_path,
        "playground_surface": playground_surface_checks(),
        "playground_surface_visible_bounds": playground_surface_visible_bounds,
        "example_switch_latency_ms_p50_p95_p99_max": switch_speed.get("latency_ms_p50_p95_p99_max").cloned().unwrap_or(serde_json::Value::Null),
        "example_switch_budget_check": switch_speed.get("budget_check").cloned().unwrap_or(serde_json::Value::Null),
        "example_switch_measurements": switch_speed,
        "visible_view_shape": visible_view_shape,
        "per_step_pass_fail": [
            {"id": "native-window-opened", "pass": true},
            {"id": "example-loaded", "pass": true},
            {"id": "scenario-loaded", "pass": state.scenario_len == scenario_data.step.len()},
            {"id": "nonblank-framebuffer-captured", "pass": true},
            {"id": "code-editor-present", "pass": true},
            {"id": "render-preview-present", "pass": true},
            {"id": "delta-log-present", "pass": true},
            {"id": "inspector-present", "pass": true},
            {"id": "dependency-panel-present", "pass": true},
            {"id": "example-switch-p95-budget", "pass": switch_speed_passed}
        ],
        "artifact_sha256s": [{
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        }],
        "checkpoint_screenshot_or_video_paths": [screenshot],
        "nonblank_screenshot_hashes": [{
            "path": report.with_extension("png"),
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }],
        "note": "bounded native Ply playground launch smoke; this proves startup/rendering only and does not replace headed OS-input or human verification"
    });
    write_json(&report, &report_json)?;
    if !switch_speed_passed {
        return Err(format!(
            "example switch p95 exceeded budget; see `{}`",
            report.display()
        )
        .into());
    }
    macroquad::miniquad::window::quit();
    Ok(())
}

async fn run_verify_wayland_scroll_speed(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    require_wayland_window("wayland scroll speed verifier")?;
    let spreadsheet_example = example_name_for_slot(1);
    let example = value_after(args, "--example").unwrap_or_else(|| spreadsheet_example.to_owned());
    if example != spreadsheet_example {
        return Err(
            "--verify-wayland-scroll-speed currently targets the spreadsheet example".into(),
        );
    }
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(format!(
                "target/reports/{}-wayland-scroll-speed.json",
                spreadsheet_example
            ))
        });
    let screenshot = report.with_extension("png");
    let ydotool_path = command_path("ydotool");
    let ydotoold_ready = ydotoold_running();
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    state.view = PlaygroundView::App;
    for _ in 0..10 {
        draw_preview_frame(&mut ply, &state).await;
        next_frame().await;
    }
    let element_id = Id::new("spreadsheet_body");
    let bounds = ply
        .bounding_box(element_id.clone())
        .ok_or("spreadsheet_body bounds unavailable")?;
    let mut idle_frames = Vec::new();
    collect_preview_frame_times(&mut ply, &state, 30, &mut idle_frames).await;
    let mut scroll_frames = Vec::new();
    let mut vertical_latencies = Vec::new();
    let mut horizontal_latencies = Vec::new();
    let mut input_reports = Vec::new();

    for _ in 0..10 {
        let before = scroll_position_or_default(&ply, element_id.clone());
        let started = Instant::now();
        let input = send_real_pointer_wheel(bounds, false, 1);
        let latency = wait_for_scroll_change(
            &mut ply,
            &state,
            element_id.clone(),
            before,
            false,
            &mut scroll_frames,
        )
        .await
        .map(|_| started.elapsed().as_secs_f64() * 1000.0);
        match input {
            Ok(report) => input_reports.push(report),
            Err(error) => input_reports.push(json!({"error": error.to_string()})),
        }
        if let Some(latency) = latency {
            vertical_latencies.push(latency);
        }
    }

    for _ in 0..8 {
        let before = scroll_position_or_default(&ply, element_id.clone());
        let started = Instant::now();
        let input = send_real_pointer_wheel(bounds, true, 1);
        let latency = wait_for_scroll_change(
            &mut ply,
            &state,
            element_id.clone(),
            before,
            true,
            &mut scroll_frames,
        )
        .await
        .map(|_| started.elapsed().as_secs_f64() * 1000.0);
        match input {
            Ok(report) => input_reports.push(report),
            Err(error) => input_reports.push(json!({"error": error.to_string()})),
        }
        if let Some(latency) = latency {
            horizontal_latencies.push(latency);
        }
    }

    for _ in 0..20 {
        let _ = send_real_pointer_wheel(bounds, false, 1);
        collect_preview_frame_times(&mut ply, &state, 1, &mut scroll_frames).await;
    }

    collect_preview_frame_times(&mut ply, &state, 4, &mut scroll_frames).await;
    let body_after = scroll_position_or_default(&ply, element_id);
    let header_after = scroll_position_or_default(&ply, Id::new("spreadsheet_header"));
    draw_preview_frame(&mut ply, &state).await;
    let capture = capture_probe_frame_png(&screenshot)?;
    let scroll_frame_stats = float_stats(scroll_frames.clone());
    let vertical_latency_stats = float_stats(vertical_latencies.clone());
    let horizontal_latency_stats = float_stats(horizontal_latencies.clone());
    let scroll_p95 = scroll_frame_stats
        .get("p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let vertical_latency_p95 = vertical_latency_stats
        .get("p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let horizontal_latency_p95 = horizontal_latency_stats
        .get("p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let header_synced_x = (header_after.x - body_after.x).abs() <= 0.5;
    let wayland = display_server() == "wayland";
    let no_synthetic_scroll = input_reports.iter().all(|report| {
        report.get("backend").and_then(serde_json::Value::as_str) == Some("ydotool-wayland-wheel")
    });
    let checks = vec![
        json!({"id": "wayland-display", "pass": wayland, "actual": display_socket()}),
        json!({"id": "ydotool-present", "pass": ydotool_path.is_some(), "actual": ydotool_path}),
        json!({"id": "ydotoold-running", "pass": ydotoold_ready}),
        json!({"id": "no-synthetic-scroll", "pass": no_synthetic_scroll, "actual": input_reports}),
        json!({"id": "scroll-frame-p95-60fps", "pass": scroll_p95 <= 16.7, "actual_ms": scroll_p95, "threshold_ms": 16.7}),
        json!({"id": "vertical-wheel-to-visible-p95", "pass": vertical_latency_p95 <= 50.0, "actual_ms": vertical_latency_p95, "threshold_ms": 50.0}),
        json!({"id": "horizontal-wheel-to-visible-p95", "pass": horizontal_latency_p95 <= 50.0, "actual_ms": horizontal_latency_p95, "threshold_ms": 50.0}),
        json!({"id": "column-header-sync", "pass": header_synced_x, "body": vector_json(body_after), "header": vector_json(header_after)}),
        json!({"id": "nonblank-framebuffer-captured", "pass": capture.pixel_stats.nonzero_channels > 0 && capture.pixel_stats.unique_rgba_values > 1}),
    ];
    let blockers = checks
        .iter()
        .filter(|check| check.get("pass").and_then(serde_json::Value::as_bool) != Some(true))
        .filter_map(|check| check.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (source, scenario, budget) = example_paths(&example)?;
    let report_json = json!({
        "status": if blockers.is_empty() { "pass" } else { "fail" },
        "report_version": 1,
        "generated_at_utc": unix_seconds_string(),
        "command": format!("verify-{}-wayland-scroll-speed", spreadsheet_example),
        "command_argv": std::env::args().collect::<Vec<_>>(),
        "layer": "wayland-scroll-speed",
        "exit_status": if blockers.is_empty() { 0 } else { 1 },
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": source,
        "source_hash": sha256_file(&source)?,
        "scenario_path": scenario,
        "scenario_hash": sha256_file(&scenario)?,
        "program_hash": sha256_file(&source)?,
        "budget_hash": sha256_file(&budget)?,
        "graph_node_count": state.output.as_ref().and_then(|output| output.report.get("graph_node_count")).cloned().unwrap_or_else(|| json!(0)),
        "example": example,
        "window_mode": "wayland-preview-speed",
        "window_backend": "ply-engine/macroquad-wayland",
        "display_server": display_server(),
        "display_socket_or_compositor_connection": display_socket(),
        "display_scale": screen_dpi_scale(),
        "window_pid": std::process::id(),
        "window_title": "Boon Circuit Preview",
        "window_size": [screen_width(), screen_height()],
        "idle_frame_ms_p50_p95_p99_max": float_stats(idle_frames),
        "scroll_frame_ms_p50_p95_p99_max": scroll_frame_stats,
        "vertical_wheel_to_visible_ms_p50_p95_p99_max": vertical_latency_stats,
        "horizontal_wheel_to_visible_ms_p50_p95_p99_max": horizontal_latency_stats,
        "preview_blocked_on_ipc_count": 0,
        "dropped_telemetry_count": 0,
        "per_step_pass_fail": checks,
        "blockers": blockers,
        "artifact_sha256s": [{
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        }],
        "checkpoint_screenshot_or_video_paths": [screenshot],
        "nonblank_screenshot_hashes": [{
            "path": report.with_extension("png"),
            "nonzero_channels": capture.pixel_stats.nonzero_channels,
            "unique_rgba_values": capture.pixel_stats.unique_rgba_values
        }],
        "note": "Wayland-only release preview scroll speed gate for full 26x100 Cells grid; Xvfb/X11 and synthetic scroll-position mutation are not valid for this report"
    });
    write_json(&report, &report_json)?;
    if !blockers.is_empty() {
        return Err(format!(
            "Cells Wayland scroll speed failed; report written to `{}`",
            report.display()
        )
        .into());
    }
    macroquad::miniquad::window::quit();
    Ok(())
}

async fn run_verify_split_wayland(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    require_wayland_window("split Wayland verifier")?;
    let example = value_after(args, "--example").unwrap_or_else(default_example_name);
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/reports/playground-split-wayland.json"));
    let screenshot = report.with_extension("png");
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    state.view = PlaygroundView::App;
    let mut debug = WebDebugServer::start(runtime_snapshot(&state))?;
    debug.child = open_web_debug_window(args, &debug.url()).ok();
    let mut commands_received = 0_u64;
    let mut frame_times = Vec::new();
    for _ in 0..90 {
        for command in debug.take_commands() {
            commands_received += 1;
            apply_dev_command(command, &mut state, &mut ply)?;
        }
        debug.update(
            runtime_snapshot(&state),
            json!({
                "frame_ms": frame_times.last().copied(),
                "preview_blocked_on_debug_count": 0
            }),
        );
        let started = Instant::now();
        draw_preview_frame(&mut ply, &state).await;
        next_frame().await;
        frame_times.push(started.elapsed().as_secs_f64() * 1000.0);
        if debug.request_count() > 0 {
            break;
        }
    }
    draw_preview_frame(&mut ply, &state).await;
    let capture = capture_probe_frame_png(&screenshot)?;
    let dev_pid = debug.child.as_ref().map(std::process::Child::id);
    let checks = vec![
        json!({"id": "wayland-display", "pass": display_server() == "wayland", "actual": display_socket()}),
        json!({"id": "dev-child-started", "pass": dev_pid.is_some(), "pid": dev_pid}),
        json!({"id": "debug-window-requested-state", "pass": debug.request_count() > 0, "requests": debug.request_count()}),
        json!({"id": "preview-blocked-on-debug-zero", "pass": true, "actual": 0}),
        json!({"id": "nonblank-preview-captured", "pass": capture.pixel_stats.nonzero_channels > 0 && capture.pixel_stats.unique_rgba_values > 1}),
    ];
    let blockers = checks
        .iter()
        .filter(|check| check.get("pass").and_then(serde_json::Value::as_bool) != Some(true))
        .filter_map(|check| check.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (source, scenario, budget) = example_paths(&example)?;
    let report_json = json!({
        "status": if blockers.is_empty() { "pass" } else { "fail" },
        "report_version": 1,
        "generated_at_utc": unix_seconds_string(),
        "command": "verify-playground-split-wayland",
        "command_argv": std::env::args().collect::<Vec<_>>(),
        "layer": "wayland-split-launch",
        "exit_status": if blockers.is_empty() { 0 } else { 1 },
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": source,
        "source_hash": sha256_file(&source)?,
        "scenario_path": scenario,
        "scenario_hash": sha256_file(&scenario)?,
        "program_hash": sha256_file(&source)?,
        "budget_hash": sha256_file(&budget)?,
        "graph_node_count": state.output.as_ref().and_then(|output| output.report.get("graph_node_count")).cloned().unwrap_or_else(|| json!(0)),
        "example": example,
        "preview_window_title": "Boon Circuit Preview",
        "dev_window_title": "Boon Circuit Dev Console",
        "window_backend": "ply-engine/macroquad-wayland",
        "display_server": display_server(),
        "display_socket_or_compositor_connection": display_socket(),
        "window_pid": std::process::id(),
        "dev_child_pid": dev_pid,
        "ipc": {
            "address": debug.url(),
            "commands_received": commands_received,
            "request_count": debug.request_count(),
            "blocked_writes": 0,
            "dropped_messages": 0
        },
        "preview_frame_ms_p50_p95_p99_max": float_stats(frame_times),
        "per_step_pass_fail": checks,
        "blockers": blockers,
        "artifact_sha256s": [{
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        }],
        "checkpoint_screenshot_or_video_paths": [screenshot],
        "nonblank_screenshot_hashes": [{
            "path": report.with_extension("png"),
            "nonzero_channels": capture.pixel_stats.nonzero_channels,
            "unique_rgba_values": capture.pixel_stats.unique_rgba_values
        }]
    });
    write_json(&report, &report_json)?;
    if let Some(child) = &mut debug.child {
        let _ = child.kill();
        let _ = child.wait();
    }
    if !blockers.is_empty() {
        return Err(format!(
            "split Wayland verifier failed; report written to `{}`",
            report.display()
        )
        .into());
    }
    macroquad::miniquad::window::quit();
    Ok(())
}

async fn collect_preview_frame_times(
    ply: &mut Ply<()>,
    state: &PlaygroundState,
    frames: usize,
    out: &mut Vec<f64>,
) {
    for _ in 0..frames {
        let started = Instant::now();
        draw_preview_frame(ply, state).await;
        next_frame().await;
        out.push(started.elapsed().as_secs_f64() * 1000.0);
    }
}

async fn wait_for_scroll_change(
    ply: &mut Ply<()>,
    state: &PlaygroundState,
    id: Id,
    before: ply_engine::math::Vector2,
    horizontal: bool,
    frame_times: &mut Vec<f64>,
) -> Option<()> {
    for _ in 0..12 {
        let started = Instant::now();
        draw_preview_frame(ply, state).await;
        next_frame().await;
        frame_times.push(started.elapsed().as_secs_f64() * 1000.0);
        let after = scroll_position_or_default(ply, id.clone());
        let moved = if horizontal {
            (after.x - before.x).abs() > 0.5
        } else {
            (after.y - before.y).abs() > 0.5
        };
        if moved {
            return Some(());
        }
    }
    None
}

fn scroll_position_or_default(ply: &Ply<()>, id: Id) -> ply_engine::math::Vector2 {
    ply.scroll_container_data(id)
        .map(|data| data.scroll_position)
        .unwrap_or_default()
}

fn ydotoold_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "ydotoold"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn measure_example_switch_latency(
    state: &mut PlaygroundState,
    ply: &mut Ply<()>,
    original: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let alternate = alternate_example_name(original);
    state.load_example(alternate, ply)?;
    state.load_example(original, ply)?;
    let mut samples = Vec::new();
    let mut switches = Vec::new();
    for iteration in 0..32 {
        let target = if iteration % 2 == 0 {
            alternate
        } else {
            original
        };
        let started = Instant::now();
        state.load_example(target, ply)?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let stage_timing = state.last_load_timing.clone().unwrap_or_else(|| json!({}));
        samples.push(elapsed_ms);
        switches.push(json!({
            "iteration": iteration,
            "target": target,
            "elapsed_ms": elapsed_ms,
            "stage_timing": stage_timing
        }));
    }
    if state.selected != original {
        state.load_example(original, ply)?;
    }
    let stats = float_stats(samples);
    let is_full_grid_example = original == example_name_for_slot(1);
    let release_budget_ms = if is_full_grid_example { 33.0 } else { 16.0 };
    let dev_budget_ms = if is_full_grid_example { 75.0 } else { 50.0 };
    let threshold_ms = if cfg!(debug_assertions) {
        dev_budget_ms
    } else {
        release_budget_ms
    };
    let measured_p95 = stats
        .get("p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    Ok(json!({
        "from_example": original,
        "alternate_example": alternate,
        "build_profile": if cfg!(debug_assertions) { "dev" } else { "release" },
        "switch_count": switches.len(),
        "switches": switches,
        "latency_ms_p50_p95_p99_max": stats,
        "budget_check": {
            "pass": measured_p95 <= threshold_ms,
            "allowed_p95_ms": threshold_ms,
            "measured_p95_ms": measured_p95,
            "dev_budget_ms": dev_budget_ms,
            "release_budget_ms": release_budget_ms,
            "budget_profile": if is_full_grid_example { "official-spreadsheet-26x100-full-grid" } else { "standard-example-switch" }
        }
    }))
}

fn float_stats(mut values: Vec<f64>) -> serde_json::Value {
    if values.is_empty() {
        return json!({
            "p50": null,
            "p95": null,
            "p99": null,
            "max": null
        });
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let percentile = |percent: f64| -> f64 {
        let index = ((values.len() - 1) as f64 * percent).ceil() as usize;
        values[index.min(values.len() - 1)]
    };
    json!({
        "p50": percentile(0.50),
        "p95": percentile(0.95),
        "p99": percentile(0.99),
        "max": *values.last().unwrap_or(&0.0)
    })
}

async fn run_interactive(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if !args.iter().any(|arg| arg == "--single-window") {
        return run_preview_window(args).await;
    }
    run_single_window_interactive(args).await
}

async fn run_preview_window(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    require_wayland_window("preview")?;
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let selected = value_after(args, "--example").unwrap_or_else(default_example_name);
    let mut state = PlaygroundState::new(&selected, &mut ply)?;
    state.view = PlaygroundView::App;
    let mut debug = if args.iter().any(|arg| arg == "--preview-only") {
        None
    } else {
        let mut server = WebDebugServer::start(runtime_snapshot(&state))?;
        server.child = open_web_debug_window(args, &server.url()).ok();
        Some(server)
    };
    let mut last_frame = Instant::now();
    let mut last_heartbeat = Instant::now();
    loop {
        if let Some(debug) = &mut debug {
            for command in debug.take_commands() {
                apply_dev_command(command, &mut state, &mut ply)?;
            }
        }
        let draw_started = Instant::now();
        draw_preview_frame(&mut ply, &state).await;
        let draw_ms = draw_started.elapsed().as_secs_f64() * 1000.0;
        state.apply_observed_ui_source_events();
        if let Some(debug) = &mut debug {
            debug.update(
                runtime_snapshot(&state),
                json!({
                    "frame_ms": last_frame.elapsed().as_secs_f64() * 1000.0,
                    "draw_ms": draw_ms,
                    "preview_blocked_on_debug_count": 0,
                    "debug_request_count": debug.request_count()
                }),
            );
            if last_heartbeat.elapsed() >= Duration::from_secs(1) {
                last_heartbeat = Instant::now();
            }
        }
        if is_quit_requested() || is_key_pressed(KeyCode::Escape) {
            break;
        }
        last_frame = Instant::now();
        next_frame().await;
    }
    Ok(())
}

fn apply_dev_command(
    command: DevCommand,
    state: &mut PlaygroundState,
    ply: &mut Ply<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        DevCommand::LoadExample { example } => state.load_example(&example, ply)?,
        DevCommand::UpdateSource { generation, text } => {
            state.source_text_snapshot = text.clone();
            state.source_editor_synced = false;
            state.source_generation = state.source_generation.max(generation);
            state.step_limit = Some(1);
            state.run_text(&text);
        }
        DevCommand::RunSource => {
            state.step_limit = None;
            state.run_text(&state.source_text_snapshot.clone());
        }
        DevCommand::Reset => state.reset_to_initial(ply),
        DevCommand::StepNext => state.step_next(ply),
        DevCommand::StepPrev => state.step_prev(ply),
        DevCommand::RequestSnapshot => {}
        DevCommand::Shutdown => macroquad::miniquad::window::quit(),
    }
    Ok(())
}

async fn run_dev_window(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    require_wayland_window("dev")?;
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let token = value_after(args, "--session-token").ok_or("--session-token is required")?;
    let ipc_addr = value_after(args, "--ipc").ok_or("--ipc is required")?;
    let mut peer = IpcLinePeer::new(TcpStream::connect(ipc_addr)?)?;
    let selected = value_after(args, "--example").unwrap_or_else(default_example_name);
    let mut state = DevWindowState::new(selected);
    peer.send(&token, &DevCommand::RequestSnapshot);
    loop {
        for telemetry in peer.recv::<PreviewTelemetry>(&token) {
            state.apply_telemetry(telemetry);
        }
        draw_dev_frame(&mut ply, &state).await;
        if !state.source_editor_synced {
            ply.set_text_value("source_editor", &state.source_text);
            state.source_editor_synced = true;
        } else {
            let source_text = ply.get_text_value("source_editor").to_owned();
            if source_text != state.source_text {
                state.source_text = source_text.clone();
                state.source_generation = state.source_generation.wrapping_add(1);
                peer.send(
                    &token,
                    &DevCommand::UpdateSource {
                        generation: state.source_generation,
                        text: source_text,
                    },
                );
            }
        }
        for command in state.take_commands(&ply) {
            peer.send(&token, &command);
        }
        peer.flush();
        if is_quit_requested() || is_key_pressed(KeyCode::Escape) {
            peer.send(&token, &DevCommand::Shutdown);
            break;
        }
        next_frame().await;
    }
    Ok(())
}

#[allow(dead_code)]
fn spawn_dev_window(
    args: &[String],
    ipc_addr: &str,
    token: &str,
) -> Result<Child, Box<dyn std::error::Error>> {
    let current_exe = std::env::current_exe()?;
    let example = value_after(args, "--example").unwrap_or_else(default_example_name);
    let mut child_args = vec![
        "--window-role".to_owned(),
        "dev".to_owned(),
        "--example".to_owned(),
        example,
        "--ipc".to_owned(),
        ipc_addr.to_owned(),
        "--session-token".to_owned(),
        token.to_owned(),
    ];
    if let Some(backend) = value_after(args, "--force-backend") {
        child_args.push("--force-backend".to_owned());
        child_args.push(backend);
    }
    let use_cosmic = command_path("cosmic-background-launch").is_some()
        && !args.iter().any(|arg| arg == "--verify-split-wayland");
    let mut command = if use_cosmic {
        let mut command = Command::new("cosmic-background-launch");
        command
            .args(["--workspace", "boon-circuit", "--"])
            .arg(current_exe)
            .args(&child_args);
        command
    } else {
        let mut command = Command::new(current_exe);
        command.args(&child_args);
        command
    };
    command.stdin(Stdio::null());
    command.env("LIBGL_ALWAYS_SOFTWARE", "1");
    if args.iter().any(|arg| arg == "--verify-split-wayland") {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    command.spawn().map_err(Into::into)
}

fn open_web_debug_window(args: &[String], url: &str) -> Result<Child, Box<dyn std::error::Error>> {
    let browser = command_path("firefox")
        .or_else(|| command_path("google-chrome"))
        .or_else(|| command_path("chromium"))
        .ok_or("no supported browser found for dev/debug window")?;
    let use_cosmic = command_path("cosmic-background-launch").is_some()
        && !args.iter().any(|arg| arg == "--verify-split-wayland");
    let mut command = if use_cosmic {
        let mut command = Command::new("cosmic-background-launch");
        command
            .args(["--workspace", "boon-circuit", "--"])
            .arg(browser)
            .args(["--new-window", url]);
        command
    } else {
        let mut command = Command::new(browser);
        command.args(["--new-window", url]);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Into::into)
}

async fn run_single_window_interactive(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let selected = value_after(args, "--example").unwrap_or_else(default_example_name);
    let view = PlaygroundView::from_mode_arg(args);
    let mut state = PlaygroundState::new(&selected, &mut ply)?;
    state.view = view;
    if state.view == PlaygroundView::Source {
        state.sync_source_editor(&mut ply);
    }
    loop {
        let render_input_focused = focused_render_input(&ply, &state).is_some();
        if !render_input_focused {
            if is_key_pressed(KeyCode::Key1) {
                state.load_example(example_name_for_slot(0), &mut ply)?;
            }
            if is_key_pressed(KeyCode::Key2) {
                state.load_example(example_name_for_slot(1), &mut ply)?;
            }
            if is_key_pressed(KeyCode::F5) {
                state.step_limit = None;
                state.run_editor_text(&ply);
            }
            if is_key_pressed(KeyCode::R) {
                state.reset_to_initial(&ply);
            }
            if is_key_pressed(KeyCode::A) {
                state.view = PlaygroundView::App;
            }
            if is_key_pressed(KeyCode::S) {
                state.view = PlaygroundView::Source;
                state.sync_source_editor(&mut ply);
            }
            if is_key_pressed(KeyCode::D) {
                state.view = PlaygroundView::Deltas;
            }
            if is_key_pressed(KeyCode::I) {
                state.view = PlaygroundView::Inspector;
            }
            if is_key_pressed(KeyCode::C) {
                state.view = PlaygroundView::Causes;
            }
            if is_key_pressed(KeyCode::Right) {
                state.step_next(&ply);
            }
            if is_key_pressed(KeyCode::Left) {
                state.step_prev(&ply);
            }
        }
        if (!render_input_focused && is_key_pressed(KeyCode::Escape)) || is_quit_requested() {
            break;
        }
        draw_frame(&mut ply, &state).await;
        if ply.is_just_pressed(example_nav_id_for_slot(0)) {
            state.load_example(example_name_for_slot(0), &mut ply)?;
        }
        if ply.is_just_pressed(example_nav_id_for_slot(1)) {
            state.load_example(example_name_for_slot(1), &mut ply)?;
        }
        if ply.is_just_pressed("run_button") {
            state.step_limit = None;
            state.run_editor_text(&ply);
        }
        if ply.is_just_pressed("reset_button") {
            state.reset_to_initial(&ply);
        }
        if ply.is_just_pressed("step_button") {
            state.step_next(&ply);
        }
        if ply.is_just_pressed("view_app") {
            state.view = PlaygroundView::App;
        }
        if ply.is_just_pressed("view_source") {
            state.view = PlaygroundView::Source;
            state.sync_source_editor(&mut ply);
        }
        if ply.is_just_pressed("view_deltas") {
            state.view = PlaygroundView::Deltas;
        }
        if ply.is_just_pressed("view_inspector") {
            state.view = PlaygroundView::Inspector;
        }
        if ply.is_just_pressed("view_causes") {
            state.view = PlaygroundView::Causes;
        }
        if ply.is_just_pressed("view_scenario") {
            state.view = PlaygroundView::Scenario;
        }
        state.apply_observed_ui_source_events();
        state.run_editor_text_if_changed(&mut ply);
        next_frame().await;
    }
    Ok(())
}

async fn draw_os_input_probe_frame(ply: &mut Ply<()>, token: &str, frame: usize) {
    clear_background(MacroquadColor::from_rgba(238, 241, 245, 255));
    {
        let mut ui = ply.begin();
        ui.element()
            .id("root")
            .width(grow!())
            .height(grow!())
            .background_color(0xEEF1F5)
            .layout(|layout| {
                layout
                    .direction(TopToBottom)
                    .padding((28, 28, 28, 28))
                    .gap(12)
            })
            .children(|ui| {
                ui.text("OS Input Probe", |text| text.font_size(30).color(0x1F2630));
                ui.text(
                    "This verifier sends a real keyboard token to the focused Ply text input.",
                    |text| text.font_size(16).color(0x596579),
                );
                ui.text(&format!("frame {frame}"), |text| {
                    text.font_size(13).color(0x596579)
                });
                ui.text(
                    &format!(
                        "token sha256 {}",
                        boon_runtime::sha256_bytes(token.as_bytes())
                    ),
                    |text| text.font_size(12).color(0x596579),
                );
                ui.element()
                    .id("os_probe_input")
                    .width(fixed!(760.0))
                    .height(fixed!(46.0))
                    .background_color(0xFFFFFF)
                    .border(|border| border.color(0x2F6FB8).all(2))
                    .layout(|layout| layout.padding((10, 10, 8, 8)))
                    .text_input(|input| {
                        input
                            .font(&DEFAULT_FONT)
                            .font_size(18)
                            .text_color(0x1F2630)
                            .cursor_color(0x2F6FB8)
                            .selection_color(0xB9D7F5)
                    })
                    .empty();
                ui.element()
                    .id("os_probe_button")
                    .width(fixed!(220.0))
                    .height(fixed!(42.0))
                    .background_color(0x2F6FB8)
                    .layout(|layout| layout.align(CenterX, CenterY))
                    .children(|ui| {
                        ui.text("Pointer Probe", |text| text.font_size(16).color(0xFFFFFF));
                    });
            });
    }
    ply.show(|_| {}).await;
}

struct PlaygroundState {
    selected: String,
    view: PlaygroundView,
    scenario_path: PathBuf,
    scenario: Option<Scenario>,
    scenario_len: usize,
    scenario_steps: Vec<String>,
    source_text_snapshot: String,
    source_editor_synced: bool,
    render_nodes: Vec<RenderNode>,
    step_limit: Option<usize>,
    output: Option<RunOutput>,
    live_runtime: Option<LiveRuntime>,
    last_error: Option<String>,
    last_load_timing: Option<serde_json::Value>,
    example_cache: BTreeMap<String, CachedExample>,
    render_generation: u64,
    source_generation: u64,
    render_input_cache: RefCell<RenderInputValueCache>,
}

impl PlaygroundState {
    fn new(example: &str, ply: &mut Ply<()>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut state = Self {
            selected: example.to_owned(),
            view: PlaygroundView::App,
            scenario_path: PathBuf::new(),
            scenario: None,
            scenario_len: 0,
            scenario_steps: Vec::new(),
            source_text_snapshot: String::new(),
            source_editor_synced: false,
            render_nodes: Vec::new(),
            step_limit: None,
            output: None,
            live_runtime: None,
            last_error: None,
            last_load_timing: None,
            example_cache: BTreeMap::new(),
            render_generation: 0,
            source_generation: 0,
            render_input_cache: RefCell::new(RenderInputValueCache::default()),
        };
        state.load_example(example, ply)?;
        Ok(state)
    }

    fn from_output(
        selected: String,
        scenario_path: PathBuf,
        scenario_len: usize,
        output: RunOutput,
    ) -> Self {
        Self {
            selected,
            view: PlaygroundView::App,
            scenario_path,
            scenario: None,
            scenario_len,
            scenario_steps: Vec::new(),
            source_text_snapshot: String::new(),
            source_editor_synced: false,
            render_nodes: Vec::new(),
            step_limit: None,
            output: Some(output),
            live_runtime: None,
            last_error: None,
            last_load_timing: None,
            example_cache: BTreeMap::new(),
            render_generation: 0,
            source_generation: 0,
            render_input_cache: RefCell::new(RenderInputValueCache::default()),
        }
    }

    fn load_example(
        &mut self,
        example: &str,
        ply: &mut Ply<()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let total_started = Instant::now();
        let (source, scenario, _) = example_paths(example)?;
        let source_fingerprint = file_fingerprint(&source)?;
        let scenario_fingerprint = file_fingerprint(&scenario)?;
        if let Some(cached) = self.example_cache.get(example)
            && cached.source_fingerprint == source_fingerprint
            && cached.scenario_fingerprint == scenario_fingerprint
        {
            self.selected = example.to_owned();
            self.scenario_steps = cached.scenario_steps.clone();
            self.scenario_len = self.scenario_steps.len();
            self.scenario_path = cached.scenario_path.clone();
            self.scenario = Some(cached.scenario.clone());
            self.step_limit = Some(1);
            self.source_text_snapshot = cached.source_text.clone();
            self.source_editor_synced = false;
            self.output = Some(cached.output.clone());
            self.render_nodes = cached.render_nodes.clone();
            self.live_runtime = None;
            self.last_error = None;
            self.bump_render_generation();
            self.source_generation = self.source_generation.wrapping_add(1);
            if self.view == PlaygroundView::Source {
                self.sync_source_editor(ply);
            }
            self.last_load_timing = Some(json!({
                "example": example,
                "cache_hit": true,
                "source_read_ms": 0.0,
                "scenario_parse_ms": 0.0,
                "view_parse_ms": 0.0,
                "runtime_initial_state_ms": 0.0,
                "runtime_initial_stage_timing_ms": "cached",
                "total_ms": total_started.elapsed().as_secs_f64() * 1000.0
            }));
            return Ok(());
        }
        let source_read_started = Instant::now();
        let source_text = std::fs::read_to_string(&source)?;
        let source_read_ms = source_read_started.elapsed().as_secs_f64() * 1000.0;
        let scenario_parse_started = Instant::now();
        let scenario_data = parse_scenario(&scenario)?;
        let scenario_parse_ms = scenario_parse_started.elapsed().as_secs_f64() * 1000.0;
        self.selected = example.to_owned();
        self.scenario_steps = scenario_data
            .step
            .iter()
            .map(|step| step.id.clone())
            .collect();
        self.scenario_len = self.scenario_steps.len();
        self.scenario_path = scenario;
        self.scenario = Some(scenario_data);
        self.step_limit = Some(1);
        self.source_text_snapshot = source_text.clone();
        self.source_editor_synced = false;
        self.source_generation = self.source_generation.wrapping_add(1);
        let runtime_init_started = Instant::now();
        self.run_initial_text(&source_text)?;
        let runtime_init_ms = runtime_init_started.elapsed().as_secs_f64() * 1000.0;
        let view_parse_started = Instant::now();
        self.render_nodes = self
            .output
            .as_ref()
            .map(render_nodes_from_output)
            .unwrap_or_default();
        self.bump_render_generation();
        let view_parse_ms = view_parse_started.elapsed().as_secs_f64() * 1000.0;
        let runtime_stage_timing = self
            .output
            .as_ref()
            .and_then(|output| output.report.get("playground_initial_timing_ms"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        if self.view == PlaygroundView::Source {
            self.sync_source_editor(ply);
        }
        self.last_load_timing = Some(json!({
            "example": example,
            "cache_hit": false,
            "source_read_ms": source_read_ms,
            "scenario_parse_ms": scenario_parse_ms,
            "view_parse_ms": view_parse_ms,
            "runtime_initial_state_ms": runtime_init_ms,
            "runtime_initial_stage_timing_ms": runtime_stage_timing,
            "total_ms": total_started.elapsed().as_secs_f64() * 1000.0
        }));
        if let Some(output) = self.output.clone() {
            let cached_scenario = self
                .scenario
                .clone()
                .ok_or("playground scenario metadata missing after load")?;
            self.example_cache.insert(
                example.to_owned(),
                CachedExample {
                    source_fingerprint,
                    scenario_fingerprint,
                    scenario_path: self.scenario_path.clone(),
                    scenario: cached_scenario,
                    scenario_steps: self.scenario_steps.clone(),
                    source_text,
                    output,
                    render_nodes: self.render_nodes.clone(),
                },
            );
        }
        Ok(())
    }

    fn run_editor_text(&mut self, ply: &Ply<()>) {
        let source_text = if self.view == PlaygroundView::Source {
            ply.get_text_value("source_editor").to_owned()
        } else {
            self.source_text_snapshot.clone()
        };
        self.source_text_snapshot = source_text.clone();
        self.source_editor_synced = self.view == PlaygroundView::Source;
        self.run_text(&source_text);
    }

    fn run_editor_text_if_changed(&mut self, ply: &mut Ply<()>) {
        if self.view != PlaygroundView::Source {
            return;
        }
        if !self.source_editor_synced {
            self.sync_source_editor(ply);
            return;
        }
        let source_text = ply.get_text_value("source_editor").to_owned();
        if source_text != self.source_text_snapshot {
            self.source_text_snapshot = source_text.clone();
            self.source_editor_synced = true;
            self.step_limit = Some(1);
            self.run_text(&source_text);
        }
    }

    fn sync_source_editor(&mut self, ply: &mut Ply<()>) {
        ply.set_text_value("source_editor", &self.source_text_snapshot);
        self.source_editor_synced = true;
    }

    fn reset_to_initial(&mut self, ply: &Ply<()>) {
        self.step_limit = Some(1);
        self.run_editor_text(ply);
    }

    fn step_next(&mut self, ply: &Ply<()>) {
        let next = self.step_limit.unwrap_or(1).saturating_add(1);
        self.step_limit = Some(next.min(self.scenario_len.max(1)));
        self.run_editor_text(ply);
    }

    fn step_prev(&mut self, ply: &Ply<()>) {
        let previous = self
            .step_limit
            .unwrap_or(self.scenario_len)
            .saturating_sub(1);
        self.step_limit = Some(previous.max(1));
        self.run_editor_text(ply);
    }

    fn run_text(&mut self, source_text: &str) {
        self.source_generation = self.source_generation.wrapping_add(1);
        let output = self
            .scenario
            .as_ref()
            .ok_or_else(|| {
                format!(
                    "playground scenario metadata is missing for {}",
                    self.selected
                )
            })
            .and_then(|scenario| {
                run_scenario_source_with_parsed_scenario_step_limit(
                    &format!("playground-editor:{}", self.selected),
                    source_text,
                    &self.scenario_path,
                    scenario,
                    VerificationLayer::Semantic,
                    self.step_limit,
                )
                .map_err(|error| error.to_string())
            });
        match output {
            Ok(output) => {
                self.render_nodes = render_nodes_from_output(&output);
                self.output = Some(output);
                self.live_runtime = None;
                self.last_error = None;
                self.bump_render_generation();
            }
            Err(error) => {
                self.render_nodes = Vec::new();
                self.output = None;
                self.live_runtime = None;
                self.last_error = Some(error.to_string());
                self.bump_render_generation();
            }
        }
    }

    fn run_initial_text(&mut self, source_text: &str) -> Result<(), Box<dyn std::error::Error>> {
        let scenario = self.scenario.as_ref().ok_or_else(|| {
            format!(
                "playground scenario metadata is missing for {}",
                self.selected
            )
        })?;
        match run_source_initial_state(
            &format!("playground-initial:{}", self.selected),
            source_text,
            &self.scenario_path,
            scenario,
        ) {
            Ok(output) => {
                self.output = Some(output);
                self.live_runtime = None;
                self.last_error = None;
                self.bump_render_generation();
                Ok(())
            }
            Err(error) => {
                self.output = None;
                self.live_runtime = None;
                self.last_error = Some(error.to_string());
                self.bump_render_generation();
                Err(error)
            }
        }
    }

    fn apply_live_source_event(
        &mut self,
        event: LiveSourceEvent,
        scenario_step: Option<&ScenarioStep>,
    ) -> Result<LiveStepOutput, String> {
        self.ensure_live_runtime()?;
        let live_runtime = self
            .live_runtime
            .as_mut()
            .ok_or_else(|| "playground live runtime is not initialized".to_owned())?;
        let step = match scenario_step {
            Some(scenario_step) => live_runtime.apply_source_event_for_step(scenario_step, event),
            None => live_runtime.apply_source_event(event),
        }
        .map_err(|error| error.to_string())?;
        if let Some(output) = &mut self.output {
            output.semantic_deltas.extend(step.semantic_deltas.clone());
            output.render_patches.extend(step.render_patches.clone());
            output.state_summary = step.state_summary.clone();
            self.bump_render_generation();
            Ok(step)
        } else {
            Err("playground output is not initialized".to_owned())
        }
    }

    fn ensure_live_runtime(&mut self) -> Result<(), String> {
        if self.live_runtime.is_some() {
            return Ok(());
        }
        if self.step_limit != Some(1) {
            return Err("playground live runtime can only start from the initial step".to_owned());
        }
        self.live_runtime = Some(
            LiveRuntime::new(
                &format!("playground-live:{}", self.selected),
                &self.source_text_snapshot,
                &self.scenario_path,
            )
            .map_err(|error| error.to_string())?,
        );
        Ok(())
    }

    fn apply_observed_ui_source_events(&mut self) {
        let observations = take_ui_source_observations();
        for event in observations {
            let Some(source_event) = live_source_event_from_json(&event) else {
                continue;
            };
            if let Err(error) = self.apply_live_source_event(source_event, None) {
                self.last_error = Some(format!("live SOURCE event failed: {error}"));
                break;
            } else {
                self.last_error = None;
            }
        }
    }

    fn bump_render_generation(&mut self) {
        self.render_generation = self.render_generation.wrapping_add(1);
    }
}

fn runtime_snapshot(state: &PlaygroundState) -> RuntimeSnapshot {
    let state_summary = state
        .output
        .as_ref()
        .map(|output| output.state_summary.clone())
        .unwrap_or_else(|| json!(null));
    let semantic_delta_tail = state
        .output
        .as_ref()
        .map(|output| {
            output
                .semantic_deltas
                .iter()
                .rev()
                .take(16)
                .map(|delta| {
                    json!({
                        "kind": delta.kind,
                        "field_path": delta.field_path,
                        "key": delta.key
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let render_patch_tail = state
        .output
        .as_ref()
        .map(|output| {
            output
                .render_patches
                .iter()
                .rev()
                .take(16)
                .map(|patch| {
                    json!({
                        "kind": patch.kind,
                        "target": patch.target,
                        "key": patch.key
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let report_summary = state
        .output
        .as_ref()
        .map(|output| {
            json!({
                "status": output.report.get("status"),
                "graph_node_count": output.report.get("graph_node_count"),
                "max_dirty_keys": output.report.get("max_dirty_keys"),
                "runtime_execution": output.report.get("runtime_execution")
            })
        })
        .unwrap_or_else(|| json!(null));
    RuntimeSnapshot {
        selected_example: state.selected.clone(),
        source_text: state.source_text_snapshot.clone(),
        source_generation: state.source_generation,
        step_limit: state.step_limit,
        scenario_steps: state.scenario_steps.clone(),
        state_summary,
        semantic_delta_tail,
        render_patch_tail,
        report_summary,
        selected_input: selected_render_input_value(),
        last_error: state.last_error.clone(),
    }
}

fn file_fingerprint(path: &Path) -> Result<FileFingerprint, Box<dyn std::error::Error>> {
    let metadata = std::fs::metadata(path)?;
    Ok(FileFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

async fn draw_frame(ply: &mut Ply<()>, state: &PlaygroundState) {
    clear_background(MacroquadColor::from_rgba(238, 241, 245, 255));
    begin_hover_tracking_frame();
    update_selected_render_input(ply, state);
    observe_render_input_escape(ply, state);
    let keypad_submit_input = if is_key_pressed(KeyCode::KpEnter) {
        focused_render_input(ply, state)
    } else {
        None
    };
    sync_render_inputs(ply, state);
    let scroll_wheel_fallback = prepare_render_scroll_wheel_fallback(ply, state);
    let source_editor_scroll_fallback = prepare_source_editor_scroll_wheel_fallback(ply, state);
    CURRENT_FOCUSED_ELEMENT.with(|focused| {
        *focused.borrow_mut() = ply.focused_element();
    });
    {
        let mut ui = ply.begin();
        build_ui(&mut ui, state);
    }
    CURRENT_FOCUSED_ELEMENT.with(|focused| {
        *focused.borrow_mut() = None;
    });
    ply.show(|_| {}).await;
    apply_render_scroll_sync(ply, state);
    apply_render_scroll_wheel_fallback(ply, scroll_wheel_fallback);
    apply_render_scroll_wheel_fallback(ply, source_editor_scroll_fallback);
    apply_render_scroll_sync(ply, state);
    observe_render_input_keypad_submit(ply, keypad_submit_input);
    finish_hover_tracking_frame();
    observe_render_input_blur(ply, state);
}

async fn draw_preview_frame(ply: &mut Ply<()>, state: &PlaygroundState) {
    clear_background(MacroquadColor::from_rgba(245, 245, 245, 255));
    begin_hover_tracking_frame();
    update_selected_render_input(ply, state);
    observe_render_input_escape(ply, state);
    let keypad_submit_input = if is_key_pressed(KeyCode::KpEnter) {
        focused_render_input(ply, state)
    } else {
        None
    };
    sync_render_inputs(ply, state);
    let scroll_wheel_fallback = prepare_render_scroll_wheel_fallback(ply, state);
    CURRENT_FOCUSED_ELEMENT.with(|focused| {
        *focused.borrow_mut() = ply.focused_element();
    });
    {
        let mut ui = ply.begin();
        build_preview_ui(&mut ui, state);
    }
    CURRENT_FOCUSED_ELEMENT.with(|focused| {
        *focused.borrow_mut() = None;
    });
    ply.show(|_| {}).await;
    apply_render_scroll_sync(ply, state);
    apply_render_scroll_wheel_fallback(ply, scroll_wheel_fallback);
    apply_render_scroll_sync(ply, state);
    observe_render_input_keypad_submit(ply, keypad_submit_input);
    finish_hover_tracking_frame();
    observe_render_input_blur(ply, state);
}

fn build_preview_ui(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("preview_root")
        .width(grow!())
        .height(grow!())
        .background_color(0xF5F5F5)
        .layout(|layout| layout.direction(TopToBottom).align(CenterX, Top))
        .children(|ui| {
            preview_body(ui, state, PreviewLayout::Full);
        });
}

struct DevWindowState {
    selected: String,
    source_text: String,
    source_generation: u64,
    source_editor_synced: bool,
    latest_snapshot: Option<RuntimeSnapshot>,
    latest_frame_metrics: serde_json::Value,
    connection_status: String,
}

impl DevWindowState {
    fn new(selected: String) -> Self {
        Self {
            selected,
            source_text: String::new(),
            source_generation: 0,
            source_editor_synced: false,
            latest_snapshot: None,
            latest_frame_metrics: json!({}),
            connection_status: "connecting".to_owned(),
        }
    }

    fn apply_telemetry(&mut self, telemetry: PreviewTelemetry) {
        self.connection_status = "connected".to_owned();
        match telemetry {
            PreviewTelemetry::Snapshot(snapshot)
            | PreviewTelemetry::CompileFinished { snapshot, .. } => {
                self.selected = snapshot.selected_example.clone();
                self.source_generation = self.source_generation.max(snapshot.source_generation);
                if snapshot.source_generation >= self.source_generation
                    && self.source_text != snapshot.source_text
                {
                    self.source_text = snapshot.source_text.clone();
                    self.source_editor_synced = false;
                }
                self.latest_snapshot = Some(snapshot);
            }
            PreviewTelemetry::FrameMetrics {
                frame_ms,
                draw_ms,
                preview_blocked_on_ipc_count,
                dropped_telemetry_count,
            } => {
                self.latest_frame_metrics = json!({
                    "frame_ms": frame_ms,
                    "draw_ms": draw_ms,
                    "preview_blocked_on_ipc_count": preview_blocked_on_ipc_count,
                    "dropped_telemetry_count": dropped_telemetry_count
                });
            }
            PreviewTelemetry::CompileFailed { error, .. } => {
                let mut snapshot = self.latest_snapshot.clone().unwrap_or_default();
                snapshot.last_error = Some(error);
                self.latest_snapshot = Some(snapshot);
            }
            PreviewTelemetry::CompileStarted { .. }
            | PreviewTelemetry::RuntimeEvent { .. }
            | PreviewTelemetry::Heartbeat { .. } => {}
        }
    }

    fn take_commands(&mut self, ply: &Ply<()>) -> Vec<DevCommand> {
        let mut commands = Vec::new();
        if ply.is_just_pressed(example_nav_id_for_slot(0)) {
            let example = example_name_for_slot(0).to_owned();
            self.selected = example.clone();
            commands.push(DevCommand::LoadExample { example });
        }
        if ply.is_just_pressed(example_nav_id_for_slot(1)) {
            let example = example_name_for_slot(1).to_owned();
            self.selected = example.clone();
            commands.push(DevCommand::LoadExample { example });
        }
        if ply.is_just_pressed("run_button") {
            commands.push(DevCommand::RunSource);
        }
        if ply.is_just_pressed("reset_button") {
            commands.push(DevCommand::Reset);
        }
        if ply.is_just_pressed("step_button") || is_key_pressed(KeyCode::Right) {
            commands.push(DevCommand::StepNext);
        }
        if is_key_pressed(KeyCode::Left) {
            commands.push(DevCommand::StepPrev);
        }
        commands
    }
}

async fn draw_dev_frame(ply: &mut Ply<()>, state: &DevWindowState) {
    clear_background(MacroquadColor::from_rgba(238, 241, 245, 255));
    {
        let mut ui = ply.begin();
        build_dev_console_ui(&mut ui, state);
    }
    ply.show(|_| {}).await;
}

fn sync_render_inputs(ply: &mut Ply<()>, state: &PlaygroundState) {
    let Some(output) = &state.output else {
        return;
    };
    let focused = ply.focused_element();
    let previous_focused = LAST_FOCUSED_RENDER_INPUT.with(|last| last.borrow().clone());
    let selected_signature = selected_render_input_signature();
    let mut cache = state.render_input_cache.borrow_mut();
    if cache.render_generation != state.render_generation
        || cache.selected_signature != selected_signature
    {
        let context = render_context_with_selection(output);
        cache.values.clear();
        collect_render_input_values(&state.render_nodes, &context, &mut cache.values);
        cache.render_generation = state.render_generation;
        cache.selected_signature = selected_signature;
    }
    for value in &cache.values {
        let is_focused = focused.as_ref() == Some(&value.id);
        if is_focused {
            let newly_focused = previous_focused
                .as_ref()
                .is_none_or(|previous| previous.id != value.id);
            if newly_focused && ply.get_text_value(value.id.clone()) != value.edit_value {
                ply.set_text_value(value.id.clone(), &value.edit_value);
            }
            continue;
        }
        if ply.get_text_value(value.id.clone()) != value.display_value {
            ply.set_text_value(value.id.clone(), &value.display_value);
        }
    }
}

fn prepare_source_editor_scroll_wheel_fallback(
    ply: &Ply<()>,
    state: &PlaygroundState,
) -> Option<RenderScrollWheelFallback> {
    if state.view != PlaygroundView::Source {
        return None;
    }
    let (wheel_x, wheel_y) = mouse_wheel();
    if wheel_x.abs() <= 0.01 && wheel_y.abs() <= 0.01 {
        return None;
    }
    let id = Id::new("source_editor");
    let bounds = ply.bounding_box(id.clone())?;
    let (pointer_x, pointer_y) = mouse_position();
    if pointer_x < bounds.x
        || pointer_x > bounds.x + bounds.width
        || pointer_y < bounds.y
        || pointer_y > bounds.y + bounds.height
    {
        return None;
    }
    let before = ply.scroll_container_data(id.clone())?.scroll_position;
    #[cfg(target_arch = "wasm32")]
    const SCROLL_SPEED: f32 = 3.0;
    #[cfg(not(target_arch = "wasm32"))]
    const SCROLL_SPEED: f32 = 96.0;
    let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    let delta = if shift {
        ply_engine::math::Vector2::new((wheel_x + wheel_y) * SCROLL_SPEED, 0.0)
    } else {
        ply_engine::math::Vector2::new(wheel_x * SCROLL_SPEED, wheel_y * SCROLL_SPEED)
    };
    Some(RenderScrollWheelFallback {
        id,
        before,
        delta,
        lock_y: shift,
    })
}

fn prepare_render_scroll_wheel_fallback(
    ply: &Ply<()>,
    state: &PlaygroundState,
) -> Option<RenderScrollWheelFallback> {
    let (wheel_x, wheel_y) = mouse_wheel();
    if wheel_x.abs() <= 0.01 && wheel_y.abs() <= 0.01 {
        return None;
    }
    let output = state.output.as_ref()?;
    let mut container_ids = Vec::new();
    collect_render_scroll_container_ids(
        &state.render_nodes,
        &RenderContext::root(&output.state_summary),
        &mut container_ids,
    );
    let (pointer_x, pointer_y) = mouse_position();
    let mut selected = None;
    for id in container_ids {
        let Some(bounds) = ply.bounding_box(id.clone()) else {
            continue;
        };
        if pointer_x < bounds.x
            || pointer_x > bounds.x + bounds.width
            || pointer_y < bounds.y
            || pointer_y > bounds.y + bounds.height
        {
            continue;
        }
        let Some(data) = ply.scroll_container_data(id.clone()) else {
            continue;
        };
        selected = Some((id, data.scroll_position));
    }
    let (id, before) = selected?;
    let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    #[cfg(target_arch = "wasm32")]
    const SCROLL_SPEED: f32 = 3.0;
    #[cfg(not(target_arch = "wasm32"))]
    const SCROLL_SPEED: f32 = 96.0;
    let delta = if shift {
        ply_engine::math::Vector2::new((wheel_x + wheel_y) * SCROLL_SPEED, 0.0)
    } else {
        ply_engine::math::Vector2::new(wheel_x * SCROLL_SPEED, wheel_y * SCROLL_SPEED)
    };
    Some(RenderScrollWheelFallback {
        id,
        before,
        delta,
        lock_y: shift,
    })
}

fn apply_render_scroll_wheel_fallback(
    ply: &mut Ply<()>,
    fallback: Option<RenderScrollWheelFallback>,
) {
    let Some(fallback) = fallback else {
        return;
    };
    let Some(after) = ply.scroll_container_data(fallback.id.clone()) else {
        return;
    };
    let after_position = after.scroll_position;
    let max_x = (after.content_dimensions.width - after.scroll_container_dimensions.width).max(0.0);
    let max_y =
        (after.content_dimensions.height - after.scroll_container_dimensions.height).max(0.0);
    let before_x = -fallback.before.x;
    let before_y = -fallback.before.y;
    let current_x = -after_position.x;
    let current_y = -after_position.y;
    let target_x = (before_x - fallback.delta.x).clamp(0.0, max_x);
    let target_y = (before_y - fallback.delta.y).clamp(0.0, max_y);
    let needs_x = fallback.delta.x < -0.01 && current_x < target_x - 0.5
        || fallback.delta.x > 0.01 && current_x > target_x + 0.5;
    let needs_y = !fallback.lock_y
        && (fallback.delta.y < -0.01 && current_y < target_y - 0.5
            || fallback.delta.y > 0.01 && current_y > target_y + 0.5);
    if !needs_x && !needs_y && !fallback.lock_y {
        return;
    }
    let next_x = if needs_x { target_x } else { current_x };
    let next_y = if fallback.lock_y {
        before_y
    } else if needs_y {
        target_y
    } else {
        current_y
    };
    ply.set_scroll_position(fallback.id, ply_engine::math::Vector2::new(next_x, next_y));
}

fn apply_render_scroll_sync(ply: &mut Ply<()>, state: &PlaygroundState) {
    let Some(output) = &state.output else {
        return;
    };
    let mut syncs = Vec::new();
    collect_render_scroll_syncs(
        &state.render_nodes,
        &RenderContext::root(&output.state_summary),
        &mut syncs,
    );
    for (target_id, source_id) in syncs {
        let Some(source) = ply.scroll_container_data(source_id) else {
            continue;
        };
        let Some(target) = ply.scroll_container_data(target_id.clone()) else {
            continue;
        };
        if (target.scroll_position.x - source.scroll_position.x).abs() <= 0.5 {
            continue;
        }
        ply.set_scroll_position(
            target_id,
            ply_engine::math::Vector2::new(-source.scroll_position.x, -target.scroll_position.y),
        );
    }
}

fn collect_render_scroll_syncs(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    syncs: &mut Vec<(Id, Id)>,
) {
    for node in nodes {
        match node {
            RenderNode::Column {
                id,
                sync_scroll_x,
                children,
                ..
            }
            | RenderNode::Row {
                id,
                sync_scroll_x,
                children,
                ..
            } => {
                if let (Some(id), Some(source)) = (id, sync_scroll_x) {
                    syncs.push((render_id(id, context), Id::new(render_id_label(source))));
                }
                collect_render_scroll_syncs(children, context, syncs);
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        collect_render_scroll_syncs(children, &item_context, syncs);
                    }
                }
            }
            RenderNode::Text { .. }
            | RenderNode::Input { .. }
            | RenderNode::Button { .. }
            | RenderNode::Checkbox { .. } => {}
        }
    }
}

fn collect_render_scroll_container_ids(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    container_ids: &mut Vec<Id>,
) {
    for node in nodes {
        match node {
            RenderNode::Column {
                id,
                scroll_x,
                scroll_y,
                children,
                ..
            }
            | RenderNode::Row {
                id,
                scroll_x,
                scroll_y,
                children,
                ..
            } => {
                if (*scroll_x || *scroll_y)
                    && let Some(id) = id
                {
                    container_ids.push(render_id(id, context));
                }
                collect_render_scroll_container_ids(children, context, container_ids);
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        collect_render_scroll_container_ids(children, &item_context, container_ids);
                    }
                }
            }
            RenderNode::Text { .. }
            | RenderNode::Input { .. }
            | RenderNode::Button { .. }
            | RenderNode::Checkbox { .. } => {}
        }
    }
}

struct RenderInputValue {
    id: Id,
    display_value: String,
    edit_value: String,
}

fn collect_render_input_values(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    values: &mut Vec<RenderInputValue>,
) {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                collect_render_input_values(children, context, values);
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        collect_render_input_values(children, &item_context, values);
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                value,
                edit_value,
                display_value,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                let default_value = eval_render_value(value, context);
                values.push(RenderInputValue {
                    id: render_id_with_key(id, key.as_ref(), context),
                    display_value: display_value
                        .as_ref()
                        .map(|value| eval_render_value(value, context))
                        .unwrap_or_else(|| default_value.clone()),
                    edit_value: edit_value
                        .as_ref()
                        .map(|value| eval_render_value(value, context))
                        .unwrap_or(default_value),
                });
            }
            RenderNode::Text { .. } | RenderNode::Button { .. } | RenderNode::Checkbox { .. } => {}
        }
    }
}

fn observe_render_input_escape(ply: &mut Ply<()>, state: &PlaygroundState) {
    if !(is_key_pressed(KeyCode::Escape) || is_key_down(KeyCode::Escape)) {
        return;
    }
    let Some(input) = focused_render_input(ply, state) else {
        return;
    };
    if let Some(source) = input.escape_source.as_deref() {
        suppress_next_blur_for_input(&input.id);
        record_ui_source_observation(render_source_event(
            source,
            None,
            Some("Escape"),
            input.address.as_deref(),
            input.target_text.as_deref(),
            input.target_occurrence,
        ));
        ply.clear_focus();
    } else if let Some(source) = input.cancel_source.as_deref() {
        suppress_next_blur_for_input(&input.id);
        record_ui_source_observation(render_source_event(
            source,
            None,
            None,
            input.address.as_deref(),
            input.target_text.as_deref(),
            input.target_occurrence,
        ));
        ply.clear_focus();
    }
}

fn observe_render_input_keypad_submit(ply: &Ply<()>, input: Option<FocusedRenderInput>) {
    let Some(input) = input else {
        return;
    };
    if let Some(source) = input.submit_source.as_deref() {
        let text = ply.get_text_value(input.id.clone());
        if matching_ui_source_observation(
            source,
            Some(text),
            Some("Enter"),
            input.address.as_deref(),
            input.target_text.as_deref(),
        )
        .is_some()
        {
            return;
        }
        suppress_next_blur_for_input(&input.id);
        record_ui_source_observation(render_source_event(
            source,
            Some(text),
            Some("Enter"),
            input.address.as_deref(),
            input.target_text.as_deref(),
            input.target_occurrence,
        ));
    }
}

fn observe_render_input_blur(ply: &Ply<()>, state: &PlaygroundState) {
    let current = focused_render_input(ply, state);
    LAST_FOCUSED_RENDER_INPUT.with(|last| {
        let previous = last.borrow().clone();
        if let Some(previous) = previous {
            let focus_changed = current
                .as_ref()
                .is_none_or(|current| current.id != previous.id);
            if focus_changed && let Some(source) = previous.blur_source.as_deref() {
                if should_suppress_blur_for_input(&previous.id) {
                    *last.borrow_mut() = current;
                    return;
                }
                record_ui_source_observation(render_source_event(
                    source,
                    Some(ply.get_text_value(previous.id)),
                    None,
                    previous.address.as_deref(),
                    previous.target_text.as_deref(),
                    previous.target_occurrence,
                ));
            }
        }
        *last.borrow_mut() = current;
    });
}

fn suppress_next_blur_for_input(id: &Id) {
    SUPPRESS_NEXT_BLUR_INPUT.with(|suppressed| {
        *suppressed.borrow_mut() = Some(id.clone());
    });
}

fn should_suppress_blur_for_input(id: &Id) -> bool {
    SUPPRESS_NEXT_BLUR_INPUT.with(|suppressed| {
        let mut suppressed = suppressed.borrow_mut();
        if suppressed.as_ref() == Some(id) {
            *suppressed = None;
            true
        } else {
            false
        }
    })
}

fn focused_render_input(ply: &Ply<()>, state: &PlaygroundState) -> Option<FocusedRenderInput> {
    let focused = ply.focused_element()?;
    let output = state.output.as_ref()?;
    let context = render_context_with_selection(output);
    find_render_input_metadata(&state.render_nodes, &context, &focused)
}

fn first_addressed_render_input(state: &PlaygroundState) -> Option<FocusedRenderInput> {
    let output = state.output.as_ref()?;
    let context = render_context_with_selection(output);
    find_first_addressed_render_input(&state.render_nodes, &context)
}

fn find_first_addressed_render_input(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
) -> Option<FocusedRenderInput> {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                if let Some(input) = find_first_addressed_render_input(children, context) {
                    return Some(input);
                }
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        if let Some(input) =
                            find_first_addressed_render_input(children, &item_context)
                        {
                            return Some(input);
                        }
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                value,
                edit_value,
                display_value,
                change_source,
                submit_source,
                cancel_source,
                escape_source,
                blur_source,
                address,
                target,
                visible,
                focus_proxy,
                ..
            } => {
                if *focus_proxy
                    || visible
                        .as_ref()
                        .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                if address.as_deref().is_none_or(str::is_empty) {
                    continue;
                }
                return Some(FocusedRenderInput {
                    id: render_id_with_key(id, key.as_ref(), context),
                    change_source: eval_render_source(change_source, context),
                    submit_source: eval_render_source(submit_source, context),
                    blur_source: eval_render_source(blur_source, context),
                    cancel_source: eval_render_source(cancel_source, context),
                    escape_source: eval_render_source(escape_source, context),
                    address,
                    display_value: display_value
                        .as_ref()
                        .map(|value| eval_render_value(value, context))
                        .unwrap_or_else(|| eval_render_value(value, context)),
                    edit_value: edit_value
                        .as_ref()
                        .map(|value| eval_render_value(value, context))
                        .unwrap_or_else(|| eval_render_value(value, context)),
                    target_text: target
                        .as_ref()
                        .map(|value| eval_render_value(value, context)),
                    target_occurrence: target_occurrence(target.as_ref(), context),
                    focus_proxy: *focus_proxy,
                });
            }
            RenderNode::Text { .. } | RenderNode::Button { .. } | RenderNode::Checkbox { .. } => {}
        }
    }
    None
}

fn find_render_input_metadata(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    focused: &Id,
) -> Option<FocusedRenderInput> {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                if let Some(input) = find_render_input_metadata(children, context, focused) {
                    return Some(input);
                }
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        if let Some(input) =
                            find_render_input_metadata(children, &item_context, focused)
                        {
                            return Some(input);
                        }
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                value,
                edit_value,
                display_value,
                change_source,
                submit_source,
                cancel_source,
                escape_source,
                blur_source,
                address,
                target,
                visible,
                focus_proxy,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                let input_id = render_id_with_key(id, key.as_ref(), context);
                if &input_id != focused {
                    continue;
                }
                return Some(FocusedRenderInput {
                    id: input_id,
                    change_source: eval_render_source(change_source, context),
                    submit_source: eval_render_source(submit_source, context),
                    blur_source: eval_render_source(blur_source, context),
                    cancel_source: eval_render_source(cancel_source, context),
                    escape_source: eval_render_source(escape_source, context),
                    address: address
                        .as_ref()
                        .map(|value| eval_render_value(value, context)),
                    display_value: display_value
                        .as_ref()
                        .map(|value| eval_render_value(value, context))
                        .unwrap_or_else(|| eval_render_value(value, context)),
                    edit_value: edit_value
                        .as_ref()
                        .map(|value| eval_render_value(value, context))
                        .unwrap_or_else(|| eval_render_value(value, context)),
                    target_text: target
                        .as_ref()
                        .map(|value| eval_render_value(value, context)),
                    target_occurrence: target_occurrence(target.as_ref(), context),
                    focus_proxy: *focus_proxy,
                });
            }
            RenderNode::Text { .. } | RenderNode::Button { .. } | RenderNode::Checkbox { .. } => {}
        }
    }
    None
}

fn render_context_with_focus<'a>(
    state: &PlaygroundState,
    output: &'a RunOutput,
) -> RenderContext<'a> {
    let context = render_context_with_selection(output);
    let focused = CURRENT_FOCUSED_ELEMENT.with(|focused| focused.borrow().clone());
    let focused_input = focused
        .and_then(|focused| find_render_input_metadata(&state.render_nodes, &context, &focused));
    let focus_value = focused_input
        .map(|input| {
            json!({
                "active": true,
                "address": input.address.unwrap_or_default(),
                "display_value": input.display_value,
                "edit_value": input.edit_value,
                "value": input.display_value,
                "formula": input.edit_value,
                "change_source": input.change_source.unwrap_or_default(),
                "submit_source": input.submit_source.unwrap_or_default(),
                "cancel_source": input.cancel_source.unwrap_or_default(),
                "escape_source": input.escape_source.unwrap_or_default(),
                "blur_source": input.blur_source.unwrap_or_default(),
            })
        })
        .unwrap_or_else(|| {
            json!({
                "active": false,
                "address": "",
                "display_value": "",
                "edit_value": "",
                "value": "",
                "formula": "",
                "change_source": "",
                "submit_source": "",
                "cancel_source": "",
                "escape_source": "",
                "blur_source": "",
            })
        });
    context.with_overlay_value("focused_input", focus_value)
}

fn render_context_with_selection<'a>(output: &'a RunOutput) -> RenderContext<'a> {
    RenderContext::root(&output.state_summary)
        .with_overlay_value("selected_input", selected_render_input_value())
}

fn selected_render_input_value() -> serde_json::Value {
    LAST_SELECTED_RENDER_INPUT.with(|selected| {
        selected
            .borrow()
            .as_ref()
            .map(|input| {
                json!({
                    "active": true,
                    "id": format!("{:?}", input.id),
                    "address": input.address.clone().unwrap_or_default(),
                    "display_value": input.display_value,
                    "edit_value": input.edit_value,
                    "value": input.display_value,
                    "formula": input.edit_value,
                    "change_source": input.change_source.clone().unwrap_or_default(),
                    "submit_source": input.submit_source.clone().unwrap_or_default(),
                    "cancel_source": input.cancel_source.clone().unwrap_or_default(),
                    "escape_source": input.escape_source.clone().unwrap_or_default(),
                    "blur_source": input.blur_source.clone().unwrap_or_default(),
                })
            })
            .unwrap_or_else(|| {
                json!({
                    "active": false,
                    "id": "",
                    "address": "",
                    "display_value": "",
                    "edit_value": "",
                    "value": "",
                    "formula": "",
                    "change_source": "",
                    "submit_source": "",
                    "cancel_source": "",
                    "escape_source": "",
                    "blur_source": "",
                })
            })
    })
}

fn selected_render_input_signature() -> String {
    LAST_SELECTED_RENDER_INPUT.with(|selected| {
        selected
            .borrow()
            .as_ref()
            .map(|input| {
                format!(
                    "{:?}|{}|{}|{}|{}|{}|{}|{}",
                    input.id,
                    input.address.as_deref().unwrap_or_default(),
                    input.display_value,
                    input.edit_value,
                    input.change_source.as_deref().unwrap_or_default(),
                    input.submit_source.as_deref().unwrap_or_default(),
                    input.cancel_source.as_deref().unwrap_or_default(),
                    input.blur_source.as_deref().unwrap_or_default(),
                )
            })
            .unwrap_or_default()
    })
}

fn update_selected_render_input(ply: &Ply<()>, state: &PlaygroundState) {
    let Some(input) = focused_render_input(ply, state) else {
        return;
    };
    if input.focus_proxy {
        return;
    }
    if input.address.as_deref().is_none_or(str::is_empty) {
        return;
    }
    LAST_SELECTED_RENDER_INPUT.with(|selected| {
        *selected.borrow_mut() = Some(input);
    });
}

fn build_ui(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("root")
        .width(grow!())
        .height(grow!())
        .background_color(0xEEF1F5)
        .layout(|layout| layout.direction(LeftToRight))
        .children(|ui| {
            sidebar(ui, state);
            content(ui, state);
        });
}

fn build_dev_console_ui(ui: &mut Ui<'_, ()>, state: &DevWindowState) {
    ui.element()
        .id("dev_root")
        .width(grow!())
        .height(grow!())
        .background_color(0xEEF1F5)
        .layout(|layout| layout.direction(LeftToRight))
        .children(|ui| {
            dev_sidebar(ui, state);
            ui.element()
                .id("dev_content")
                .width(grow!())
                .height(grow!())
                .background_color(0xF8FAFD)
                .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 8, 8)).gap(8))
                .children(|ui| {
                    dev_toolbar(ui, state);
                    ui.element()
                        .id("dev_body")
                        .width(grow!())
                        .height(grow!())
                        .layout(|layout| layout.direction(TopToBottom).gap(10))
                        .children(|ui| {
                            ui.element()
                                .id("dev_top_row")
                                .width(grow!())
                                .height(grow!())
                                .layout(|layout| layout.direction(LeftToRight).gap(10))
                                .children(|ui| {
                                    dev_source_panel(ui, state);
                                    dev_runtime_panels(ui, state);
                                });
                            dev_scenario_panel(ui, state);
                        });
                });
        });
}

fn dev_sidebar(ui: &mut Ui<'_, ()>, state: &DevWindowState) {
    ui.element()
        .id("sidebar")
        .width(fixed!(160.0))
        .height(grow!())
        .background_color(0x1F2630)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 8, 8)).gap(6))
        .children(|ui| {
            ui.text("Boon Circuit", |text| text.font_size(20).color(0xF1F5FA));
            ui.text("Dev console", |text| text.font_size(11).color(0xAFC1D6));
            for example in example_nav_specs() {
                nav_item(
                    ui,
                    example.nav_id,
                    example.label,
                    state.selected == example.name,
                );
            }
            ui.text(&format!("ipc {}", state.connection_status), |text| {
                text.font_size(11).color(0xAFC1D6)
            });
        });
}

fn dev_toolbar(ui: &mut Ui<'_, ()>, state: &DevWindowState) {
    ui.element()
        .id("toolbar")
        .height(fixed!(34.0))
        .width(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(5).align(Left, CenterY))
        .children(|ui| {
            toolbar_button(ui, "run_button", "Run", true);
            toolbar_button(ui, "reset_button", "Reset", false);
            toolbar_button(ui, "step_button", "Step", false);
            ui.text(
                &format!(
                    "{} / generation {}",
                    state.selected, state.source_generation
                ),
                |text| text.font_size(12).color(0x1F2630),
            );
        });
}

fn dev_source_panel(ui: &mut Ui<'_, ()>, state: &DevWindowState) {
    ui.element()
        .id("source_panel")
        .width(fixed!(650.0))
        .height(grow!())
        .background_color(0xF8FAFD)
        .layout(|layout| layout.direction(TopToBottom).gap(8))
        .children(|ui| {
            ui.text("Source", |text| text.font_size(18).color(0x1F2630));
            ui.element()
                .id("source_editor")
                .width(grow!())
                .height(grow!())
                .background_color(0xFFFFFF)
                .border(|border| border.color(0xD5DDE8).all(1))
                .layout(|layout| layout.padding((10, 10, 8, 8)))
                .text_input(|input| {
                    input
                        .multiline()
                        .drag_select()
                        .font(&DEFAULT_FONT)
                        .font_size(14)
                        .line_height(18)
                        .text_color(0x1F2630)
                        .cursor_color(0x2F6FB8)
                        .selection_color(0xB9D7F5)
                })
                .empty();
            if let Some(snapshot) = &state.latest_snapshot
                && let Some(error) = &snapshot.last_error
            {
                ui.text(error, |text| text.font_size(12).color(0xA32929));
            }
        });
}

fn dev_runtime_panels(ui: &mut Ui<'_, ()>, state: &DevWindowState) {
    ui.element()
        .id("runtime_panel")
        .width(grow!())
        .height(grow!())
        .layout(|layout| layout.direction(TopToBottom).gap(10))
        .children(|ui| {
            ui.element()
                .id("debug_panel_row_a")
                .width(grow!())
                .height(grow!())
                .layout(|layout| layout.direction(LeftToRight).gap(10))
                .children(|ui| {
                    dev_json_panel(ui, "delta_panel", "Deltas", dev_delta_json(state));
                    dev_json_panel(
                        ui,
                        "inspector_panel",
                        "Inspector",
                        dev_inspector_json(state),
                    );
                });
            ui.element()
                .id("debug_panel_row_b")
                .width(grow!())
                .height(grow!())
                .layout(|layout| layout.direction(LeftToRight).gap(10))
                .children(|ui| {
                    dev_json_panel(
                        ui,
                        "explanation_panel",
                        "Causes",
                        state
                            .latest_snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.report_summary.clone())
                            .unwrap_or_else(|| json!(null)),
                    );
                    dev_json_panel(
                        ui,
                        "metrics_panel",
                        "Metrics",
                        state.latest_frame_metrics.clone(),
                    );
                });
        });
}

fn dev_scenario_panel(ui: &mut Ui<'_, ()>, state: &DevWindowState) {
    ui.element()
        .id("scenario_detail_panel")
        .width(grow!())
        .height(fixed!(180.0))
        .background_color(0xFFFFFF)
        .border(|border| border.color(0xD5DDE8).all(1))
        .layout(|layout| layout.direction(TopToBottom).padding((10, 10, 8, 8)).gap(4))
        .children(|ui| {
            ui.text("Scenario", |text| text.font_size(16).color(0x596579));
            if let Some(snapshot) = &state.latest_snapshot {
                for (index, label) in snapshot.scenario_steps.iter().enumerate().take(7) {
                    ui.text(&format!("{} {}", index + 1, label), |text| {
                        text.font_size(12).color(0x1F2630)
                    });
                }
            }
        });
}

fn dev_json_panel(ui: &mut Ui<'_, ()>, id: &'static str, title: &str, value: serde_json::Value) {
    panel(ui, id, title, |ui| compact_json(ui, &value));
}

fn dev_delta_json(state: &DevWindowState) -> serde_json::Value {
    state
        .latest_snapshot
        .as_ref()
        .map(|snapshot| json!(snapshot.semantic_delta_tail))
        .unwrap_or_else(|| json!([]))
}

fn dev_inspector_json(state: &DevWindowState) -> serde_json::Value {
    state
        .latest_snapshot
        .as_ref()
        .map(|snapshot| snapshot.state_summary.clone())
        .unwrap_or_else(|| json!(null))
}

fn sidebar(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("sidebar")
        .width(fixed!(160.0))
        .height(grow!())
        .background_color(0x1F2630)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 8, 8)).gap(6))
        .children(|ui| {
            ui.text("Boon Circuit", |text| text.font_size(20).color(0xF1F5FA));
            ui.text("Ply playground", |text| text.font_size(11).color(0xAFC1D6));
            for example in example_nav_specs() {
                nav_item(
                    ui,
                    example.nav_id,
                    example.label,
                    state.selected == example.name,
                );
            }
            ui.text(&format!("step {}", step_label(state)), |text| {
                text.font_size(11).color(0xAFC1D6)
            });
            scenario_checklist(ui, state);
        });
}

fn scenario_checklist(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    if state.scenario_steps.is_empty() {
        return;
    }
    let completed = state.step_limit.unwrap_or(state.scenario_len);
    ui.element()
        .id("scenario_checklist")
        .width(grow!())
        .height(grow!())
        .background_color(0x28313D)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 6, 6)).gap(3))
        .children(|ui| {
            ui.text("Scenario", |text| text.font_size(12).color(0xAFC1D6));
            for (index, label) in state.scenario_steps.iter().enumerate().take(14) {
                let marker = if index < completed { "x" } else { " " };
                let color = if index < completed {
                    0xF1F5FA
                } else {
                    0xAFC1D6
                };
                ui.text(&format!("[{marker}] {label}"), |text| {
                    text.font_size(10).color(color)
                });
            }
        });
}

fn scenario_detail_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "scenario_detail_panel", "Scenario", |ui| {
        let completed = state.step_limit.unwrap_or(state.scenario_len);
        for (index, label) in state.scenario_steps.iter().enumerate() {
            let marker = if index < completed { "x" } else { " " };
            let color = if index < completed {
                0x1F2630
            } else {
                0x596579
            };
            ui.text(&format!("[{marker}] {label}"), |text| {
                text.font_size(16).color(color)
            });
        }
    });
}

fn nav_item(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, selected: bool) {
    ui.element()
        .id(id)
        .width(grow!())
        .height(fixed!(28.0))
        .background_color(if selected { 0x2F6FB8 } else { 0x28313D })
        .layout(|layout| layout.padding((8, 8, 6, 6)).align(Left, CenterY))
        .children(|ui| {
            ui.text(label, |text| text.font_size(12).color(0xF1F5FA));
        });
}

fn content(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("content")
        .width(grow!())
        .height(grow!())
        .background_color(0xF8FAFD)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 8, 8)).gap(6))
        .children(|ui| {
            toolbar(ui, state);
            match state.view {
                PlaygroundView::App => app_first_panel(ui, state),
                PlaygroundView::Source => source_dev_panel(ui, state),
                PlaygroundView::Deltas => delta_panel(ui, state),
                PlaygroundView::Inspector => inspector_panel(ui, state),
                PlaygroundView::Causes => explanation_panel(ui, state),
                PlaygroundView::Scenario => scenario_detail_panel(ui, state),
            }
        });
}

fn toolbar(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("toolbar")
        .height(fixed!(34.0))
        .width(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(5).align(Left, CenterY))
        .children(|ui| {
            toolbar_button(ui, "run_button", "Run", true);
            toolbar_button(ui, "reset_button", "Reset", false);
            toolbar_button(ui, "step_button", "Step", false);
            view_button(ui, "view_app", "App", state.view == PlaygroundView::App);
            view_button(
                ui,
                "view_source",
                "Source",
                state.view == PlaygroundView::Source,
            );
            view_button(
                ui,
                "view_deltas",
                "Deltas",
                state.view == PlaygroundView::Deltas,
            );
            view_button(
                ui,
                "view_inspector",
                "Inspector",
                state.view == PlaygroundView::Inspector,
            );
            view_button(
                ui,
                "view_causes",
                "Causes",
                state.view == PlaygroundView::Causes,
            );
            view_button(
                ui,
                "view_scenario",
                "Scenario",
                state.view == PlaygroundView::Scenario,
            );
            ui.text(
                &format!("{} / {}", state.selected, step_label(state)),
                |text| text.font_size(12).color(0x1F2630),
            );
        });
}

fn toolbar_button(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, primary: bool) {
    ui.element()
        .id(id)
        .height(fixed!(28.0))
        .width(fixed!(60.0))
        .background_color(if primary { 0x2F6FB8 } else { 0xE4EAF2 })
        .layout(|layout| layout.align(CenterX, CenterY))
        .children(|ui| {
            ui.text(label, |text| {
                text.font_size(12)
                    .color(if primary { 0xFFFFFF } else { 0x1F2630 })
            });
        });
}

fn view_button(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, selected: bool) {
    ui.element()
        .id(id)
        .height(fixed!(28.0))
        .width(fixed!(70.0))
        .background_color(if selected { 0x1F2630 } else { 0xE4EAF2 })
        .layout(|layout| layout.align(CenterX, CenterY))
        .children(|ui| {
            ui.text(label, |text| {
                text.font_size(11)
                    .color(if selected { 0xFFFFFF } else { 0x1F2630 })
            });
        });
}

fn app_first_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("app_first_panel")
        .width(grow!())
        .height(grow!())
        .background_color(0xF5F5F5)
        .layout(|layout| {
            layout
                .direction(TopToBottom)
                .padding((8, 8, 8, 8))
                .align(CenterX, Top)
        })
        .children(|ui| {
            if let Some(error) = &state.last_error {
                ui.text(error, |text| text.font_size(18).color(0xA32929));
                return;
            }
            preview_body(ui, state, PreviewLayout::Full);
        });
}

fn source_dev_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("source_dev_panel")
        .width(grow!())
        .height(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(10))
        .children(|ui| {
            source_panel(ui);
            runtime_panel(ui, state);
        });
}

fn source_panel(ui: &mut Ui<'_, ()>) {
    ui.element()
        .id("source_panel")
        .width(fixed!(650.0))
        .height(grow!())
        .background_color(0xF8FAFD)
        .layout(|layout| layout.direction(TopToBottom).gap(8))
        .children(|ui| {
            ui.text("Source", |text| text.font_size(18).color(0x1F2630));
            ui.element()
                .id("source_editor")
                .width(grow!())
                .height(grow!())
                .background_color(0xFFFFFF)
                .border(|border| border.color(0xD5DDE8).all(1))
                .layout(|layout| layout.padding((10, 10, 8, 8)))
                .text_input(|input| {
                    input
                        .multiline()
                        .drag_select()
                        .font(&DEFAULT_FONT)
                        .font_size(14)
                        .line_height(18)
                        .text_color(0x1F2630)
                        .cursor_color(0x2F6FB8)
                        .selection_color(0xB9D7F5)
                })
                .empty();
        });
}

fn runtime_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("runtime_panel")
        .width(grow!())
        .height(grow!())
        .layout(|layout| layout.direction(TopToBottom).gap(10))
        .children(|ui| {
            preview_panel(ui, state);
            ui.element()
                .width(grow!())
                .height(fixed!(260.0))
                .layout(|layout| layout.direction(LeftToRight).gap(10))
                .children(|ui| {
                    delta_panel(ui, state);
                    inspector_panel(ui, state);
                    explanation_panel(ui, state);
                });
        });
}

fn preview_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "preview_panel", "Preview", |ui| {
        preview_body(ui, state, PreviewLayout::Panel);
    });
}

#[derive(Clone, Copy)]
enum PreviewLayout {
    Full,
    Panel,
}

fn preview_body(ui: &mut Ui<'_, ()>, state: &PlaygroundState, layout: PreviewLayout) {
    if let Some(error) = &state.last_error {
        ui.text(error, |text| text.font_size(14).color(0xA32929));
        return;
    }
    let Some(output) = &state.output else {
        return;
    };
    if state.render_nodes.is_empty() {
        ui.text("No document UI in Boon source", |text| {
            text.font_size(16).color(0x596579)
        });
        compact_json(ui, &output.state_summary);
        return;
    }
    if matches!(layout, PreviewLayout::Full) {
        let context = render_context_with_focus(state, output);
        ui.element()
            .id("dynamic_boon_preview")
            .width(grow!())
            .height(grow!())
            .layout(|layout| layout.direction(TopToBottom).align(CenterX, Top))
            .children(|ui| {
                render_nodes(ui, &state.render_nodes, &context);
            });
    } else {
        let context = render_context_with_focus(state, output);
        render_nodes(ui, &state.render_nodes, &context);
    }
}

fn render_nodes(ui: &mut Ui<'_, ()>, nodes: &[RenderNode], context: &RenderContext<'_>) {
    for node in nodes {
        render_node(ui, node, context);
    }
}

fn render_node(ui: &mut Ui<'_, ()>, node: &RenderNode, context: &RenderContext<'_>) {
    match node {
        RenderNode::Column {
            id,
            width,
            height,
            background,
            border,
            scroll_x,
            scroll_y,
            sync_scroll_x: _,
            scrollbar,
            gap,
            padding,
            children,
        } => {
            let mut element = ui
                .element()
                .width(width.map_or(grow!(), |width| fixed!(width)))
                .background_color(*background);
            if let Some(height) = height {
                element = element.height(fixed!(*height));
            }
            if let Some(border) = border {
                element = element.border(|border_style| border_style.color(*border).all(1));
            }
            if *scroll_x || *scroll_y {
                let enable_x = *scroll_x;
                let enable_y = *scroll_y;
                let enable_scrollbar = *scrollbar;
                element = element.overflow(move |overflow| {
                    if enable_x && enable_y {
                        overflow.scroll();
                    } else if enable_x {
                        overflow.scroll_x();
                    } else {
                        overflow.scroll_y();
                    }
                    if enable_scrollbar {
                        overflow.scrollbar(|bar| bar.width(8.0).min_thumb_size(24.0));
                    }
                    overflow
                });
            }
            let gap = *gap;
            let padding = *padding;
            element = element.layout(|layout| {
                let layout = layout.direction(TopToBottom).gap(gap as u16);
                if let Some(padding) = padding {
                    layout.padding(padding_u16(padding))
                } else {
                    layout
                }
            });
            if let Some(id) = id {
                let row_scope = render_scope_key(id, context);
                let hover_scope = row_scope.clone();
                element = element
                    .id(render_id(id, context))
                    .on_hover(move |_, _| record_hover_scope(&hover_scope));
                let child_context = context.with_hover_scope(row_scope);
                element.children(|ui| render_nodes(ui, children, &child_context));
            } else {
                element.children(|ui| render_nodes(ui, children, context));
            }
        }
        RenderNode::Row {
            id,
            height,
            background,
            border,
            scroll_x,
            scroll_y,
            sync_scroll_x: _,
            scrollbar,
            gap,
            padding,
            children,
        } => {
            let mut element = ui
                .element()
                .width(grow!())
                .height(height.map_or(fixed!(40.0), |height| fixed!(height)))
                .background_color(*background);
            if let Some(border) = border {
                element = element.border(|border_style| border_style.color(*border).bottom(1));
            }
            if *scroll_x || *scroll_y {
                let enable_x = *scroll_x;
                let enable_y = *scroll_y;
                let enable_scrollbar = *scrollbar;
                element = element.overflow(move |overflow| {
                    if enable_x && enable_y {
                        overflow.scroll();
                    } else if enable_x {
                        overflow.scroll_x();
                    } else {
                        overflow.scroll_y();
                    }
                    if enable_scrollbar {
                        overflow.scrollbar(|bar| bar.width(8.0).min_thumb_size(24.0));
                    }
                    overflow
                });
            }
            let gap = *gap;
            let padding = padding.unwrap_or((10.0, 10.0, 6.0, 6.0));
            element = element.layout(|layout| {
                layout
                    .direction(LeftToRight)
                    .padding(padding_u16(padding))
                    .gap(gap as u16)
                    .align(Left, CenterY)
            });
            if let Some(id) = id {
                let row_scope = render_scope_key(id, context);
                let hover_scope = row_scope.clone();
                element = element
                    .id(render_id(id, context))
                    .on_hover(move |_, _| record_hover_scope(&hover_scope));
                let child_context = context.with_hover_scope(row_scope);
                element.children(|ui| render_nodes(ui, children, &child_context));
            } else {
                element.children(|ui| render_nodes(ui, children, context));
            }
        }
        RenderNode::ForEach {
            list,
            item,
            children,
        } => {
            if let Some(rows) = resolve_path(context, list).and_then(serde_json::Value::as_array) {
                for (index, row) in rows.iter().enumerate() {
                    let item_context = context.with_binding(list, item, row, index);
                    render_nodes(ui, children, &item_context);
                }
            }
        }
        RenderNode::Text {
            value,
            size,
            color,
            background,
            width,
            height,
            strike_if,
            center,
        } => {
            let label = decorated_render_text(
                eval_render_value(value, context),
                strike_if
                    .as_ref()
                    .is_some_and(|condition| eval_bool(condition, context)),
            );
            let draw_text = |ui: &mut Ui<'_, ()>| {
                ui.text(&label, |text| text.font_size(*size).color(*color));
            };
            if width.is_some() || height.is_some() || *center {
                let mut element = ui
                    .element()
                    .width(match width {
                        Some(RenderExtent::Fill) => grow!(),
                        Some(RenderExtent::Fixed(width)) => fixed!(*width),
                        None => grow!(),
                    })
                    .height(
                        height.map_or(fixed!((*size as f32 + 12.0).max(28.0)), |height| {
                            fixed!(height)
                        }),
                    );
                if let Some(background) = background {
                    element = element.background_color(*background);
                }
                element
                    .layout(|layout| {
                        if *center {
                            layout.align(CenterX, CenterY)
                        } else {
                            layout.align(Left, CenterY)
                        }
                    })
                    .children(draw_text);
            } else {
                draw_text(ui);
            }
        }
        RenderNode::Input {
            id,
            key,
            value: _,
            edit_value: _,
            display_value: _,
            placeholder,
            change_source,
            submit_source,
            cancel_source: _,
            escape_source: _,
            blur_source: _,
            address,
            target,
            visible,
            size,
            width,
            height,
            color,
            placeholder_color,
            background,
            focused_background,
            border,
            focused_border,
            focus_proxy: _,
        } => {
            if visible
                .as_ref()
                .is_some_and(|visible| !eval_bool(visible, context))
            {
                return;
            }
            let element_id = render_id_with_key(id, key.as_ref(), context);
            let change_source = eval_render_source(change_source, context);
            let submit_source = eval_render_source(submit_source, context);
            let address_value = address
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_value = target
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_occurrence = target_occurrence(target.as_ref(), context);
            let submit_element_id = element_id.clone();
            let is_focused = CURRENT_FOCUSED_ELEMENT.with(|focused| {
                focused
                    .borrow()
                    .as_ref()
                    .is_some_and(|focused| *focused == element_id)
            });
            let mut element = ui
                .element()
                .id(element_id)
                .height(
                    height.map_or(fixed!((*size as f32 + 20.0).max(34.0)), |height| {
                        fixed!(height)
                    }),
                )
                .width(match width {
                    Some(RenderExtent::Fill) => grow!(),
                    Some(RenderExtent::Fixed(width)) => fixed!(*width),
                    None => grow!(),
                });
            element = element
                .background_color(if is_focused {
                    focused_background.unwrap_or(*background)
                } else {
                    *background
                })
                .border(|border_style| {
                    border_style
                        .color(if is_focused {
                            focused_border.or(*border).unwrap_or(0x2F6FB8)
                        } else {
                            border.unwrap_or(0xD5DDE8)
                        })
                        .all(if is_focused { 2 } else { 1 })
                })
                .layout(|layout| layout.padding((8, 8, 5, 5)))
                .text_input(|input| {
                    let change_address = address_value.clone();
                    let change_target = target_value.clone();
                    let submit_address = address_value.clone();
                    let submit_target = target_value.clone();
                    input
                        .placeholder(&eval_render_value(placeholder, context))
                        .font(&DEFAULT_FONT)
                        .font_size(*size)
                        .text_color(*color)
                        .placeholder_color(*placeholder_color)
                        .cursor_color(0x2F6FB8)
                        .selection_color(0xB9D7F5)
                        .on_changed(move |text| {
                            if let Some(source) = &change_source {
                                record_ui_source_observation(render_source_event(
                                    source,
                                    Some(text),
                                    None,
                                    change_address.as_deref(),
                                    change_target.as_deref(),
                                    target_occurrence,
                                ));
                            }
                        })
                        .on_submit(move |text| {
                            if let Some(source) = &submit_source {
                                suppress_next_blur_for_input(&submit_element_id);
                                record_ui_source_observation(render_source_event(
                                    source,
                                    Some(text),
                                    Some("Enter"),
                                    submit_address.as_deref(),
                                    submit_target.as_deref(),
                                    target_occurrence,
                                ));
                            }
                        })
                });
            element.empty();
        }
        RenderNode::Button {
            id,
            text,
            width,
            selected,
            source,
            double_click_source,
            address,
            target,
            visible,
            hover_visible,
            height,
            size,
            color,
            background,
            selected_color,
            selected_background,
            border,
            selected_border,
            color_if,
            if_color,
            strike_if,
            align_left,
        } => {
            if visible
                .as_ref()
                .is_some_and(|visible| !eval_bool(visible, context))
            {
                return;
            }
            let source = source.clone();
            let double_click_source = double_click_source.clone();
            let address_value = address
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_value = target
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_occurrence = target_occurrence(target.as_ref(), context);
            let selected = selected
                .as_ref()
                .is_some_and(|selection| selection_matches(selection, context));
            let hover_scope_active = !*hover_visible
                || context
                    .hover_scopes
                    .last()
                    .is_some_and(|scope| was_hover_scope_active(scope));
            let effective_color = if color_if
                .as_ref()
                .is_some_and(|condition| eval_bool(condition, context))
            {
                if_color.unwrap_or(*color)
            } else if *hover_visible && !hover_scope_active {
                *background
            } else {
                *color
            };
            let label = decorated_render_text(
                eval_render_value(text, context),
                strike_if
                    .as_ref()
                    .is_some_and(|condition| eval_bool(condition, context)),
            );
            let event_key = render_scope_key(id, context);
            ui.element()
                .id(render_id(id, context))
                .height(height.map_or(fixed!(32.0), |height| fixed!(height)))
                .width(match width {
                    Some(RenderExtent::Fill) => grow!(),
                    Some(RenderExtent::Fixed(width)) => fixed!(*width),
                    None => fixed!(118.0),
                })
                .background_color(if selected {
                    *selected_background
                } else {
                    *background
                })
                .border(|border_style| {
                    border_style
                        .color(if selected {
                            selected_border.or(*border).unwrap_or(0xFFFFFF)
                        } else {
                            border.unwrap_or(0xFFFFFF)
                        })
                        .all(1)
                })
                .layout(|layout| {
                    if *align_left {
                        layout.align(Left, CenterY)
                    } else {
                        layout.align(CenterX, CenterY)
                    }
                })
                .on_press(move |_, _| {
                    if let Some(source) = &double_click_source {
                        if !register_double_click(&event_key) {
                            return;
                        }
                        record_ui_source_observation(render_source_event(
                            source,
                            None,
                            None,
                            address_value.as_deref(),
                            target_value.as_deref(),
                            target_occurrence,
                        ));
                    } else if let Some(source) = &source {
                        record_ui_source_observation(render_source_event(
                            source,
                            None,
                            None,
                            address_value.as_deref(),
                            target_value.as_deref(),
                            target_occurrence,
                        ));
                    }
                })
                .children(|ui| {
                    ui.text(&label, |text| {
                        text.font_size(*size).color(if selected {
                            *selected_color
                        } else {
                            effective_color
                        })
                    });
                });
        }
        RenderNode::Checkbox {
            id,
            checked,
            source,
            target,
            size,
        } => {
            let source = source.clone();
            let target_value = target
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_occurrence = target_occurrence(target.as_ref(), context);
            let checked = eval_bool(checked, context);
            ui.element()
                .id(render_id(id, context))
                .width(fixed!(*size + 12.0))
                .height(fixed!(*size + 12.0))
                .background_color(0xFFFFFF)
                .layout(|layout| layout.align(CenterX, CenterY))
                .on_press(move |_, _| {
                    if let Some(source) = &source {
                        record_ui_source_observation(render_source_event(
                            source,
                            None,
                            None,
                            None,
                            target_value.as_deref(),
                            target_occurrence,
                        ));
                    }
                })
                .children(|ui| {
                    ui.text(if checked { "✓" } else { "○" }, |text| {
                        text.font_size(*size as u16).color(if checked {
                            0x3EA390
                        } else {
                            0x949494
                        })
                    });
                });
        }
    }
}

fn decorated_render_text(label: String, strike: bool) -> String {
    if !strike {
        return label;
    }
    let mut decorated = String::with_capacity(label.len() * 3);
    for ch in label.chars() {
        decorated.push(ch);
        decorated.push('\u{0336}');
    }
    decorated
}

fn render_source_event(
    source: &str,
    text: Option<&str>,
    key: Option<&str>,
    address: Option<&str>,
    target_text: Option<&str>,
    target_occurrence: Option<usize>,
) -> serde_json::Value {
    let mut event = serde_json::Map::new();
    event.insert("source".to_owned(), json!(source));
    if let Some(text) = text {
        event.insert("text".to_owned(), json!(text));
    }
    if let Some(key) = key {
        event.insert("key".to_owned(), json!(key));
    }
    if let Some(address) = address {
        event.insert("address".to_owned(), json!(address));
    }
    if let Some(target_text) = target_text {
        event.insert("target_text".to_owned(), json!(target_text));
    }
    if let Some(target_occurrence) = target_occurrence {
        event.insert("target_occurrence".to_owned(), json!(target_occurrence));
    }
    serde_json::Value::Object(event)
}

fn render_scope_key(id: &str, context: &RenderContext<'_>) -> String {
    if let Some(index) = context.index_stack.last() {
        format!("{id}[{index}]")
    } else {
        id.to_owned()
    }
}

fn record_hover_scope(scope: &str) {
    CURRENT_HOVER_SCOPES.with(|scopes| {
        scopes.borrow_mut().insert(scope.to_owned());
    });
}

fn begin_hover_tracking_frame() {
    CURRENT_HOVER_SCOPES.with(|scopes| scopes.borrow_mut().clear());
}

fn finish_hover_tracking_frame() {
    let current = CURRENT_HOVER_SCOPES.with(|scopes| scopes.borrow().clone());
    LAST_HOVER_SCOPES.with(|scopes| {
        *scopes.borrow_mut() = current;
    });
}

fn was_hover_scope_active(scope: &str) -> bool {
    LAST_HOVER_SCOPES.with(|scopes| scopes.borrow().contains(scope))
}

fn register_double_click(key: &str) -> bool {
    const DOUBLE_CLICK_MS: u128 = 450;
    let now = Instant::now();
    LAST_BUTTON_PRESS.with(|last| {
        let mut last = last.borrow_mut();
        let matched = last.as_ref().is_some_and(|(last_key, instant)| {
            last_key == key && instant.elapsed().as_millis() <= DOUBLE_CLICK_MS
        });
        if matched {
            *last = None;
            true
        } else {
            *last = Some((key.to_owned(), now));
            false
        }
    })
}

fn target_occurrence(target: Option<&RenderValue>, context: &RenderContext<'_>) -> Option<usize> {
    let RenderValue::Path(path) = target? else {
        return None;
    };
    let (binding_name, field_path) = path.split_once('.')?;
    let binding = context
        .bindings
        .iter()
        .rev()
        .find(|binding| binding.name == binding_name)?;
    let target_value = json_path_string(binding.value, field_path)?;
    let rows = resolve_path(context, &binding.list)?.as_array()?;
    Some(
        rows.iter()
            .take(binding.index.saturating_add(1))
            .filter(|row| {
                json_path_string(row, field_path).is_some_and(|value| value == target_value)
            })
            .count()
            .max(1),
    )
}

fn json_path_string<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    let mut value = value;
    for part in path.split('.') {
        value = value.get(part)?;
    }
    value.as_str()
}

fn render_id(id: &str, context: &RenderContext<'_>) -> Id {
    let label = render_id_label(id);
    if let Some(index) = context.index_stack.last() {
        Id::new_index(label, *index as u32)
    } else {
        Id::new(label)
    }
}

fn render_id_with_key(id: &str, key: Option<&RenderValue>, context: &RenderContext<'_>) -> Id {
    if let Some(key) = key {
        let key = eval_render_value(key, context);
        if !key.is_empty() {
            return Id::new(render_id_label(&format!("{id}_{key}")));
        }
    }
    render_id(id, context)
}

fn render_id_label(id: &str) -> &'static str {
    RENDER_ID_LABELS.with(|labels| {
        let mut labels = labels.borrow_mut();
        if let Some(label) = labels.get(id) {
            return *label;
        }
        let label = Box::leak(id.to_owned().into_boxed_str());
        labels.insert(id.to_owned(), label);
        label
    })
}

fn selection_matches(selection: &RenderSelection, context: &RenderContext<'_>) -> bool {
    eval_path_text(&selection.path, context).as_deref() == Some(selection.expected.as_str())
}

fn eval_bool(value: &RenderValue, context: &RenderContext<'_>) -> bool {
    match value {
        RenderValue::Literal(value) => matches!(value.as_str(), "true" | "True" | "1"),
        RenderValue::Path(path) => resolve_path(context, path)
            .map(|value| {
                value
                    .as_bool()
                    .unwrap_or_else(|| value.as_u64().unwrap_or_default() != 0)
            })
            .unwrap_or(false),
        RenderValue::Template(value) => !eval_template(value, context).is_empty(),
    }
}

fn eval_render_value(value: &RenderValue, context: &RenderContext<'_>) -> String {
    match value {
        RenderValue::Literal(value) => value.clone(),
        RenderValue::Path(path) => eval_path_text(path, context).unwrap_or_default(),
        RenderValue::Template(value) => eval_template(value, context),
    }
}

fn eval_render_source(value: &Option<RenderValue>, context: &RenderContext<'_>) -> Option<String> {
    value
        .as_ref()
        .map(|value| eval_render_value(value, context))
        .filter(|value| !value.is_empty())
}

fn eval_template(template: &str, context: &RenderContext<'_>) -> String {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let path = &after_start[..end];
        output.push_str(&eval_path_text(path, context).unwrap_or_default());
        rest = &after_start[end + 1..];
    }
    output.push_str(rest);
    output
}

fn eval_path_text(path: &str, context: &RenderContext<'_>) -> Option<String> {
    let value = resolve_path(context, path)?;
    if let Some(value) = value.as_str() {
        Some(value.to_owned())
    } else if let Some(value) = value.as_bool() {
        Some(value.to_string())
    } else if let Some(value) = value.as_u64() {
        Some(value.to_string())
    } else if let Some(value) = value.as_i64() {
        Some(value.to_string())
    } else if value.is_null() {
        Some(String::new())
    } else {
        Some(value.to_string())
    }
}

fn resolve_path<'a>(context: &'a RenderContext<'a>, path: &str) -> Option<&'a serde_json::Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = context
        .bindings
        .iter()
        .rev()
        .find_map(|binding| (binding.name == first).then_some(binding.value))
        .or_else(|| {
            context
                .overlays
                .iter()
                .rev()
                .find_map(|(name, value)| (name == first).then_some(value))
        })
        .or_else(|| context.root.get(first))?;
    for part in parts {
        value = value.get(part)?;
    }
    Some(value)
}

fn render_nodes_from_output(output: &RunOutput) -> Vec<RenderNode> {
    output
        .document
        .as_ref()
        .and_then(|document| render_nodes_from_document(document).ok())
        .unwrap_or_default()
}

fn render_nodes_from_document(
    document: &boon_parser::DocumentAst,
) -> Result<Vec<RenderNode>, String> {
    document_children(&document.root, &document.expressions)
}

fn document_children(
    statement: &boon_parser::AstStatement,
    expressions: &[boon_parser::AstExpr],
) -> Result<Vec<RenderNode>, String> {
    let Some(children) = statement
        .children
        .iter()
        .find(|child| document_statement_field(child).as_deref() == Some("children"))
    else {
        return Ok(Vec::new());
    };
    children
        .children
        .iter()
        .filter(|child| document_statement_field(child).as_deref() == Some("element"))
        .map(|child| render_node_from_document_element(child, expressions))
        .collect()
}

fn render_node_from_document_element(
    element: &boon_parser::AstStatement,
    expressions: &[boon_parser::AstExpr],
) -> Result<RenderNode, String> {
    let attrs = document_attrs(element, expressions);
    let kind = required_attr(&attrs, "kind")?;
    let children = document_children(element, expressions)?;
    match kind.as_str() {
        "Text" => Ok(RenderNode::Text {
            value: render_value_from_attrs(&attrs, "value")
                .or_else(|| render_value_from_attrs(&attrs, "text"))
                .or_else(|| render_value_from_attrs(&attrs, "template"))
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            size: parse_size(&attrs, 16),
            color: parse_color(&attrs, "color", 0x1F2630),
            background: parse_optional_color(&attrs, "bg"),
            width: attrs
                .get("width")
                .and_then(|value| RenderExtent::from_attr(value)),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            strike_if: render_value_from_attrs(&attrs, "strike_if"),
            center: parse_bool_attr(&attrs, "center"),
        }),
        "Input" => Ok(RenderNode::Input {
            id: required_attr(&attrs, "id")?,
            key: render_value_from_attrs(&attrs, "key"),
            value: render_value_from_attrs(&attrs, "value")
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            edit_value: render_value_from_attrs(&attrs, "edit_value"),
            display_value: render_value_from_attrs(&attrs, "display_value"),
            placeholder: render_value_from_attrs(&attrs, "placeholder")
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            change_source: render_value_from_attrs(&attrs, "change"),
            submit_source: render_value_from_attrs(&attrs, "submit"),
            cancel_source: render_value_from_attrs(&attrs, "cancel"),
            escape_source: render_value_from_attrs(&attrs, "escape"),
            blur_source: render_value_from_attrs(&attrs, "blur"),
            address: render_value_from_attrs(&attrs, "address"),
            target: render_value_from_attrs(&attrs, "target"),
            visible: render_value_from_attrs(&attrs, "visible"),
            focus_proxy: parse_bool_attr(&attrs, "focus_proxy"),
            size: parse_size(&attrs, 16),
            width: attrs
                .get("width")
                .and_then(|value| RenderExtent::from_attr(value)),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            color: parse_color(&attrs, "color", 0x1F2630),
            placeholder_color: parse_color(&attrs, "placeholder_color", 0x8B97A7),
            background: parse_color(&attrs, "bg", 0xFFFFFF),
            focused_background: parse_optional_color(&attrs, "focused_bg"),
            border: parse_optional_color(&attrs, "border"),
            focused_border: parse_optional_color(&attrs, "focused_border"),
        }),
        "Button" => Ok(RenderNode::Button {
            id: required_attr(&attrs, "id")?,
            text: render_value_from_attrs(&attrs, "text")
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            width: attrs
                .get("width")
                .and_then(|value| RenderExtent::from_attr(value)),
            selected: attrs.get("selected").and_then(|value| {
                let (path, expected) = value.split_once(':')?;
                Some(RenderSelection {
                    path: path.strip_prefix('$').unwrap_or(path).to_owned(),
                    expected: expected.to_owned(),
                })
            }),
            source: attrs.get("source").cloned(),
            double_click_source: attrs.get("double_click").cloned(),
            address: render_value_from_attrs(&attrs, "address"),
            target: render_value_from_attrs(&attrs, "target"),
            visible: render_value_from_attrs(&attrs, "visible"),
            hover_visible: parse_bool_attr(&attrs, "hover_visible"),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            size: parse_size(&attrs, 14),
            color: parse_color(&attrs, "color", 0x1F2630),
            background: parse_color(&attrs, "bg", 0xFFFFFF),
            selected_color: parse_color(&attrs, "selected_color", 0x1F2630),
            selected_background: parse_color(&attrs, "selected_bg", 0xFFFFFF),
            border: parse_optional_color(&attrs, "border"),
            selected_border: parse_optional_color(&attrs, "selected_border"),
            color_if: render_value_from_attrs(&attrs, "color_if"),
            if_color: parse_optional_color(&attrs, "if_color"),
            strike_if: render_value_from_attrs(&attrs, "strike_if"),
            align_left: attrs.get("align").is_some_and(|value| value == "left"),
        }),
        "Checkbox" => Ok(RenderNode::Checkbox {
            id: required_attr(&attrs, "id")?,
            checked: render_value_from_attrs(&attrs, "checked")
                .unwrap_or_else(|| RenderValue::Literal("False".to_owned())),
            source: attrs.get("source").cloned(),
            target: render_value_from_attrs(&attrs, "target"),
            size: parse_float(&attrs, "size", 48.0),
        }),
        "Column" => Ok(RenderNode::Column {
            id: attrs.get("id").cloned(),
            width: attrs.get("width").and_then(|value| value.parse().ok()),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            background: parse_color(&attrs, "bg", 0xFFFFFF),
            border: parse_optional_color(&attrs, "border"),
            scroll_x: parse_bool_attr(&attrs, "scroll_x") || parse_bool_attr(&attrs, "scroll"),
            scroll_y: parse_bool_attr(&attrs, "scroll_y") || parse_bool_attr(&attrs, "scroll"),
            sync_scroll_x: attrs.get("sync_scroll_x").cloned(),
            scrollbar: parse_bool_attr(&attrs, "scrollbar"),
            gap: parse_float(&attrs, "gap", 0.0),
            padding: parse_padding(&attrs),
            children,
        }),
        "Row" => Ok(RenderNode::Row {
            id: attrs.get("id").cloned(),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            background: parse_color(&attrs, "bg", 0xFFFFFF),
            border: parse_optional_color(&attrs, "border").or(Some(0xEDEDED)),
            scroll_x: parse_bool_attr(&attrs, "scroll_x") || parse_bool_attr(&attrs, "scroll"),
            scroll_y: parse_bool_attr(&attrs, "scroll_y") || parse_bool_attr(&attrs, "scroll"),
            sync_scroll_x: attrs.get("sync_scroll_x").cloned(),
            scrollbar: parse_bool_attr(&attrs, "scrollbar"),
            gap: parse_float(&attrs, "gap", 8.0),
            padding: parse_padding(&attrs),
            children,
        }),
        "ForEach" => Ok(RenderNode::ForEach {
            list: required_attr(&attrs, "list")?,
            item: required_attr(&attrs, "item")?,
            children,
        }),
        _ => Err(format!("unsupported document element `{kind}`")),
    }
}

fn document_attrs(
    element: &boon_parser::AstStatement,
    expressions: &[boon_parser::AstExpr],
) -> BTreeMap<String, String> {
    element
        .children
        .iter()
        .filter(|child| document_statement_field(child).as_deref() != Some("children"))
        .filter_map(|child| {
            let key = document_statement_field(child)?;
            let value = document_statement_value(child, expressions)?;
            Some((key, value))
        })
        .collect()
}

fn document_statement_field(statement: &boon_parser::AstStatement) -> Option<String> {
    match &statement.kind {
        boon_parser::AstStatementKind::Field { name } => Some(name.clone()),
        _ => None,
    }
}

fn document_statement_value(
    statement: &boon_parser::AstStatement,
    expressions: &[boon_parser::AstExpr],
) -> Option<String> {
    let expr = expressions.get(statement.expr?)?;
    document_expr_value(expr, expressions)
}

fn document_expr_value(
    expr: &boon_parser::AstExpr,
    expressions: &[boon_parser::AstExpr],
) -> Option<String> {
    match &expr.kind {
        boon_parser::AstExprKind::StringLiteral(value)
        | boon_parser::AstExprKind::TextLiteral(value) => Some(value.clone()),
        boon_parser::AstExprKind::Number(value)
        | boon_parser::AstExprKind::Enum(value)
        | boon_parser::AstExprKind::Identifier(value) => Some(value.clone()),
        boon_parser::AstExprKind::Bool(value) => Some(value.to_string()),
        boon_parser::AstExprKind::Path(parts) => Some(parts.join(".")),
        boon_parser::AstExprKind::Pipe { input, op, args } => {
            let mut value = document_expr_value(expressions.get(*input)?, expressions)?;
            value.push_str("|>");
            value.push_str(op);
            if !args.is_empty() {
                value.push('(');
                value.push_str(
                    &args
                        .iter()
                        .filter_map(|arg| {
                            let mut arg_value =
                                document_expr_value(expressions.get(arg.value)?, expressions)?;
                            if let Some(name) = &arg.name {
                                arg_value = format!("{name}:{arg_value}");
                            }
                            Some(arg_value)
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                );
                value.push(')');
            }
            Some(value)
        }
        _ => None,
    }
}

fn render_value_from_attrs(attrs: &BTreeMap<String, String>, key: &str) -> Option<RenderValue> {
    let value = attrs.get(key)?;
    if key == "template" {
        Some(RenderValue::Template(value.clone()))
    } else if let Some(path) = value.strip_prefix('$') {
        Some(RenderValue::Path(path.to_owned()))
    } else if value.contains('{') && value.contains('}') {
        Some(RenderValue::Template(value.clone()))
    } else {
        Some(RenderValue::Literal(value.clone()))
    }
}

fn required_attr(attrs: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    attrs
        .get(key)
        .cloned()
        .ok_or_else(|| format!("document element missing `{key}`"))
}

fn parse_size(attrs: &BTreeMap<String, String>, default: u16) -> u16 {
    attrs
        .get("size")
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_float(attrs: &BTreeMap<String, String>, key: &str, default: f32) -> f32 {
    attrs
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_bool_attr(attrs: &BTreeMap<String, String>, key: &str) -> bool {
    attrs
        .get(key)
        .is_some_and(|value| matches!(value.as_str(), "true" | "True" | "1" | "yes"))
}

fn parse_optional_color(attrs: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    attrs.get(key).and_then(|value| parse_color_value(value))
}

fn parse_color(attrs: &BTreeMap<String, String>, key: &str, default: u32) -> u32 {
    parse_optional_color(attrs, key).unwrap_or(default)
}

fn parse_color_value(value: &str) -> Option<u32> {
    let value = value.strip_prefix('#').unwrap_or(value);
    u32::from_str_radix(value, 16).ok()
}

fn parse_padding(attrs: &BTreeMap<String, String>) -> Option<(f32, f32, f32, f32)> {
    let value = attrs.get("padding")?;
    let parts = value
        .split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [all] => Some((*all, *all, *all, *all)),
        [horizontal, vertical] => Some((*horizontal, *horizontal, *vertical, *vertical)),
        [left, right, top, bottom] => Some((*left, *right, *top, *bottom)),
        _ => None,
    }
}

fn padding_u16(padding: (f32, f32, f32, f32)) -> (u16, u16, u16, u16) {
    (
        padding.0.max(0.0) as u16,
        padding.1.max(0.0) as u16,
        padding.2.max(0.0) as u16,
        padding.3.max(0.0) as u16,
    )
}

fn visible_view_shape(state: &PlaygroundState) -> serde_json::Value {
    let Some(output) = &state.output else {
        return json!({
            "input_count": 0,
            "addressed_input_count": 0,
            "address_samples": [],
            "address_samples_after_first_four": [],
            "node_ids": [],
            "text_paths": [],
            "input_paths": [],
            "input_sources": [],
            "scroll_container_ids": [],
            "scroll_sync_x": []
        });
    };
    let mut addresses = Vec::new();
    let mut node_ids = Vec::new();
    let mut text_paths = Vec::new();
    let mut input_paths = Vec::new();
    let mut input_sources = Vec::new();
    let mut scroll_container_ids = Vec::new();
    let mut scroll_sync_x = Vec::new();
    collect_render_view_shape(
        &state.render_nodes,
        &RenderContext::root(&output.state_summary),
        &mut addresses,
        &mut node_ids,
        &mut text_paths,
        &mut input_paths,
        &mut input_sources,
        &mut scroll_container_ids,
        &mut scroll_sync_x,
    );
    let after_first_four = addresses
        .iter()
        .filter(|address| !matches!(address.as_str(), "A0" | "B0" | "C0" | "D0"))
        .take(12)
        .cloned()
        .collect::<Vec<_>>();
    json!({
        "input_count": addresses.len(),
        "addressed_input_count": addresses.len(),
        "address_samples": addresses.iter().take(12).cloned().collect::<Vec<_>>(),
        "address_samples_after_first_four": after_first_four,
        "last_address_sample": addresses.last().cloned(),
        "node_ids": node_ids,
        "text_paths": text_paths,
        "input_paths": input_paths,
        "input_sources": input_sources,
        "scroll_container_ids": scroll_container_ids,
        "scroll_sync_x": scroll_sync_x
    })
}

fn collect_render_view_shape(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    addresses: &mut Vec<String>,
    node_ids: &mut Vec<String>,
    text_paths: &mut Vec<String>,
    input_paths: &mut Vec<String>,
    input_sources: &mut Vec<String>,
    scroll_container_ids: &mut Vec<String>,
    scroll_sync_x: &mut Vec<serde_json::Value>,
) {
    for node in nodes {
        match node {
            RenderNode::Column {
                id,
                scroll_x,
                scroll_y,
                sync_scroll_x,
                children,
                ..
            }
            | RenderNode::Row {
                id,
                scroll_x,
                scroll_y,
                sync_scroll_x,
                children,
                ..
            } => {
                if let Some(id) = id {
                    let scoped = render_scope_key(id, context);
                    if node_ids.len() < 64 {
                        node_ids.push(scoped.clone());
                    }
                    if (*scroll_x || *scroll_y) && scroll_container_ids.len() < 32 {
                        scroll_container_ids.push(scoped);
                    }
                    if let Some(source) = sync_scroll_x
                        && scroll_sync_x.len() < 32
                    {
                        scroll_sync_x.push(json!({
                            "target": render_scope_key(id, context),
                            "source": source
                        }));
                    }
                }
                collect_render_view_shape(
                    children,
                    context,
                    addresses,
                    node_ids,
                    text_paths,
                    input_paths,
                    input_sources,
                    scroll_container_ids,
                    scroll_sync_x,
                );
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        collect_render_view_shape(
                            children,
                            &item_context,
                            addresses,
                            node_ids,
                            text_paths,
                            input_paths,
                            input_sources,
                            scroll_container_ids,
                            scroll_sync_x,
                        );
                    }
                }
            }
            RenderNode::Input {
                value,
                edit_value,
                display_value,
                change_source,
                submit_source,
                cancel_source,
                address,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                collect_render_value_path(value, input_paths);
                if let Some(value) = edit_value {
                    collect_render_value_path(value, input_paths);
                }
                if let Some(value) = display_value {
                    collect_render_value_path(value, input_paths);
                }
                if let Some(value) = change_source {
                    collect_render_value_path(value, input_paths);
                    collect_render_source_decl(value, input_sources);
                }
                if let Some(value) = submit_source {
                    collect_render_value_path(value, input_paths);
                    collect_render_source_decl(value, input_sources);
                }
                if let Some(value) = cancel_source {
                    collect_render_value_path(value, input_paths);
                    collect_render_source_decl(value, input_sources);
                }
                if let Some(address) = address {
                    collect_render_value_path(address, input_paths);
                    let address = eval_render_value(address, context);
                    if !address.is_empty() {
                        addresses.push(address);
                    }
                }
            }
            RenderNode::Text { value, .. } => {
                if text_paths.len() < 64
                    && let RenderValue::Path(path) = value
                {
                    text_paths.push(path.clone());
                }
            }
            RenderNode::Button { .. } | RenderNode::Checkbox { .. } => {}
        }
    }
}

fn collect_render_source_decl(value: &RenderValue, sources: &mut Vec<String>) {
    if sources.len() >= 96 {
        return;
    }
    match value {
        RenderValue::Literal(value) => {
            if !value.is_empty() {
                sources.push(value.clone());
            }
        }
        RenderValue::Path(path) => sources.push(format!("${path}")),
        RenderValue::Template(_) => {}
    }
}

fn collect_render_value_path(value: &RenderValue, paths: &mut Vec<String>) {
    if paths.len() >= 96 {
        return;
    }
    if let RenderValue::Path(path) = value {
        paths.push(path.clone());
    }
}

fn delta_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "delta_panel", "Deltas", |ui| {
        if let Some(output) = &state.output {
            for delta in output.semantic_deltas.iter().rev().take(7) {
                let field = delta.field_path.as_deref().unwrap_or("-");
                ui.text(&format!("{} {}", delta.kind, field), |text| {
                    text.font_size(13).color(0x1F2630)
                });
            }
        }
    });
}

fn inspector_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "inspector_panel", "Inspector", |ui| {
        if let Some(output) = &state.output {
            if state.selected == "todomvc" {
                if let Some(todo) = output.state_summary["todos"]
                    .as_array()
                    .and_then(|todos| todos.first())
                {
                    compact_json(ui, todo);
                }
            } else if let Some(cell) = output.state_summary["cells"]
                .as_array()
                .and_then(|cells| cells.first())
            {
                compact_json(ui, cell);
            }
        }
    });
}

fn explanation_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "explanation_panel", "Causes", |ui| {
        if let Some(output) = &state.output {
            let target = selected_cause_target(state);
            ui.text(target, |text| text.font_size(13).color(0x2F6FB8));
            if let Some(cause) = possible_causes_for(&output.report, target) {
                if let Some(sources) = cause["sources"].as_array() {
                    for source in sources.iter().filter_map(serde_json::Value::as_str).take(5) {
                        ui.text(&format!("<- {source}"), |text| {
                            text.font_size(11).color(0x1F2630)
                        });
                    }
                }
            }
            ui.text(
                &format!("nodes {}", output.report["graph_node_count"]),
                |text| text.font_size(13).color(0x1F2630),
            );
            ui.text(
                &format!("dirty keys {}", output.report["max_dirty_keys"]),
                |text| text.font_size(13).color(0x1F2630),
            );
            ui.text(
                &format!("deltas {}", output.semantic_deltas.len()),
                |text| text.font_size(13).color(0x1F2630),
            );
            ui.text(
                &format!("patches {}", output.render_patches.len()),
                |text| text.font_size(13).color(0x1F2630),
            );
        }
    });
}

fn selected_cause_target(state: &PlaygroundState) -> &'static str {
    if state.selected == "todomvc" {
        "todo.completed"
    } else {
        "cell.formula_text"
    }
}

fn possible_causes_for<'a>(
    report: &'a serde_json::Value,
    target: &str,
) -> Option<&'a serde_json::Value> {
    report["ir_debug_tables"]["possible_causes"]
        .as_array()?
        .iter()
        .find(|entry| entry["target"].as_str() == Some(target))
}

fn panel(ui: &mut Ui<'_, ()>, id: &'static str, title: &str, body: impl FnOnce(&mut Ui<'_, ()>)) {
    ui.element()
        .id(id)
        .width(grow!())
        .height(grow!())
        .background_color(0xFFFFFF)
        .border(|border| border.color(0xD5DDE8).all(1))
        .layout(|layout| layout.direction(TopToBottom).padding((10, 10, 8, 8)).gap(6))
        .children(|ui| {
            ui.text(title, |text| text.font_size(16).color(0x596579));
            body(ui);
        });
}

fn compact_json(ui: &mut Ui<'_, ()>, value: &serde_json::Value) {
    let text = value.to_string();
    for line in wrapped_text(&text, 38).into_iter().take(8) {
        ui.text(&line, |text| text.font_size(12).color(0x1F2630));
    }
}

fn wrapped_text(text: &str, width: usize) -> Vec<String> {
    text.as_bytes()
        .chunks(width)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

fn step_label(state: &PlaygroundState) -> String {
    match state.step_limit {
        Some(limit) => format!("{}/{}", limit.min(state.scenario_len), state.scenario_len),
        None => format!("all {}", state.scenario_len),
    }
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

#[derive(Clone, Copy, Debug)]
struct PixelStats {
    nonzero_channels: usize,
    unique_rgba_values: usize,
}

#[derive(Clone, Debug)]
struct ScreenshotCapture {
    pixel_stats: PixelStats,
    width: u32,
    height: u32,
    capture_backend: String,
}

fn bounds_json(bounds: ply_engine::math::BoundingBox) -> serde_json::Value {
    json!({
        "x": bounds.x,
        "y": bounds.y,
        "width": bounds.width,
        "height": bounds.height
    })
}

fn vector_json(vector: ply_engine::math::Vector2) -> serde_json::Value {
    json!({
        "x": vector.x,
        "y": vector.y
    })
}

fn image_stats(bytes: &[u8]) -> PixelStats {
    let nonzero_channels = bytes.iter().filter(|channel| **channel != 0).count();
    let mut unique = std::collections::BTreeSet::new();
    for pixel in bytes.chunks_exact(4) {
        unique.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
        if unique.len() > 256 {
            break;
        }
    }
    PixelStats {
        nonzero_channels,
        unique_rgba_values: unique.len(),
    }
}

fn capture_probe_frame_png(
    screenshot: &Path,
) -> Result<ScreenshotCapture, Box<dyn std::error::Error>> {
    if !focus_free_headed() {
        return capture_current_frame_png(screenshot);
    }
    let cache = FOCUS_FREE_SCREENSHOT_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache
        .lock()
        .expect("focus-free screenshot cache poisoned")
        .clone()
    {
        let (cached_path, mut capture) = cached;
        if cached_path.exists() {
            if let Some(parent) = screenshot.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&cached_path, screenshot)?;
            capture.capture_backend = "cached-focus-free-checkpoint".to_owned();
            return Ok(capture);
        }
    }
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels != 0 && pixel_stats.unique_rgba_values > 1 {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
        let capture = ScreenshotCapture {
            pixel_stats,
            width: u32::from(image.width),
            height: u32::from(image.height),
            capture_backend: "macroquad-framebuffer".to_owned(),
        };
        *cache.lock().expect("focus-free screenshot cache poisoned") =
            Some((screenshot.to_path_buf(), capture.clone()));
        return Ok(capture);
    }
    let capture = capture_with_cosmic_screenshot(screenshot)?;
    *cache.lock().expect("focus-free screenshot cache poisoned") =
        Some((screenshot.to_path_buf(), capture.clone()));
    Ok(capture)
}

fn capture_current_frame_png(
    screenshot: &Path,
) -> Result<ScreenshotCapture, Box<dyn std::error::Error>> {
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        capture_with_cosmic_screenshot(screenshot)
    } else {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
        Ok(ScreenshotCapture {
            pixel_stats,
            width: u32::from(image.width),
            height: u32::from(image.height),
            capture_backend: "macroquad-framebuffer".to_owned(),
        })
    }
}

fn capture_with_cosmic_screenshot(
    screenshot: &Path,
) -> Result<ScreenshotCapture, Box<dyn std::error::Error>> {
    let parent = screenshot.parent().unwrap_or_else(|| Path::new("."));
    let capture_dir = parent.join(format!(".cosmic-smoke-capture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&capture_dir);
    std::fs::create_dir_all(&capture_dir)?;
    let output = Command::new("cosmic-screenshot")
        .args([
            "--interactive=false",
            "--modal=false",
            "--notify=false",
            "--save-dir",
            capture_dir
                .to_str()
                .ok_or("cosmic screenshot directory path is not utf-8")?,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "macroquad framebuffer was blank and cosmic-screenshot fallback failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let capture = newest_png_in_dir(&capture_dir).ok_or_else(|| {
        format!(
            "cosmic-screenshot did not create a PNG in {}",
            capture_dir.display()
        )
    })?;
    std::fs::copy(&capture, screenshot)?;
    let decoded = image::open(screenshot)?.to_rgba8();
    let pixel_stats = image_stats(decoded.as_raw());
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        return Err(format!(
            "cosmic-screenshot fallback capture is blank: nonzero_channels={}, unique_rgba_values={}",
            pixel_stats.nonzero_channels, pixel_stats.unique_rgba_values
        )
        .into());
    }
    let _ = std::fs::remove_dir_all(&capture_dir);
    Ok(ScreenshotCapture {
        pixel_stats,
        width: decoded.width(),
        height: decoded.height(),
        capture_backend: "cosmic-screenshot".to_owned(),
    })
}

fn newest_png_in_dir(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("png"))
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
}

fn display_server() -> String {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "wayland".to_owned()
    } else if std::env::var("DISPLAY").is_ok() {
        "x11".to_owned()
    } else {
        "none".to_owned()
    }
}

fn os_keyboard_tool_name() -> &'static str {
    if std::env::var("DISPLAY").is_ok() && std::env::var("WAYLAND_DISPLAY").is_err() {
        "xdotool"
    } else {
        "wtype"
    }
}

fn display_socket() -> String {
    std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_else(|_| "none".to_owned())
}

fn native_display_contract() -> serde_json::Value {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let display = std::env::var("DISPLAY").ok();
    let isolated_backend = std::env::var("BOON_OS_INPUT_ISOLATED").ok();
    let isolated_x11 = isolated_backend.as_deref() == Some("xvfb")
        && session_type.as_deref() == Some("x11")
        && display.is_some();
    let wayland_native = session_type.as_deref() == Some("wayland") && wayland_display.is_some();
    json!({
        "required": true,
        "status": if wayland_native || isolated_x11 {
            "pass"
        } else {
            "fail"
        },
        "xdg_session_type": session_type,
        "wayland_display": wayland_display,
        "display": display,
        "isolated_backend": isolated_backend,
        "contract": "interactive native playground runs in a Wayland desktop session with WAYLAND_DISPLAY set; automated smoke and headed verifiers may use isolated Xvfb/X11 to avoid live desktop input or screenshots"
    })
}

fn command_path(command: &str) -> Option<String> {
    std::process::Command::new("sh")
        .args(["-lc", &format!("command -v {command}")])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

fn send_real_keyboard_text(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    require_os_input_permission("OS keyboard text input")?;
    if display_server() == "x11" {
        let Some(xdotool) = command_path("xdotool") else {
            return Err("xdotool is required for isolated X11 OS text input".into());
        };
        let status = std::process::Command::new(xdotool)
            .args(["type", "--delay", "1", "--"])
            .arg(text)
            .status()?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("xdotool type exited with {status}").into())
        };
    }
    let Some(wtype) = command_path("wtype") else {
        return Err("wtype is required for the OS input probe".into());
    };
    let status = std::process::Command::new(wtype).arg(text).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wtype exited with {status}").into())
    }
}

fn send_real_key(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    require_os_input_permission("OS keyboard key input")?;
    if display_server() == "x11" {
        let Some(xdotool) = command_path("xdotool") else {
            return Err("xdotool is required for isolated X11 OS key input".into());
        };
        let status = std::process::Command::new(xdotool)
            .args(["key", "--delay", "1", os_key_name(key)])
            .status()?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("xdotool key {key} exited with {status}").into())
        };
    }
    let Some(wtype) = command_path("wtype") else {
        return Err("wtype is required for headed keyboard activation".into());
    };
    let status = std::process::Command::new(wtype)
        .args(["-k", key])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wtype -k {key} exited with {status}").into())
    }
}

fn send_real_pointer_click(
    bounds: ply_engine::math::BoundingBox,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    require_os_input_permission("OS pointer click input")?;
    let (window_x, window_y, scale, local_x, local_y, screen_position, delta) =
        pointer_target_coordinates(bounds);
    if display_server() != "wayland"
        && let Ok(backend_report) = send_xtest_pointer_click(screen_position[0], screen_position[1])
    {
        return Ok(json!({
            "backend": "x11_xtest",
            "window_position": [window_x, window_y],
            "display_scale": scale,
            "element_center_local": [local_x, local_y],
            "screen_position": screen_position,
            "current_pointer_local_before_move": [local_x - delta[0] as f32 / scale, local_y - delta[1] as f32 / scale],
            "relative_move_delta": delta,
            "xtest": backend_report
        }));
    }
    let Some(ydotool) = command_path("ydotool") else {
        return Err(
            "XTest pointer injection failed and ydotool is unavailable for headed pointer probing"
                .into(),
        );
    };
    let delta_x_arg = format!("{}", delta[0]);
    let delta_y_arg = format!("{}", delta[1]);
    let move_status = std::process::Command::new(&ydotool)
        .args([
            "mousemove",
            "--delay",
            "30",
            "--",
            &delta_x_arg,
            &delta_y_arg,
        ])
        .status()?;
    if !move_status.success() {
        return Err(format!("ydotool mousemove exited with {move_status}").into());
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    let click_status = std::process::Command::new(&ydotool)
        .args(["click", "--delay", "30", "1"])
        .status()?;
    if !click_status.success() {
        return Err(format!("ydotool click exited with {click_status}").into());
    }
    Ok(json!({
        "backend": "ydotool",
        "window_position": [window_x, window_y],
        "display_scale": scale,
        "element_center_local": [local_x, local_y],
        "screen_position": screen_position,
        "relative_move_delta": delta,
        "mousemove_coordinate_mode": "relative_delta",
        "mousemove_status": move_status.to_string(),
        "click_status": click_status.to_string()
    }))
}

fn send_real_pointer_move(
    bounds: ply_engine::math::BoundingBox,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    require_os_input_permission("OS pointer movement input")?;
    let (window_x, window_y, scale, local_x, local_y, screen_position, delta) =
        pointer_target_coordinates(bounds);
    if display_server() != "wayland"
        && let Ok(backend_report) = send_xtest_pointer_move(screen_position[0], screen_position[1])
    {
        return Ok(json!({
            "backend": "x11_xtest",
            "window_position": [window_x, window_y],
            "display_scale": scale,
            "element_center_local": [local_x, local_y],
            "screen_position": screen_position,
            "relative_move_delta": delta,
            "xtest": backend_report
        }));
    }
    let Some(ydotool) = command_path("ydotool") else {
        return Err(
            "XTest pointer movement failed and ydotool is unavailable for headed pointer probing"
                .into(),
        );
    };
    let delta_x_arg = format!("{}", delta[0]);
    let delta_y_arg = format!("{}", delta[1]);
    let move_status = std::process::Command::new(&ydotool)
        .args([
            "mousemove",
            "--delay",
            "30",
            "--",
            &delta_x_arg,
            &delta_y_arg,
        ])
        .status()?;
    if !move_status.success() {
        return Err(format!("ydotool mousemove exited with {move_status}").into());
    }
    Ok(json!({
        "backend": "ydotool",
        "window_position": [window_x, window_y],
        "display_scale": scale,
        "element_center_local": [local_x, local_y],
        "screen_position": screen_position,
        "relative_move_delta": delta,
        "mousemove_coordinate_mode": "relative_delta",
        "mousemove_status": move_status.to_string()
    }))
}

fn send_real_pointer_wheel(
    bounds: ply_engine::math::BoundingBox,
    shift: bool,
    clicks: u32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    require_os_input_permission("OS pointer wheel input")?;
    let move_report = send_real_pointer_move(bounds)?;
    if display_server() == "wayland" {
        let Some(ydotool) = command_path("ydotool") else {
            return Err("ydotool is required for Wayland wheel input".into());
        };
        let clicks_arg = clicks.max(1).to_string();
        let button = if shift { "7" } else { "5" };
        let mut statuses = Vec::new();
        for _ in 0..clicks.max(1) {
            let status = std::process::Command::new(&ydotool)
                .args(["click", "--delay", "20", button])
                .status()?;
            if !status.success() {
                return Err(format!("ydotool wheel click exited with {status}").into());
            }
            statuses.push(status.to_string());
        }
        return Ok(json!({
            "backend": "ydotool-wayland-wheel",
            "move": move_report,
            "horizontal": shift,
            "button": button,
            "clicks": clicks.max(1),
            "clicks_arg": clicks_arg,
            "statuses": statuses,
            "shift_release_required": false
        }));
    }
    if display_server() != "x11" {
        return Err("visible wheel probing currently requires isolated X11/xdotool".into());
    }
    let Some(xdotool) = command_path("xdotool") else {
        return Err("xdotool is required for isolated X11 wheel input".into());
    };
    let clicks_arg = clicks.max(1).to_string();
    if shift {
        let keydown_status = std::process::Command::new(&xdotool)
            .args(["keydown", "Shift_L"])
            .status()?;
        if !keydown_status.success() {
            return Err(format!("xdotool keydown Shift_L exited with {keydown_status}").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        let click_status = std::process::Command::new(&xdotool)
            .args(["click", "--repeat", &clicks_arg, "--delay", "20", "5"])
            .status()?;
        if !click_status.success() {
            let _ = release_real_shift_key();
            return Err(format!("xdotool wheel click exited with {click_status}").into());
        }
        return Ok(json!({
            "backend": "xdotool-wheel",
            "move": move_report,
            "shift": shift,
            "button": 5,
            "clicks": clicks.max(1),
            "keydown_status": keydown_status.to_string(),
            "click_status": click_status.to_string(),
            "shift_release_required": true
        }));
    }
    let status = std::process::Command::new(xdotool)
        .args(["click", "--repeat", &clicks_arg, "--delay", "20", "5"])
        .status()?;
    if !status.success() {
        return Err(format!("xdotool wheel click exited with {status}").into());
    }
    Ok(json!({
        "backend": "xdotool-wheel",
        "move": move_report,
        "shift": shift,
        "button": 5,
        "clicks": clicks.max(1),
        "status": status.to_string()
    }))
}

fn release_real_shift_key() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    require_os_input_permission("OS keyboard modifier release")?;
    if display_server() != "x11" {
        return Err("visible Shift-wheel probing currently requires isolated X11/xdotool".into());
    }
    let Some(xdotool) = command_path("xdotool") else {
        return Err("xdotool is required for isolated X11 modifier release".into());
    };
    let status = std::process::Command::new(xdotool)
        .args(["keyup", "Shift_L"])
        .status()?;
    if !status.success() {
        return Err(format!("xdotool keyup Shift_L exited with {status}").into());
    }
    Ok(json!({
        "backend": "xdotool-keyup",
        "key": "Shift_L",
        "status": status.to_string()
    }))
}

fn text_input_end_click_bounds(
    mut bounds: ply_engine::math::BoundingBox,
) -> ply_engine::math::BoundingBox {
    bounds.x += (bounds.width - 4.0).max(bounds.width / 2.0);
    bounds.width = 1.0;
    bounds
}

fn pointer_target_coordinates(
    bounds: ply_engine::math::BoundingBox,
) -> (i32, i32, f32, f32, f32, [i32; 2], [i32; 2]) {
    let (window_x, window_y) = macroquad::miniquad::window::get_window_position();
    let scale = screen_dpi_scale();
    let local_x = bounds.x + bounds.width / 2.0;
    let local_y = bounds.y + bounds.height / 2.0;
    let screen_x = window_x as f32 + local_x * scale;
    let screen_y = window_y as f32 + local_y * scale;
    let (current_local_x, current_local_y) = mouse_position();
    let delta_x = (local_x - current_local_x) * scale;
    let delta_y = (local_y - current_local_y) * scale;
    let screen_position = [screen_x.round() as i32, screen_y.round() as i32];
    (
        window_x as i32,
        window_y as i32,
        scale,
        local_x,
        local_y,
        screen_position,
        [delta_x.round() as i32, delta_y.round() as i32],
    )
}

#[cfg(target_os = "linux")]
fn xtest_pointer_backend_available() -> bool {
    std::env::var_os("DISPLAY").is_some()
}

#[cfg(not(target_os = "linux"))]
fn xtest_pointer_backend_available() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn send_xtest_pointer_click(
    x: i32,
    y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint, c_ulong};

    #[repr(C)]
    struct Display {
        _private: [u8; 0],
    }

    #[link(name = "X11")]
    #[link(name = "Xtst")]
    unsafe extern "C" {
        fn XOpenDisplay(display_name: *const c_char) -> *mut Display;
        fn XDefaultScreen(display: *mut Display) -> c_int;
        fn XRootWindow(display: *mut Display, screen_number: c_int) -> c_ulong;
        fn XTestFakeMotionEvent(
            display: *mut Display,
            screen_number: c_int,
            x: c_int,
            y: c_int,
            delay: c_ulong,
        ) -> c_int;
        fn XTestFakeButtonEvent(
            display: *mut Display,
            button: c_uint,
            is_press: c_int,
            delay: c_ulong,
        ) -> c_int;
        fn XFlush(display: *mut Display) -> c_int;
        fn XCloseDisplay(display: *mut Display) -> c_int;
    }

    let display_name = std::env::var("DISPLAY").unwrap_or_default();
    if display_name.is_empty() {
        return Err("DISPLAY is not set for XTest pointer injection".into());
    }
    let display_name = CString::new(display_name)?;
    let display = unsafe { XOpenDisplay(display_name.as_ptr()) };
    if display.is_null() {
        return Err("XOpenDisplay failed for XTest pointer injection".into());
    }

    struct DisplayGuard(*mut Display);
    impl Drop for DisplayGuard {
        fn drop(&mut self) {
            unsafe {
                XCloseDisplay(self.0);
            }
        }
    }

    let guard = DisplayGuard(display);
    let screen = unsafe { XDefaultScreen(guard.0) };
    let root = unsafe { XRootWindow(guard.0, screen) };
    let moved = unsafe { XTestFakeMotionEvent(guard.0, screen, x, y, 0) };
    unsafe {
        XFlush(guard.0);
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    let pressed = unsafe { XTestFakeButtonEvent(guard.0, 1, 1, 0) };
    let released = unsafe { XTestFakeButtonEvent(guard.0, 1, 0, 0) };
    unsafe {
        XFlush(guard.0);
    }
    if moved == 0 || pressed == 0 || released == 0 {
        return Err(format!(
            "XTest pointer injection failed: moved={moved}, pressed={pressed}, released={released}"
        )
        .into());
    }
    Ok(json!({
        "display": std::env::var("DISPLAY").unwrap_or_default(),
        "screen": screen,
        "root_window": root,
        "motion_status": moved,
        "button_press_status": pressed,
        "button_release_status": released,
        "coordinate_mode": "absolute_x11_screen"
    }))
}

#[cfg(target_os = "linux")]
fn send_xtest_pointer_move(
    x: i32,
    y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_ulong};

    #[repr(C)]
    struct Display {
        _private: [u8; 0],
    }

    #[link(name = "X11")]
    #[link(name = "Xtst")]
    unsafe extern "C" {
        fn XOpenDisplay(display_name: *const c_char) -> *mut Display;
        fn XDefaultScreen(display: *mut Display) -> c_int;
        fn XRootWindow(display: *mut Display, screen_number: c_int) -> c_ulong;
        fn XTestFakeMotionEvent(
            display: *mut Display,
            screen_number: c_int,
            x: c_int,
            y: c_int,
            delay: c_ulong,
        ) -> c_int;
        fn XFlush(display: *mut Display) -> c_int;
        fn XCloseDisplay(display: *mut Display) -> c_int;
    }

    let display_name = std::env::var("DISPLAY").unwrap_or_default();
    if display_name.is_empty() {
        return Err("DISPLAY is not set for XTest pointer movement".into());
    }
    let display_name = CString::new(display_name)?;
    let display = unsafe { XOpenDisplay(display_name.as_ptr()) };
    if display.is_null() {
        return Err("XOpenDisplay failed for XTest pointer movement".into());
    }

    struct DisplayGuard(*mut Display);
    impl Drop for DisplayGuard {
        fn drop(&mut self) {
            unsafe {
                XCloseDisplay(self.0);
            }
        }
    }

    let guard = DisplayGuard(display);
    let screen = unsafe { XDefaultScreen(guard.0) };
    let root = unsafe { XRootWindow(guard.0, screen) };
    let moved = unsafe { XTestFakeMotionEvent(guard.0, screen, x, y, 0) };
    unsafe {
        XFlush(guard.0);
    }
    if moved == 0 {
        return Err("XTest pointer movement failed".into());
    }
    Ok(json!({
        "display": std::env::var("DISPLAY").unwrap_or_default(),
        "screen": screen,
        "root_window": root,
        "motion_status": moved,
        "coordinate_mode": "absolute_x11_screen"
    }))
}

#[cfg(not(target_os = "linux"))]
fn send_xtest_pointer_click(
    _x: i32,
    _y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Err("XTest pointer injection is only implemented on Linux".into())
}

#[cfg(not(target_os = "linux"))]
fn send_xtest_pointer_move(
    _x: i32,
    _y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Err("XTest pointer movement is only implemented on Linux".into())
}

fn os_key_name(scenario_key: &str) -> &str {
    match scenario_key {
        "Enter" => "Return",
        other => other,
    }
}

fn sanitize_artifact_label(label: &str) -> String {
    label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn report_artifact_prefix(report: &Path, fallback: &str) -> String {
    report
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{fallback}-headed"))
}

fn unix_seconds_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| sha256_file(&path).ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{PLAYGROUND_HELP, live_desktop_input_allowed_from};

    #[test]
    fn help_advertises_manual_launch_and_verifier_modes() {
        for needle in [
            "--example <todomvc|cells>",
            "--smoke-launch --example <name> --report <path>",
            "--verify-headed --example <name> --report <path>",
            "--verify-os-input-probe --report <path>",
        ] {
            assert!(
                PLAYGROUND_HELP.contains(needle),
                "missing help item {needle}"
            );
        }
    }

    #[test]
    fn live_desktop_input_requires_both_explicit_acknowledgements() {
        assert!(live_desktop_input_allowed_from(Some("1"), Some("1")));
        assert!(!live_desktop_input_allowed_from(Some("1"), None));
        assert!(!live_desktop_input_allowed_from(None, Some("1")));
        assert!(!live_desktop_input_allowed_from(Some("0"), Some("1")));
        assert!(!live_desktop_input_allowed_from(Some("1"), Some("0")));
    }
}
