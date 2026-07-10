// SPDX-License-Identifier: MPL-2.0
use std::sync::{Arc, Mutex};
use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_buffer::{Event, WlBuffer};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_keyboard::WlKeyboard;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::protocol::wl_subsurface::WlSubsurface;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel};

use super::ax;
use super::buffer::AllocatedBuffer;
use super::cursor::{CursorRequest, MouseRegion};
use super::{App, BufferReleaseInfo, Configure, OutputInfo, SurfaceEvents};
use crate::coordinates::Position;
use crate::input::InputEventOrigin;
use crate::input::mouse::MouseWindowLocation;
use crate::sys::window::{ConfigureContentBufferAction, WindowInternal};

enum PendingSurfaceMouseInput {
    Motion {
        time: u32,
        location: MouseWindowLocation,
    },
    Leave,
    Button {
        time: u32,
        button: u32,
        state: u32,
    },
    Axis {
        time: u32,
        axis: u32,
        value: f64,
    },
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _qh: &QueueHandle<App>,
    ) {
        logwise::debuginternal_sync!(
            "Got registry event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        _state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _qh: &QueueHandle<App>,
    ) {
        logwise::debuginternal_sync!(
            "Got registry event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<XdgWmBase, ()> for App {
    fn event(
        _state: &mut Self,
        proxy: &XdgWmBase,
        event: <XdgWmBase as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wayland_protocols::xdg::shell::client::xdg_wm_base::Event::Ping { serial } => {
                proxy.pong(serial);
            }
            _ => {
                logwise::debuginternal_sync!(
                    "Unknown XdgWmBase event {event}",
                    event = logwise::privacy::LogIt(&event)
                );
            }
        }
    }
}

impl Dispatch<WlCompositor, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlCompositor,
        event: <WlCompositor as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "Got compositor event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<WlShm, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlShm,
        event: <WlShm as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "Got WlShm event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<WlSurface, SurfaceEvents> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlSurface,
        event: <WlSurface as Proxy>::Event,
        data: &SurfaceEvents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wayland_client::protocol::wl_surface::Event::Enter { output } => {
                if let SurfaceEvents::Standard(window_internal) = data {
                    let output_id = output.id().protocol_id();
                    window_internal
                        .lock()
                        .unwrap()
                        .current_outputs
                        .insert(output_id);
                }
            }
            wayland_client::protocol::wl_surface::Event::Leave { output } => {
                if let SurfaceEvents::Standard(window_internal) = data {
                    let output_id = output.id().protocol_id();
                    window_internal
                        .lock()
                        .unwrap()
                        .current_outputs
                        .remove(&output_id);
                }
            }
            _ => {
                logwise::debuginternal_sync!(
                    "Got WlSurface event {event}",
                    event = logwise::privacy::LogIt(&event)
                );
            }
        }
    }
}

