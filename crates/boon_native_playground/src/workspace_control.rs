use cosmic_protocols::workspace::v2::client::{
    zcosmic_workspace_handle_v2::{self, ZcosmicWorkspaceHandleV2},
    zcosmic_workspace_manager_v2::{self, ZcosmicWorkspaceManagerV2},
};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_output, wl_registry},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::{self, ZxdgOutputManagerV1},
    zxdg_output_v1::{self, ZxdgOutputV1},
};

const READY: [u8; 4] = *b"BNWS";
const DEFAULT_TIMEOUT_MS: u64 = 10 * 60 * 1_000;

pub struct WorkspaceGuard {
    child: Child,
    input: Option<ChildStdin>,
    output_size: (i32, i32),
}

impl WorkspaceGuard {
    pub fn start(executable: &Path, workspace: &str) -> Result<Self, String> {
        let mut child = Command::new(executable)
            .args([
                "--role",
                "workspace-control",
                "--workspace",
                workspace,
                "--timeout-ms",
                &DEFAULT_TIMEOUT_MS.to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("start Wayland workspace guard: {error}"))?;
        let input = child
            .stdin
            .take()
            .ok_or("Wayland workspace guard has no control pipe")?;
        let mut output = child
            .stdout
            .take()
            .ok_or("Wayland workspace guard has no ready pipe")?;
        let mut ready = [0_u8; READY.len() + 8];
        output
            .read_exact(&mut ready)
            .map_err(|error| format!("Wayland workspace guard did not become ready: {error}"))?;
        if ready[..READY.len()] != READY {
            return Err("Wayland workspace guard returned an invalid handshake".to_owned());
        }
        let width = i32::from_le_bytes(ready[4..8].try_into().expect("fixed ready frame"));
        let height = i32::from_le_bytes(ready[8..12].try_into().expect("fixed ready frame"));
        if width <= 0 || height <= 0 {
            return Err(format!(
                "Wayland workspace guard returned invalid output size {width}x{height}"
            ));
        }
        Ok(Self {
            child,
            input: Some(input),
            output_size: (width, height),
        })
    }

    pub fn output_size(&self) -> (i32, i32) {
        self.output_size
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        if let Some(mut input) = self.input.take() {
            input
                .write_all(&[0])
                .and_then(|()| input.flush())
                .map_err(|error| format!("stop Wayland workspace guard: {error}"))?;
        }
        let status = self
            .child
            .wait()
            .map_err(|error| format!("wait for Wayland workspace guard: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Wayland workspace guard exited with {status}"))
        }
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

pub fn run_guard_process(args: &[String]) -> Result<(), String> {
    let workspace = argument(args, "--workspace")
        .ok_or("workspace-control requires --workspace")?
        .to_owned();
    let timeout_ms = argument(args, "--timeout-ms")
        .map(str::parse::<u64>)
        .transpose()
        .map_err(|error| format!("invalid workspace-control timeout: {error}"))?
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(1_000, DEFAULT_TIMEOUT_MS);
    let mut lease = WorkspaceLease::prepare(&workspace)?;
    let (width, height) = lease.output_size;
    let mut ready = [0_u8; READY.len() + 8];
    ready[..4].copy_from_slice(&READY);
    ready[4..8].copy_from_slice(&width.to_le_bytes());
    ready[8..12].copy_from_slice(&height.to_le_bytes());
    io::stdout()
        .write_all(&ready)
        .and_then(|()| io::stdout().flush())
        .map_err(|error| format!("write workspace-control ready handshake: {error}"))?;

    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut byte = [0_u8; 1];
        let _ = io::stdin().read(&mut byte);
        let _ = send.send(());
    });
    let _ = receive.recv_timeout(Duration::from_millis(timeout_ms));
    lease.finish()
}

fn argument<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

struct WorkspaceLease {
    queue: wayland_client::EventQueue<Catalog>,
    catalog: Catalog,
    _manager: ExtWorkspaceManagerV1,
    target: wayland_client::backend::ObjectId,
    output_size: (i32, i32),
    closed: bool,
}

