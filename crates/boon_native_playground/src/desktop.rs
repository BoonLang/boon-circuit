use crate::catalog::{Catalog, LoadedExample};
use crate::protocol::{Connection, Message, PreviewIntent, Role, SourceUnit, TestStep};
use std::fs;
use std::io;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

pub type DesktopResult<T> = Result<T, Box<dyn std::error::Error>>;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CHILD_EXIT_GRACE: Duration = Duration::from_millis(500);
const SUPERVISOR_TICK: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
pub struct DesktopOptions {
    pub initial_example: Option<String>,
    pub socket_path: Option<PathBuf>,
    pub connect_timeout: Duration,
}

impl Default for DesktopOptions {
    fn default() -> Self {
        Self {
            initial_example: None,
            socket_path: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }
}

impl DesktopOptions {
    pub fn from_args(args: &[String]) -> DesktopResult<Self> {
        let mut options = Self::default();
        let mut index = 1;
        while index < args.len() {
            match args[index].as_str() {
                "--example" => {
                    options.initial_example = Some(required_value(args, index, "--example")?);
                    index += 2;
                }
                "--ipc-path" => {
                    options.socket_path =
                        Some(PathBuf::from(required_value(args, index, "--ipc-path")?));
                    index += 2;
                }
                "--child-connect-timeout-ms" => {
                    let value = required_value(args, index, "--child-connect-timeout-ms")?;
                    options.connect_timeout = Duration::from_millis(value.parse()?);
                    index += 2;
                }
                _ => index += 1,
            }
        }
        Ok(options)
    }
}

pub fn run(args: &[String]) -> DesktopResult<()> {
    run_with_options(DesktopOptions::from_args(args)?)
}

pub fn run_with_options(options: DesktopOptions) -> DesktopResult<()> {
    let catalog = Catalog::load()?;
    let initial_id = catalog.initial_id(options.initial_example.as_deref())?;
    let initial = catalog.open(initial_id)?;
    let executable = std::env::current_exe()?;
    let socket_path = options
        .socket_path
        .clone()
        .unwrap_or_else(default_socket_path);
    let mut supervisor = DesktopSupervisor::start(
        executable,
        socket_path,
        options.connect_timeout,
        catalog,
        initial,
    )?;
    supervisor.run()
}

struct DesktopSupervisor {
    socket_path: PathBuf,
    catalog: Catalog,
    source: SourceState,
    preview: ChildRole,
    dev: ChildRole,
    events: Receiver<Inbound>,
}

impl DesktopSupervisor {
    fn start(
        executable: PathBuf,
        socket_path: PathBuf,
        connect_timeout: Duration,
        catalog: Catalog,
        initial: LoadedExample,
    ) -> DesktopResult<Self> {
        let listener = bind_listener(&socket_path)?;
        let mut preview_child = spawn_child(&executable, Role::Preview, &socket_path)?;
        let mut dev_child = match spawn_child(&executable, Role::Dev, &socket_path) {
            Ok(child) => child,
            Err(error) => {
                stop_child(&mut preview_child);
                return Err(error);
            }
        };
        let connected = match accept_children(&listener, connect_timeout) {
            Ok(connected) => connected,
            Err(error) => {
                stop_child(&mut preview_child);
                stop_child(&mut dev_child);
                return Err(error);
            }
        };
        let (events_tx, events) = mpsc::channel();
        let preview = ChildRole::new(
            Role::Preview,
            preview_child,
            connected.preview,
            events_tx.clone(),
        )?;
        let dev = ChildRole::new(Role::Dev, dev_child, connected.dev, events_tx)?;
        Ok(Self {
            socket_path,
            catalog,
            source: SourceState::new(initial),
            preview,
            dev,
            events,
        })
    }