impl Dispatch<XdgSurface, Arc<Mutex<WindowInternal>>> for App {
    fn event(
        _state: &mut Self,
        proxy: &XdgSurface,
        event: <XdgSurface as Proxy>::Event,
        data: &Arc<Mutex<WindowInternal>>,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let mut accessibility_update = None;
        let mut locked_data = data.as_ref().lock().unwrap();
        match event {
            xdg_surface::Event::Configure { serial } => {
                let proposed = locked_data.proposed_configure.take();
                if let Some(mut configure) = proposed {
                    let app_state = locked_data.app_state.upgrade().unwrap();
                    if configure.width == 0 && configure.height == 0 {
                        // xdg-shell uses 0x0 to let the client pick its own size.
                        // Keep the size requested by Window::new instead of falling
                        // back to the library demo default.
                        if let Some(applied) = locked_data.applied_configure.as_ref() {
                            configure.width = applied.width;
                            configure.height = applied.height;
                        }
                    }
                    //check size (always attach on first configure)
                    let size_changed = locked_data
                        .applied_configure
                        .as_ref()
                        .map(|c| c.width != configure.width || c.height != configure.height)
                        .unwrap_or(true);
                    if !locked_data.has_been_configured || size_changed {
                        //apply decor position
                        locked_data
                            .decor_subsurface
                            .as_ref()
                            .unwrap()
                            .set_position(configure.width - app_state.decor_dimensions.0 as i32, 0);
                        locked_data.applied_configure = Some(configure);
                        let title = locked_data.title.clone();
                        let applied_size = locked_data.applied_size();
                        accessibility_update = Some((
                            Arc::clone(&locked_data.adapter),
                            ax::build_tree_update(title, applied_size),
                        ));
                        if let Some(f) = locked_data.size_update_notify.as_ref() {
                            f.0(locked_data.applied_size())
                        }

                        match locked_data.configure_content_buffer_action() {
                            ConfigureContentBufferAction::AttachShmBuffer => {
                                // rebuild main buffer
                                let buffer = AllocatedBuffer::new(
                                    locked_data.applied_configure.as_ref().unwrap().width,
                                    locked_data.applied_configure.as_ref().unwrap().height,
                                    &app_state.shm,
                                    qh,
                                    data.clone(),
                                );
                                // attach to surface
                                locked_data.wl_surface.as_ref().expect("No surface").attach(
                                    Some(&buffer.buffer),
                                    0,
                                    0,
                                );
                                locked_data.note_shm_content_attach();
                                // ack_configure MUST come before commit per xdg-shell protocol
                                proxy.ack_configure(serial);
                                locked_data.has_been_configured = true;
                                locked_data
                                    .wl_surface
                                    .as_ref()
                                    .expect("No surface")
                                    .commit();
                            }
                            ConfigureContentBufferAction::SkipExternalSurfaceOwner => {
                                locked_data.note_external_surface_configure_skip();
                                // ack_configure MUST come before the next WGPU surface commit.
                                proxy.ack_configure(serial);
                                locked_data.has_been_configured = true;
                            }
                        }
                    } else {
                        // No buffer changes needed, but still must ack
                        proxy.ack_configure(serial);
                        locked_data.has_been_configured = true;
                    }
                } else {
                    // No proposed configure, still ack
                    proxy.ack_configure(serial);
                    locked_data.has_been_configured = true;
                }
            }
            _ => {
                logwise::debuginternal_sync!(
                    "got XdgSurface_shm_buffer event {event}",
                    event = logwise::privacy::LogIt(&event)
                );
            }
        }
        drop(locked_data);
        if let Some((adapter, update)) = accessibility_update
            && let Some(adapter) = adapter.lock().unwrap().as_mut()
        {
            adapter.update_if_active(|| update);
        }
    }
}

impl<A: AsRef<Mutex<WindowInternal>>> Dispatch<XdgToplevel, A> for App {
    fn event(
        _state: &mut Self,
        _proxy: &XdgToplevel,
        event: <XdgToplevel as Proxy>::Event,
        data: &A,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "Got XdgToplevel event {event}",
            event = logwise::privacy::LogIt(&event)
        );
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states: _,
            } => {
                let mut window = data.as_ref().lock().unwrap();
                if let Some(surface) = window.wl_surface.as_ref() {
                    crate::input::linux::xdg_toplevel_configure_event_for_window(
                        width,
                        height,
                        surface.id(),
                    );
                } else {
                    crate::input::linux::xdg_toplevel_configure_event(width, height);
                }
                window.proposed_configure = Some(Configure { width, height });
            }
            _ => {
                //?
            }
        }
    }
}

impl Dispatch<WlShmPool, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlShmPool,
        event: <WlShmPool as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "Got WlshmPool event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<WlBuffer, BufferReleaseInfo> for App {
    fn event(
        _state: &mut Self,
        proxy: &WlBuffer,
        event: <WlBuffer as Proxy>::Event,
        data: &BufferReleaseInfo,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "Got WlBuffer event {event}",
            event = logwise::privacy::LogIt(&event)
        );
        match event {
            Event::Release => {
                if data.decor {
                    proxy.destroy();
                    return;
                }
                let release = data.opt.lock().unwrap().take().expect("No release info");
                let buf = release.allocated_buffer.expect("No allocated buffer");

                let mut lock = release.window_internal.lock().unwrap();
                if lock.external_surface_created {
                    proxy.destroy();
                    return;
                }
                if buf.width == lock.applied_configure.as_ref().unwrap().width
                    && buf.height == lock.applied_configure.as_ref().unwrap().height
                {
                    //re-use the buffer
                    lock.drawable_buffer = Some(buf);
                } else {
                    //discard the buffer
                    proxy.destroy();
                }
            }
            _ => { /* not implemented yet */ }
        }
    }
}