impl WorkspaceLease {
    fn prepare(name: &str) -> Result<Self, String> {
        let connection = Connection::connect_to_env()
            .map_err(|error| format!("connect workspace-control to Wayland: {error}"))?;
        let (globals, mut queue) = registry_queue_init::<Catalog>(&connection)
            .map_err(|error| format!("read Wayland globals for workspace-control: {error}"))?;
        let qh = queue.handle();
        let cosmic_manager = globals
            .bind::<ZcosmicWorkspaceManagerV2, _, _>(&qh, 2..=2, ())
            .map_err(|error| format!("bind zcosmic_workspace_manager_v2: {error}"))?;
        let xdg_manager = globals
            .bind::<ZxdgOutputManagerV1, _, _>(&qh, 1..=3, ())
            .map_err(|error| format!("bind zxdg_output_manager_v1: {error}"))?;
        let output_handles = globals.contents().with_list(|list| {
            list.iter()
                .filter(|global| global.interface == "wl_output")
                .map(|global| {
                    globals.registry().bind::<wl_output::WlOutput, _, _>(
                        global.name,
                        global.version.min(4),
                        &qh,
                        (),
                    )
                })
                .collect::<Vec<_>>()
        });
        let manager = globals
            .bind::<ExtWorkspaceManagerV1, _, _>(&qh, 1..=1, ())
            .map_err(|error| format!("bind ext_workspace_manager_v1: {error}"))?;
        let mut catalog = Catalog {
            output_handles,
            _xdg_manager: Some(xdg_manager.clone()),
            cosmic_manager: Some(cosmic_manager),
            ..Catalog::default()
        };
        for output in catalog.output_handles.clone() {
            catalog.outputs.entry(output.id()).or_default();
            let xdg_output = xdg_manager.get_xdg_output(&output, &qh, output.clone());
            catalog.xdg_outputs.push(xdg_output);
        }
        queue
            .roundtrip(&mut catalog)
            .map_err(|error| format!("read initial workspace catalog: {error}"))?;

        let mut matching_targets = catalog
            .workspaces
            .values()
            .filter(|workspace| workspace.name == name)
            .map(|workspace| workspace.handle.clone());
        let target = matching_targets
            .next()
            .ok_or_else(|| format!("Wayland workspace `{name}` is not available"))?;
        if matching_targets.next().is_some() {
            return Err(format!(
                "Wayland workspace name `{name}` is ambiguous across output groups"
            ));
        }
        let group = catalog
            .groups
            .iter()
            .find(|(_, workspaces)| workspaces.contains(&target.id()))
            .map(|(group, _)| group.clone())
            .ok_or_else(|| format!("Wayland workspace `{name}` has no output group"))?;
        let active = catalog
            .groups
            .get(&group)
            .into_iter()
            .flatten()
            .filter_map(|id| catalog.workspaces.get(id))
            .find(|workspace| workspace.active)
            .ok_or_else(|| {
                format!("Wayland workspace group for `{name}` has no active workspace")
            })?;
        if active.handle == target {
            return Err(format!(
                "Wayland workspace `{name}` is active; isolated verification refuses to disturb it"
            ));
        }
        let cosmic_target = catalog
            .cosmic_workspaces
            .get(&target.id())
            .map(|workspace| workspace.handle.clone())
            .ok_or_else(|| format!("COSMIC workspace extension for `{name}` is unavailable"))?;
        let output_size = catalog
            .group_outputs
            .get(&group)
            .into_iter()
            .flatten()
            .filter_map(|id| catalog.outputs.get(id))
            .find(|output| output.width > 0 && output.height > 0)
            .map(|output| (output.width, output.height))
            .ok_or_else(|| format!("Wayland workspace `{name}` has no logical output geometry"))?;

        cosmic_target.set_tiling_state(zcosmic_workspace_handle_v2::TilingState::TilingEnabled);
        manager.commit();
        queue
            .roundtrip(&mut catalog)
            .map_err(|error| format!("enable background workspace tiling for `{name}`: {error}"))?;
        let target_tiled = catalog
            .cosmic_workspaces
            .get(&target.id())
            .is_some_and(|workspace| {
                matches!(
                    workspace.tiling,
                    Some(WEnum::Value(
                        zcosmic_workspace_handle_v2::TilingState::TilingEnabled
                    ))
                )
            });
        if !target_tiled {
            return Err(format!(
                "COSMIC did not enable tiling for Wayland workspace `{name}`"
            ));
        }
        let target_active = catalog
            .workspaces
            .get(&target.id())
            .is_some_and(|workspace| workspace.active);
        if target_active {
            return Err(format!(
                "COSMIC activated background workspace `{name}` during isolated setup"
            ));
        }
        Ok(Self {
            queue,
            catalog,
            _manager: manager,
            target: target.id(),
            output_size,
            closed: false,
        })
    }

    fn finish(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.queue
            .roundtrip(&mut self.catalog)
            .map_err(|error| format!("refresh isolated workspace before shutdown: {error}"))?;
        if self
            .catalog
            .workspaces
            .get(&self.target)
            .is_some_and(|workspace| workspace.active)
        {
            return Err(
                "isolated background workspace became active during verification".to_owned(),
            );
        }
        self.closed = true;
        Ok(())
    }
}