    fn run(&mut self) -> DesktopResult<()> {
        self.dev.send(&Message::Catalog {
            entries: self.catalog.items(),
            active_id: self.source.active.id.clone(),
        })?;
        self.send_open_editor()?;
        self.send_preview(PreviewIntent::Replace, None)?;

        let result = loop {
            match self.events.recv_timeout(SUPERVISOR_TICK) {
                Ok(Inbound::Message(role, message)) => {
                    if let Some(result) = self.route(role, message) {
                        break result;
                    }
                }
                Ok(Inbound::Closed(role, result)) => {
                    break result.map_err(|error| {
                        format!("{} IPC closed with an error: {error}", role_name(role)).into()
                    });
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(result) = self.child_exit_result()? {
                        break result;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        self.shutdown();
        result
    }

    fn route(&mut self, role: Role, message: Message) -> Option<DesktopResult<()>> {
        let result = match (role, message) {
            (Role::Dev, Message::Ready { role: Role::Dev }) => Ok(()),
            (
                Role::Preview,
                Message::Ready {
                    role: Role::Preview,
                },
            ) => self.dev.send(&Message::Ready {
                role: Role::Preview,
            }),
            (Role::Dev, Message::DevSelectExample { example_id }) => {
                self.select_example(&example_id)
            }
            (Role::Dev, Message::DevSourceChanged { revision, units }) => {
                self.accept_source(revision, units).and_then(|accepted| {
                    if accepted {
                        self.send_preview(PreviewIntent::Replace, None)
                    } else {
                        Ok(())
                    }
                })
            }
            (Role::Dev, Message::DevRun { revision, units }) => {
                self.accept_source(revision, units).and_then(|accepted| {
                    if accepted {
                        self.send_preview(PreviewIntent::Run, None)
                    } else {
                        Ok(())
                    }
                })
            }
            (Role::Dev, Message::DevReset) => self.reset_source(),
            (
                Role::Dev,
                Message::DevInspect {
                    request_id,
                    revision,
                    path,
                },
            ) => self.preview.send(&Message::PreviewInspect {
                request_id,
                revision,
                path,
            }),
            (
                Role::Dev,
                Message::DevTest {
                    request_id,
                    revision,
                    units,
                },
            ) => self.accept_source(revision, units).and_then(|accepted| {
                if accepted {
                    self.send_preview(PreviewIntent::Test, Some(request_id))
                } else {
                    Ok(())
                }
            }),
            (Role::Preview, message @ Message::PreviewStats(_))
            | (Role::Preview, message @ Message::PreviewStatus { .. })
            | (Role::Preview, message @ Message::PreviewRuntimeChanged { .. })
            | (Role::Preview, message @ Message::PreviewTestResult { .. })
            | (Role::Preview, message @ Message::PreviewInspectResult { .. }) => {
                self.dev.send(&message)
            }
            (_, Message::Shutdown) => return Some(Ok(())),
            (source, unexpected) => Err(format!(
                "desktop received invalid {} message: {unexpected:?}",
                role_name(source)
            )
            .into()),
        };
        result.err().map(Err)
    }

    fn select_example(&mut self, id: &str) -> DesktopResult<()> {
        let next = self.catalog.open(id)?;
        self.source.select(next);
        self.dev.send(&Message::Catalog {
            entries: self.catalog.items(),
            active_id: self.source.active.id.clone(),
        })?;
        self.send_open_editor()?;
        self.send_preview(PreviewIntent::Replace, None)
    }

    fn accept_source(&mut self, revision: u64, units: Vec<SourceUnit>) -> DesktopResult<bool> {
        validate_units(&units)?;
        if revision < self.source.revision {
            self.dev.send(&Message::PreviewStatus {
                revision,
                ok: false,
                message: format!(
                    "stale source revision {revision}; current revision is {}",
                    self.source.revision
                ),
            })?;
            return Ok(false);
        }
        self.source.revision = revision;
        self.source.working_units = units;
        Ok(true)
    }

    fn reset_source(&mut self) -> DesktopResult<()> {
        self.source.revision = self.source.revision.saturating_add(1);
        self.source
            .working_units
            .clone_from(&self.source.baseline_units);
        self.send_open_editor()?;
        self.send_preview(PreviewIntent::Reset, None)
    }

    fn send_open_editor(&mut self) -> DesktopResult<()> {
        self.dev.send(&Message::OpenEditor {
            example_id: self.source.active.id.clone(),
            label: self.source.active.label.clone(),
            revision: self.source.revision,
            units: self.source.working_units.clone(),
        })?;
        Ok(())
    }

    fn send_preview(
        &mut self,
        intent: PreviewIntent,
        request_id: Option<u64>,
    ) -> DesktopResult<()> {
        self.preview.send(&Message::PreviewApply {
            intent,
            request_id,
            revision: self.source.revision,
            units: self.source.working_units.clone(),
            test_steps: if intent == PreviewIntent::Test {
                self.source.test_steps.clone()
            } else {
                Vec::new()
            },
        })?;
        Ok(())
    }

    fn child_exit_result(&mut self) -> DesktopResult<Option<DesktopResult<()>>> {
        for child in [&mut self.preview, &mut self.dev] {
            if let Some(status) = child.process.try_wait()? {
                return Ok(Some(exit_result(child.role, status)));
            }
        }
        Ok(None)
    }

    fn shutdown(&mut self) {
        let _ = self.preview.send(&Message::Shutdown);
        let _ = self.dev.send(&Message::Shutdown);
        stop_child(&mut self.preview.process);
        stop_child(&mut self.dev.process);
    }
}

impl Drop for DesktopSupervisor {
    fn drop(&mut self) {
        self.shutdown();
        let _ = fs::remove_file(&self.socket_path);
    }
}

struct SourceState {
    active: LoadedExample,
    baseline_units: Vec<SourceUnit>,
    working_units: Vec<SourceUnit>,
    revision: u64,
    test_steps: Vec<TestStep>,
}

impl SourceState {
    fn new(active: LoadedExample) -> Self {
        let baseline_units = active.units.clone();
        let test_steps = active.test_steps.clone();
        Self {
            working_units: baseline_units.clone(),
            active,
            baseline_units,
            revision: 1,
            test_steps,
        }
    }

    fn select(&mut self, active: LoadedExample) {
        self.revision = self.revision.saturating_add(1);
        self.baseline_units = active.units.clone();
        self.working_units = active.units.clone();
        self.active = active;
        self.test_steps = self.active.test_steps.clone();
    }
}

struct ChildRole {
    role: Role,
    process: Child,
    writer: Connection,
}

impl ChildRole {
    fn new(
        role: Role,
        process: Child,
        connection: Connection,
        sender: Sender<Inbound>,
    ) -> DesktopResult<Self> {
        let mut reader = connection.try_clone()?;
        thread::Builder::new()
            .name(format!("boon-{}-ipc-reader", role_name(role)))
            .spawn(move || {
                loop {
                    match reader.receive() {
                        Ok(Some(message)) => {
                            if sender.send(Inbound::Message(role, message)).is_err() {
                                break;
                            }
                        }
                        Ok(None) => {
                            let _ = sender.send(Inbound::Closed(role, Ok(())));
                            break;
                        }
                        Err(error) => {
                            let _ = sender.send(Inbound::Closed(role, Err(error.to_string())));
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            role,
            process,
            writer: connection,
        })
    }

    fn send(&mut self, message: &Message) -> DesktopResult<()> {
        self.writer.send(message)?;
        Ok(())
    }
}

enum Inbound {
    Message(Role, Message),
    Closed(Role, Result<(), String>),
}

struct ConnectedChildren {
    preview: Connection,
    dev: Connection,
}

fn accept_children(listener: &UnixListener, timeout: Duration) -> DesktopResult<ConnectedChildren> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    let mut preview = None;
    let mut dev = None;
    while preview.is_none() || dev.is_none() {
        match listener.accept() {
            Ok((stream, _)) => {
                let mut connection = Connection::new(stream);
                connection.set_read_timeout(Some(timeout))?;
                let hello = connection
                    .receive()?
                    .ok_or("child closed IPC before its hello message")?;
                connection.set_read_timeout(None)?;
                match hello {
                    Message::Hello {
                        role: Role::Preview,
                        ..
                    } if preview.is_none() => preview = Some(connection),
                    Message::Hello {
                        role: Role::Dev, ..
                    } if dev.is_none() => dev = Some(connection),
                    Message::Hello { role, .. } => {
                        return Err(
                            format!("duplicate {} child connection", role_name(role)).into()
                        );
                    }
                    other => return Err(format!("child connected without hello: {other:?}").into()),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "preview/dev IPC connection timed out after {}ms",
                        timeout.as_millis()
                    )
                    .into());
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(ConnectedChildren {
        preview: preview.expect("preview connection"),
        dev: dev.expect("dev connection"),
    })
}

fn bind_listener(path: &Path) -> DesktopResult<UnixListener> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(UnixListener::bind(path)?)
}

fn spawn_child(executable: &Path, role: Role, socket_path: &Path) -> DesktopResult<Child> {
    let role = role_name(role);
    Ok(Command::new(executable)
        .args([
            "--role",
            role,
            "--connect",
            socket_path
                .to_str()
                .ok_or("desktop IPC path is not valid UTF-8")?,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?)
}

fn stop_child(child: &mut Child) {
    let deadline = Instant::now() + CHILD_EXIT_GRACE;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn exit_result(role: Role, status: ExitStatus) -> DesktopResult<()> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} child exited with {status}", role_name(role)).into())
    }
}

fn validate_units(units: &[SourceUnit]) -> DesktopResult<()> {
    if units.is_empty() {
        return Err("source update contains no units".into());
    }
    let mut paths = std::collections::BTreeSet::new();
    for unit in units {
        if unit.path.is_empty() {
            return Err("source update contains an empty path".into());
        }
        if !paths.insert(unit.path.as_str()) {
            return Err(format!("source update contains duplicate path `{}`", unit.path).into());
        }
    }
    Ok(())
}

fn default_socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("boon-playground-{}.sock", std::process::id()))
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Preview => "preview",
        Role::Dev => "dev",
    }
}

fn required_value(args: &[String], index: usize, flag: &str) -> DesktopResult<String> {
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value").into())
}