impl Dispatch<WlSeat, Arc<Mutex<WindowInternal>>> for App {
    fn event(
        _state: &mut Self,
        proxy: &WlSeat,
        event: <WlSeat as Proxy>::Event,
        data: &Arc<Mutex<WindowInternal>>,
        _conn: &Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities { capabilities } => {
                let capabilities = wl_seat::Capability::from_bits_truncate(capabilities.into());
                let mut window = data.lock().unwrap();
                if capabilities.contains(wl_seat::Capability::Pointer) {
                    if window.wl_pointer.is_none() {
                        window.wl_pointer = Some(proxy.get_pointer(qhandle, Arc::clone(data)));
                    }
                } else if let Some(pointer) = window.wl_pointer.take()
                    && pointer.version() >= 3
                {
                    pointer.release();
                }
                if capabilities.contains(wl_seat::Capability::Keyboard) {
                    if window.wl_keyboard.is_none() {
                        window.wl_keyboard = Some(proxy.get_keyboard(qhandle, Arc::clone(data)));
                    }
                } else if let Some(keyboard) = window.wl_keyboard.take()
                    && keyboard.version() >= 3
                {
                    keyboard.release();
                }
            }
            _ => {
                logwise::debuginternal_sync!(
                    "Got WlSeat event {event}",
                    event = logwise::privacy::LogIt(&event)
                );
            }
        }
    }
}

impl Dispatch<WlSubcompositor, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlSubcompositor,
        event: <WlSubcompositor as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "Got WlSubcompositor event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<WlSubsurface, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlSubsurface,
        event: <WlSubsurface as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "got WlSubsurface event {event}",
            event = logwise::privacy::LogIt(&event)
        );
    }
}

impl Dispatch<WlOutput, u32> for App {
    fn event(
        state: &mut Self,
        _proxy: &WlOutput,
        event: <WlOutput as Proxy>::Event,
        output_id: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wayland_client::protocol::wl_output::Event::Scale { factor } => {
                let mut outputs = state.0.outputs.lock().unwrap();
                if let Some(output_info) = outputs.get_mut(output_id) {
                    output_info.scale_factor = factor as f64;
                } else {
                    outputs.insert(
                        *output_id,
                        OutputInfo {
                            scale_factor: factor as f64,
                        },
                    );
                }
            }
            wayland_client::protocol::wl_output::Event::Done => {
                // Output configuration is complete
            }
            _ => {
                // Handle other output events if needed (geometry, mode, etc.)
            }
        }
    }
}