impl Drop for WorkspaceLease {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[derive(Default)]
struct Catalog {
    workspaces: HashMap<wayland_client::backend::ObjectId, Workspace>,
    groups: HashMap<wayland_client::backend::ObjectId, HashSet<wayland_client::backend::ObjectId>>,
    group_outputs:
        HashMap<wayland_client::backend::ObjectId, HashSet<wayland_client::backend::ObjectId>>,
    outputs: HashMap<wayland_client::backend::ObjectId, OutputGeometry>,
    output_handles: Vec<wl_output::WlOutput>,
    xdg_outputs: Vec<ZxdgOutputV1>,
    _xdg_manager: Option<ZxdgOutputManagerV1>,
    cosmic_manager: Option<ZcosmicWorkspaceManagerV2>,
    cosmic_workspaces: HashMap<wayland_client::backend::ObjectId, CosmicWorkspace>,
}

#[derive(Default)]
struct OutputGeometry {
    width: i32,
    height: i32,
}

struct CosmicWorkspace {
    handle: ZcosmicWorkspaceHandleV2,
    tiling: Option<WEnum<zcosmic_workspace_handle_v2::TilingState>>,
}

struct Workspace {
    handle: ExtWorkspaceHandleV1,
    name: String,
    active: bool,
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Catalog {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.groups.entry(workspace_group.id()).or_default();
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                if let Some(manager) = &state.cosmic_manager {
                    let cosmic = manager.get_cosmic_workspace(&workspace, qh, workspace.clone());
                    state.cosmic_workspaces.insert(
                        workspace.id(),
                        CosmicWorkspace {
                            handle: cosmic,
                            tiling: None,
                        },
                    );
                }
                state.workspaces.insert(
                    workspace.id(),
                    Workspace {
                        handle: workspace,
                        name: String::new(),
                        active: false,
                    },
                );
            }
            ext_workspace_manager_v1::Event::Done | ext_workspace_manager_v1::Event::Finished => {}
            _ => unreachable!("unknown ext workspace manager event"),
        }
    }

    wayland_client::event_created_child!(Catalog, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ())
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Catalog {
    fn event(
        state: &mut Self,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                state
                    .groups
                    .entry(group.id())
                    .or_default()
                    .insert(workspace.id());
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                if let Some(workspaces) = state.groups.get_mut(&group.id()) {
                    workspaces.remove(&workspace.id());
                }
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                state.groups.remove(&group.id());
                state.group_outputs.remove(&group.id());
            }
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => {
                state
                    .group_outputs
                    .entry(group.id())
                    .or_default()
                    .insert(output.id());
            }
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => {
                if let Some(outputs) = state.group_outputs.get_mut(&group.id()) {
                    outputs.remove(&output.id());
                }
            }
            ext_workspace_group_handle_v1::Event::Capabilities { .. } => {}
            _ => unreachable!("unknown ext workspace group event"),
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for Catalog {
    fn event(
        state: &mut Self,
        handle: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(workspace) = state.workspaces.get_mut(&handle.id()) else {
            return;
        };
        match event {
            ext_workspace_handle_v1::Event::Name { name } => workspace.name = name,
            ext_workspace_handle_v1::Event::State { state } => {
                let state = match state {
                    WEnum::Value(state) => state,
                    WEnum::Unknown(bits) => ext_workspace_handle_v1::State::from_bits_retain(bits),
                };
                workspace.active = state.contains(ext_workspace_handle_v1::State::Active);
            }
            ext_workspace_handle_v1::Event::Removed => {
                state.workspaces.remove(&handle.id());
            }
            ext_workspace_handle_v1::Event::Id { .. }
            | ext_workspace_handle_v1::Event::Coordinates { .. }
            | ext_workspace_handle_v1::Event::Capabilities { .. } => {}
            _ => unreachable!("unknown ext workspace event"),
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Catalog {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for Catalog {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for Catalog {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("zxdg_output_manager_v1 has no events")
    }
}

impl Dispatch<ZxdgOutputV1, wl_output::WlOutput> for Catalog {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        output: &wl_output::WlOutput,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let geometry = state.outputs.entry(output.id()).or_default();
        match event {
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                geometry.width = width;
                geometry.height = height;
            }
            zxdg_output_v1::Event::LogicalPosition { .. }
            | zxdg_output_v1::Event::Done
            | zxdg_output_v1::Event::Name { .. }
            | zxdg_output_v1::Event::Description { .. } => {}
            _ => unreachable!("unknown xdg output event"),
        }
    }
}

impl Dispatch<ZcosmicWorkspaceManagerV2, ()> for Catalog {
    fn event(
        _: &mut Self,
        _: &ZcosmicWorkspaceManagerV2,
        _: zcosmic_workspace_manager_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("zcosmic_workspace_manager_v2 has no events")
    }
}

impl Dispatch<ZcosmicWorkspaceHandleV2, ExtWorkspaceHandleV1> for Catalog {
    fn event(
        state: &mut Self,
        _: &ZcosmicWorkspaceHandleV2,
        event: zcosmic_workspace_handle_v2::Event,
        workspace: &ExtWorkspaceHandleV1,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_workspace_handle_v2::Event::TilingState { state: tiling } = event
            && let Some(workspace) = state.cosmic_workspaces.get_mut(&workspace.id())
        {
            workspace.tiling = Some(tiling);
        }
    }
}
