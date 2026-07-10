// SPDX-License-Identifier: MPL-2.0
use crate::input::mouse::{MouseWindowLocation, Shared};
use crate::input::{InputEventOrigin, Window};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use wayland_client::backend::ObjectId;

#[derive(Debug)]
pub(super) struct PlatformCoalescedMouse {}

#[derive(Default)]
struct MouseState {
    shareds: Vec<Weak<Shared>>,
    recent_x_pos: Option<f64>,
    recent_y_pos: Option<f64>,
    recent_window_width: Option<i32>,
    recent_window_height: Option<i32>,
    recent_window: Option<ObjectId>,
    window_sizes: BTreeMap<u32, (i32, i32)>,
}

impl MouseState {
    fn live_shareds(&mut self) -> Vec<Arc<Shared>> {
        let mut live = Vec::with_capacity(self.shareds.len());
        self.shareds.retain(|shared| {
            if let Some(shared) = shared.upgrade() {
                live.push(shared);
                true
            } else {
                false
            }
        });
        live
    }
    fn current_location(&self) -> Option<MouseWindowLocation> {
        if let (Some(recent_x_pos), Some(recent_y_pos)) = (self.recent_x_pos, self.recent_y_pos) {
            let window = match self.recent_window.as_ref() {
                None => None,
                Some(object_id) => NonNull::new(object_id.protocol_id() as *mut c_void).map(Window),
            };
            let (recent_window_width, recent_window_height) = self
                .recent_window
                .as_ref()
                .and_then(|object_id| self.window_sizes.get(&object_id.protocol_id()).copied())
                .or_else(|| Some((self.recent_window_width?, self.recent_window_height?)))
                .unwrap_or((1, 1));
            let pos = MouseWindowLocation::new(
                recent_x_pos,
                recent_y_pos,
                recent_window_width as f64,
                recent_window_height as f64,
                window,
            );
            Some(pos)
        } else {
            None
        }
    }
}

/**
Call this to handle [wayland_client::protocol::wl_pointer::Event::Motion].

Call this from your wayland dispatch queue.
*/
pub fn motion_event(_time: u32, surface_x: f64, surface_y: f64, window: ObjectId) {
    let (shareds, location) = {
        let mut state = MOUSE_STATE.get_or_init(Mutex::default).lock().unwrap();
        state.recent_x_pos = Some(surface_x);
        state.recent_y_pos = Some(surface_y);
        state.recent_window = Some(window);
        let location = state.current_location();
        (state.live_shareds(), location)
    };
    if let Some(location) = location {
        for shared in shareds {
            shared.set_window_location(location, InputEventOrigin::RealOs);
        }
    }
}

/// Clears the coalesced pointer position when Wayland reports that the pointer
/// left a surface. This lets retained hover state disappear without waiting for
/// a motion event in another window.
pub fn pointer_leave_event(window: ObjectId) {
    let shareds = {
        let mut state = MOUSE_STATE.get_or_init(Mutex::default).lock().unwrap();
        state.recent_x_pos = None;
        state.recent_y_pos = None;
        state.recent_window = None;
        state.live_shareds()
    };
    let window_ptr = window.protocol_id() as *mut c_void;
    for shared in shareds {
        shared.clear_window_location(window_ptr, InputEventOrigin::RealOs);
    }
}

/**
Call this to handle [wayland_protocols::xdg::shell::client::xdg_toplevel::Event::Configure].

Call this from your wayland dispatch queue.
*/
pub fn xdg_toplevel_configure_event(width: i32, height: i32) {
    let (shareds, location) = {
        let mut state = MOUSE_STATE.get_or_init(Mutex::default).lock().unwrap();
        state.recent_window_width = Some(width);
        state.recent_window_height = Some(height);
        let location = state.current_location();
        (state.live_shareds(), location)
    };
    if let Some(location) = location {
        for shared in shareds {
            shared.set_window_location(location, InputEventOrigin::RealOs);
        }
    }
}

pub fn xdg_toplevel_configure_event_for_window(width: i32, height: i32, window: ObjectId) {
    let (shareds, location) = {
        let mut state = MOUSE_STATE.get_or_init(Mutex::default).lock().unwrap();
        state.recent_window_width = Some(width);
        state.recent_window_height = Some(height);
        state
            .window_sizes
            .insert(window.protocol_id(), (width.max(1), height.max(1)));
        let location = state.current_location();
        (state.live_shareds(), location)
    };
    if let Some(location) = location {
        for shared in shareds {
            shared.set_window_location(location, InputEventOrigin::RealOs);
        }
    }
}

/**
Call this to handle wayland_client::protocol::wl_pointer::Event::Button.

Call this from your wayland dispatch queue.
*/
pub fn button_event(_time: u32, button: u32, state: u32, window: ObjectId) {
    let down = state != 0;
    let Some(btn_code) = button_code(button) else {
        logwise::warn_sync!("Unknown button code: {button}", button = button);
        return;
    };
    let shareds = MOUSE_STATE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .live_shareds();
    for shared in shareds {
        shared.set_key_state(
            btn_code,
            down,
            window.protocol_id() as *mut c_void,
            InputEventOrigin::RealOs,
        );
    }
}

pub(crate) fn button_code(button: u32) -> Option<u8> {
    // See linux/input-event-codes.h.
    Some(match button {
        0x110 => 0, //BTN_LEFT
        0x111 => 1, //BTN_RIGHT
        0x112 => 2, //BTN_MIDDLE
        0x113 => 3, //BTN_SIDE
        0x114 => 4, //BTN_EXTRA
        0x115 => 5, //BTN_FORWARD
        0x116 => 6, //BTN_BACK
        0x117 => 7, //BTN_TASK
        0x118 => 8,
        0x119 => 9,
        _ => return None,
    })
}

pub fn axis_event(_time: u32, axis: u32, value: f64, window: ObjectId) {
    let shareds = MOUSE_STATE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .live_shareds();
    if axis == 0 {
        for shared in shareds {
            shared.add_scroll_delta(
                0.0,
                value,
                window.protocol_id() as *mut c_void,
                InputEventOrigin::RealOs,
            );
        }
    } else {
        for shared in shareds {
            shared.add_scroll_delta(
                value,
                0.0,
                window.protocol_id() as *mut c_void,
                InputEventOrigin::RealOs,
            );
        }
    }
}

static MOUSE_STATE: OnceLock<Mutex<MouseState>> = OnceLock::new();

#[cfg(test)]
pub(super) fn reset_and_register_test_shared(shared: &Arc<Shared>) {
    let mut state = MOUSE_STATE.get_or_init(Mutex::default).lock().unwrap();
    *state = MouseState::default();
    state.shareds.push(Arc::downgrade(shared));
}

impl PlatformCoalescedMouse {
    pub(crate) fn attached() -> Self {
        Self {}
    }

    pub async fn new(shared: &Arc<Shared>) -> Self {
        MOUSE_STATE
            .get_or_init(Mutex::default)
            .lock()
            .unwrap()
            .shareds
            .push(Arc::downgrade(shared));
        PlatformCoalescedMouse {}
    }
}