impl<A: AsRef<Mutex<WindowInternal>>> Dispatch<WlPointer, A> for App {
    fn event(
        _state: &mut Self,
        proxy: &WlPointer,
        event: <WlPointer as Proxy>::Event,
        data: &A,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let mut data = data.as_ref().lock().unwrap();
        let surface_mouse = Arc::clone(&data.surface_mouse);
        let window_id = data.wl_surface.as_ref().expect("No surface").id();
        let mut pending_input = None;
        match event {
            wayland_client::protocol::wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } => {
                data.wl_pointer_enter_serial = Some(serial);
                data.wl_pointer_enter_surface = Some(surface);
                let position = Position::new(surface_x, surface_y);
                data.wl_pointer_pos.replace(position);
                let size = data.applied_size();
                pending_input = Some(PendingSurfaceMouseInput::Motion {
                    time: 0,
                    location: MouseWindowLocation::for_window_protocol_id(
                        surface_x,
                        surface_y,
                        size.width(),
                        size.height(),
                        u64::from(window_id.protocol_id()),
                    ),
                });
                //set cursor?
                let app = data.app_state.upgrade().expect("App state gone");
                let cursor_request = app
                    .active_cursor
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .active_request
                    .lock()
                    .unwrap()
                    .clone();

                proxy.set_cursor(
                    serial,
                    Some(
                        &app.active_cursor
                            .lock()
                            .unwrap()
                            .as_ref()
                            .unwrap()
                            .cursor_surface,
                    ),
                    cursor_request.hot_x,
                    cursor_request.hot_y,
                );
            }
            wayland_client::protocol::wl_pointer::Event::Leave { .. } => {
                pending_input = Some(PendingSurfaceMouseInput::Leave);
                data.wl_pointer_enter_surface = None;
                data.wl_pointer_pos = None;
            }
            wayland_client::protocol::wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                time: _time,
            } => {
                let parent_surface_x;
                let parent_surface_y;
                if data.wl_pointer_enter_surface != data.wl_surface {
                    //we're in the decor; slide by decor dimensions
                    let surface_dimensions = data
                        .applied_configure
                        .clone()
                        .expect("No surface dimensions");
                    parent_surface_x = surface_x + surface_dimensions.width as f64
                        - data.app_state.upgrade().unwrap().decor_dimensions.0 as f64;
                    parent_surface_y = surface_y;
                } else {
                    parent_surface_x = surface_x;
                    parent_surface_y = surface_y;
                }
                //get current size
                let size = data.applied_size();
                pending_input = Some(PendingSurfaceMouseInput::Motion {
                    time: _time,
                    location: MouseWindowLocation::for_window_protocol_id(
                        parent_surface_x,
                        parent_surface_y,
                        size.width(),
                        size.height(),
                        u64::from(window_id.protocol_id()),
                    ),
                });
                let position = Position::new(parent_surface_x, parent_surface_y);
                data.wl_pointer_pos.replace(position);
                let cursor_request = match MouseRegion::from_position(size, position) {
                    MouseRegion::BottomRight => CursorRequest::bottom_right_corner(),
                    MouseRegion::Bottom => CursorRequest::bottom_side(),
                    MouseRegion::Right => CursorRequest::right_side(),
                    MouseRegion::Client => data.client_cursor_request(),
                    MouseRegion::MaximizeButton
                    | MouseRegion::CloseButton
                    | MouseRegion::MinimizeButton => CursorRequest::left_ptr(),
                    MouseRegion::Titlebar => CursorRequest::left_ptr(),
                };
                let app_state = data.app_state.upgrade().unwrap();
                let lock_a = app_state.active_cursor.lock().unwrap();
                let active_cursor = lock_a.as_ref().expect("No active cursor");
                let active_request = active_cursor.active_request.lock().unwrap();
                let changed = *active_request != cursor_request;
                if changed {
                    proxy.set_cursor(
                        data.wl_pointer_enter_serial.expect("No serial"),
                        Some(&active_cursor.cursor_surface),
                        cursor_request.hot_x,
                        cursor_request.hot_y,
                    );
                    active_cursor.cursor_request(cursor_request);
                }
            }
            wayland_client::protocol::wl_pointer::Event::Button {
                serial,
                time: _time,
                button,
                state,
            } => {
                let pressed: u32 = state.into();
                pending_input = Some(PendingSurfaceMouseInput::Button {
                    time: _time,
                    button,
                    state: pressed,
                });

                //get current size
                let size = data.applied_size();
                let mouse_pos = data.wl_pointer_pos.expect("No pointer position");
                let mouse_region = MouseRegion::from_position(size, mouse_pos);
                if button == 0x110 {
                    //BUTTON_LEFT
                    if pressed == 1 {
                        match mouse_region {
                            MouseRegion::BottomRight => {
                                let toplevel = data.xdg_toplevel.as_ref().unwrap();
                                let app_state = data.app_state.upgrade().unwrap();
                                let seat = app_state.seat.lock().unwrap();
                                toplevel.resize(
                                    seat.as_ref().unwrap(),
                                    serial,
                                    xdg_toplevel::ResizeEdge::BottomRight,
                                );
                            }
                            MouseRegion::Bottom => {
                                let toplevel = data.xdg_toplevel.as_ref().unwrap();
                                let app_state = data.app_state.upgrade().unwrap();
                                let seat = app_state.seat.lock().unwrap();
                                toplevel.resize(
                                    seat.as_ref().unwrap(),
                                    serial,
                                    xdg_toplevel::ResizeEdge::Bottom,
                                );
                            }
                            MouseRegion::Right => {
                                let toplevel = data.xdg_toplevel.as_ref().unwrap();
                                let app_state = data.app_state.upgrade().unwrap();
                                let seat = app_state.seat.lock().unwrap();
                                toplevel.resize(
                                    seat.as_ref().unwrap(),
                                    serial,
                                    xdg_toplevel::ResizeEdge::Right,
                                );
                            }
                            MouseRegion::Client => {}
                            MouseRegion::Titlebar => {
                                let toplevel = data.xdg_toplevel.as_ref().unwrap();
                                let app_state = data.app_state.upgrade().unwrap();
                                let seat = app_state.seat.lock().unwrap();
                                toplevel._move(seat.as_ref().unwrap(), serial);
                            }
                            MouseRegion::CloseButton => {
                                data.close_window();
                            }
                            MouseRegion::MaximizeButton => data.maximize(),
                            MouseRegion::MinimizeButton => {
                                data.minimize();
                            }
                        }
                    }
                }
            }
            wayland_client::protocol::wl_pointer::Event::Axis {
                time: _time,
                axis,
                value,
            } => {
                pending_input = Some(PendingSurfaceMouseInput::Axis {
                    time: _time,
                    axis: axis.into(),
                    value,
                });
            }
            _ => {
                //?
            }
        }
        drop(data);

        let window_ptr = window_id.protocol_id() as usize as *mut std::ffi::c_void;
        match pending_input {
            Some(PendingSurfaceMouseInput::Motion { time, location }) => {
                surface_mouse.set_window_location(location, InputEventOrigin::RealOs);
                crate::input::linux::motion_event(
                    time,
                    location.pos_x(),
                    location.pos_y(),
                    window_id,
                );
            }
            Some(PendingSurfaceMouseInput::Leave) => {
                surface_mouse.clear_window_location(window_ptr, InputEventOrigin::RealOs);
                crate::input::linux::pointer_leave_event(window_id);
            }
            Some(PendingSurfaceMouseInput::Button {
                time,
                button,
                state,
            }) => {
                if let Some(button_code) = crate::input::mouse::linux::button_code(button) {
                    surface_mouse.set_key_state(
                        button_code,
                        state != 0,
                        window_ptr,
                        InputEventOrigin::RealOs,
                    );
                }
                crate::input::linux::button_event(time, button, state, window_id);
            }
            Some(PendingSurfaceMouseInput::Axis { time, axis, value }) => {
                let (delta_x, delta_y) = if axis == 0 {
                    (0.0, value)
                } else {
                    (value, 0.0)
                };
                surface_mouse.add_scroll_delta(
                    delta_x,
                    delta_y,
                    window_ptr,
                    InputEventOrigin::RealOs,
                );
                crate::input::linux::axis_event(time, axis, value, window_id);
            }
            None => {}
        }
    }
}

impl<A: AsRef<Mutex<WindowInternal>>> Dispatch<WlKeyboard, A> for App {
    fn event(
        _state: &mut Self,
        _proxy: &WlKeyboard,
        event: <WlKeyboard as Proxy>::Event,
        data: &A,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        logwise::debuginternal_sync!(
            "got WlKeyboard event {event}",
            event = logwise::privacy::LogIt(&event)
        );
        let mut pending_key = None;
        match event {
            wayland_client::protocol::wl_keyboard::Event::Enter {
                serial: _,
                surface: _,
                keys: _,
            } => {
                let adapter = Arc::clone(&data.as_ref().lock().unwrap().adapter);
                if let Some(e) = adapter.lock().unwrap().as_mut() {
                    e.update_window_focus_state(true)
                }
            }
            wayland_client::protocol::wl_keyboard::Event::Leave {
                serial: _,
                surface: _,
            } => {
                let adapter = Arc::clone(&data.as_ref().lock().unwrap().adapter);
                if let Some(e) = adapter.lock().unwrap().as_mut() {
                    e.update_window_focus_state(false)
                }
            }
            wayland_client::protocol::wl_keyboard::Event::Key {
                serial: _serial,
                time: _time,
                key: _key,
                state: _state,
            } => {
                pending_key = Some((_serial, _time, _key, _state.into()));
            }
            _ => {}
        }
        if let Some((serial, time, key, state)) = pending_key {
            let (surface_keyboard, surface_id) = {
                let window = data.as_ref().lock().unwrap();
                (
                    Arc::clone(&window.surface_keyboard),
                    window.wl_surface.as_ref().expect("No surface").id(),
                )
            };
            crate::input::keyboard::linux::wl_keyboard_event_for_shared(
                &surface_keyboard,
                key,
                state,
                &surface_id,
            );
            crate::input::linux::wl_keyboard_event(serial, time, key, state, surface_id);
        }
    }
}
