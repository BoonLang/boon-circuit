use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_compositor::WlCompositor as ClientCompositor;
use wayland_client::protocol::wl_seat::WlSeat as ClientSeat;
use wayland_client::protocol::wl_shm::WlShm as ClientShm;
use wayland_client::{Connection, Proxy};
use wayland_server::protocol::{
    wl_compositor::{self, WlCompositor},
    wl_keyboard::{self, WlKeyboard},
    wl_pointer::{self, WlPointer},
    wl_seat::{self, WlSeat},
    wl_shm::{self, WlShm},
    wl_surface::{self, WlSurface},
};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
};

use crate::coordinates::Size;
use crate::input::InputEventOrigin;
use crate::input::keyboard::key::KeyboardKey;
use crate::input::mouse::{MOUSE_BUTTON_LEFT, MOUSE_BUTTON_RIGHT};
use crate::sys::linux::cursor::{ActiveCursor, CursorRequest};
use crate::sys::linux::{App, AppState, Surface, SurfaceEvents};

const EMIT_FIRST: u8 = 1;
const EMIT_SECOND: u8 = 2;
const REMOVE_CAPABILITIES: u8 = 3;
const ASSERT_RELEASED: u8 = 4;
const READD_CAPABILITIES: u8 = 5;
const EMIT_READDED: u8 = 6;
const STOP: u8 = 255;

#[derive(Clone, Copy, Debug)]
struct SeatData(usize);

#[derive(Default)]
struct ProtocolServer {
    surfaces: Vec<WlSurface>,
    seats: Vec<WlSeat>,
    pointers: Vec<(usize, WlPointer)>,
    keyboards: Vec<(usize, WlKeyboard)>,
    pointer_release_count: usize,
    keyboard_release_count: usize,
}

impl ProtocolServer {
    fn pointer(&self, seat: usize) -> &WlPointer {
        &self
            .pointers
            .iter()
            .rev()
            .find(|(owner, _)| *owner == seat)
            .expect("missing pointer")
            .1
    }

    fn keyboard(&self, seat: usize) -> &WlKeyboard {
        &self
            .keyboards
            .iter()
            .rev()
            .find(|(owner, _)| *owner == seat)
            .expect("missing keyboard")
            .1
    }

    fn emit(&self, seat: usize, pressed: bool) {
        let surface = &self.surfaces[seat];
        let pointer = self.pointer(seat);
        let keyboard = self.keyboard(seat);
        let (x, y, button, key) = if seat == 0 {
            (51.0, 61.0, 0x110, 30)
        } else {
            (81.0, 91.0, 0x111, 31)
        };
        let serial = 100 + seat as u32 * 10 + u32::from(!pressed);
        pointer.enter(serial, surface, x, y);
        pointer.motion(serial + 1, x + 1.0, y + 1.0);
        pointer.button(
            serial + 2,
            serial + 3,
            button,
            if pressed {
                wl_pointer::ButtonState::Pressed
            } else {
                wl_pointer::ButtonState::Released
            },
        );
        keyboard.enter(serial + 4, surface, Vec::new());
        keyboard.key(
            serial + 5,
            serial + 6,
            key,
            if pressed {
                wl_keyboard::KeyState::Pressed
            } else {
                wl_keyboard::KeyState::Released
            },
        );
    }
}

impl GlobalDispatch<WlCompositor, ()> for ProtocolServer {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<WlCompositor>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlCompositor, ()> for ProtocolServer {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &WlCompositor,
        request: wl_compositor::Request,
        _data: &(),
        _handle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let wl_compositor::Request::CreateSurface { id } = request {
            state.surfaces.push(data_init.init(id, ()));
        }
    }
}

impl Dispatch<WlSurface, ()> for ProtocolServer {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &WlSurface,
        _request: wl_surface::Request,
        _data: &(),
        _handle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<WlShm, ()> for ProtocolServer {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<WlShm>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlShm, ()> for ProtocolServer {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &WlShm,
        _request: wl_shm::Request,
        _data: &(),
        _handle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }
}

impl GlobalDispatch<WlSeat, ()> for ProtocolServer {
    fn bind(
        state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<WlSeat>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let seat = data_init.init(resource, SeatData(state.seats.len()));
        seat.capabilities(wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard);
        state.seats.push(seat);
    }
}

impl Dispatch<WlSeat, SeatData> for ProtocolServer {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &WlSeat,
        request: wl_seat::Request,
        data: &SeatData,
        _handle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_seat::Request::GetPointer { id } => {
                state.pointers.push((data.0, data_init.init(id, *data)));
            }
            wl_seat::Request::GetKeyboard { id } => {
                let keyboard = data_init.init(id, *data);
                let keymap = tempfile::tempfile().unwrap();
                keyboard.keymap(wl_keyboard::KeymapFormat::NoKeymap, keymap.as_fd(), 0);
                state.keyboards.push((data.0, keyboard));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, SeatData> for ProtocolServer {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &WlPointer,
        request: wl_pointer::Request,
        _data: &SeatData,
        _handle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if matches!(request, wl_pointer::Request::Release) {
            state.pointer_release_count += 1;
        }
    }
}

impl Dispatch<WlKeyboard, SeatData> for ProtocolServer {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &WlKeyboard,
        request: wl_keyboard::Request,
        _data: &SeatData,
        _handle: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if matches!(request, wl_keyboard::Request::Release) {
            state.keyboard_release_count += 1;
        }
    }
}

fn run_server(mut display: Display<ProtocolServer>, mut control: UnixStream) {
    let mut state = ProtocolServer::default();
    loop {
        let mut poll_fds = [
            libc::pollfd {
                fd: display.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: control.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        assert!(unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as _, -1) } > 0);
        if poll_fds[0].revents & libc::POLLIN != 0 {
            display.dispatch_clients(&mut state).unwrap();
            display.flush_clients().unwrap();
        }
        if poll_fds[1].revents & libc::POLLIN != 0 {
            let mut command = [0_u8];
            control.read_exact(&mut command).unwrap();
            match command[0] {
                EMIT_FIRST => state.emit(0, true),
                EMIT_SECOND => state.emit(1, true),
                REMOVE_CAPABILITIES => {
                    for seat in &state.seats {
                        seat.capabilities(wl_seat::Capability::empty());
                    }
                }
                ASSERT_RELEASED => {
                    assert_eq!(state.pointer_release_count, 2);
                    assert_eq!(state.keyboard_release_count, 2);
                    assert!(
                        state
                            .pointers
                            .iter()
                            .all(|(_, pointer)| !pointer.is_alive())
                    );
                    assert!(
                        state
                            .keyboards
                            .iter()
                            .all(|(_, keyboard)| !keyboard.is_alive())
                    );
                }
                READD_CAPABILITIES => {
                    for seat in &state.seats {
                        seat.capabilities(
                            wl_seat::Capability::Pointer | wl_seat::Capability::Keyboard,
                        );
                    }
                }
                EMIT_READDED => {
                    assert_eq!(state.pointers.len(), 4);
                    assert_eq!(state.keyboards.len(), 4);
                    state.emit(0, false);
                    state.emit(1, false);
                }
                STOP => {}
                other => panic!("unknown server command {other}"),
            }
            display.flush_clients().unwrap();
            control.write_all(&command).unwrap();
            if command[0] == STOP {
                break;
            }
        }
    }
}

fn server_command(control: &mut UnixStream, command: u8) {
    control.write_all(&[command]).unwrap();
    let mut acknowledgement = [0_u8];
    control.read_exact(&mut acknowledgement).unwrap();
    assert_eq!(acknowledgement[0], command);
}

#[test]
fn wayland_event_queue_routes_input_per_surface_across_capability_readd() {
    let (server_socket, client_socket) = UnixStream::pair().unwrap();
    let (mut test_control, server_control) = UnixStream::pair().unwrap();
    let display = Display::<ProtocolServer>::new().unwrap();
    let mut display_handle = display.handle();
    display_handle.create_global::<ProtocolServer, WlCompositor, _>(6, ());
    display_handle.create_global::<ProtocolServer, WlShm, _>(1, ());
    display_handle.create_global::<ProtocolServer, WlSeat, _>(9, ());
    display_handle
        .insert_client(server_socket, Arc::new(()))
        .unwrap();
    let server = std::thread::spawn(move || run_server(display, server_control));

    let connection = Connection::from_socket(client_socket).unwrap();
    let (globals, mut event_queue) = registry_queue_init::<App>(&connection).unwrap();
    let queue_handle = event_queue.handle();
    let compositor: ClientCompositor = globals.bind(&queue_handle, 1..=6, ()).unwrap();
    let shm: ClientShm = globals.bind(&queue_handle, 1..=1, ()).unwrap();
    let app_state = Arc::new(AppState {
        compositor: compositor.clone(),
        shm,
        active_cursor: Mutex::new(None),
        seat: Mutex::new(None),
        outputs: Mutex::new(HashMap::new()),
        _decor: Vec::new(),
        decor_dimensions: (0, 0),
    });
    let mut app = App(Arc::clone(&app_state));
    let window_one = super::WindowInternal::new(
        &app_state,
        Size::new(200.0, 200.0),
        "one".to_owned(),
        &queue_handle,
        false,
    );
    let window_two = super::WindowInternal::new(
        &app_state,
        Size::new(200.0, 200.0),
        "two".to_owned(),
        &queue_handle,
        false,
    );
    let wl_surface_one = compositor.create_surface(
        &queue_handle,
        SurfaceEvents::Standard(Arc::clone(&window_one)),
    );
    let wl_surface_two = compositor.create_surface(
        &queue_handle,
        SurfaceEvents::Standard(Arc::clone(&window_two)),
    );
    window_one.lock().unwrap().wl_surface = Some(wl_surface_one.clone());
    window_two.lock().unwrap().wl_surface = Some(wl_surface_two.clone());
    let cursor_surface = compositor.create_surface(&queue_handle, SurfaceEvents::Cursor);
    let (cursor_sender, _cursor_receiver) = std::sync::mpsc::channel();
    app_state
        .active_cursor
        .lock()
        .unwrap()
        .replace(ActiveCursor {
            cursor_surface: Arc::new(cursor_surface),
            cursor_sender,
            active_request: Arc::new(Mutex::new(CursorRequest::left_ptr())),
        });
    let seat_one: ClientSeat = globals
        .bind(&queue_handle, 1..=9, Arc::clone(&window_one))
        .unwrap();
    let seat_two: ClientSeat = globals
        .bind(&queue_handle, 1..=9, Arc::clone(&window_two))
        .unwrap();
    app_state.seat.lock().unwrap().replace(seat_one.clone());

    event_queue.roundtrip(&mut app).unwrap();
    event_queue.roundtrip(&mut app).unwrap();
    let initial_device_ids = {
        let one = window_one.lock().unwrap();
        let two = window_two.lock().unwrap();
        (
            one.wl_pointer.as_ref().unwrap().id(),
            one.wl_keyboard.as_ref().unwrap().id(),
            two.wl_pointer.as_ref().unwrap().id(),
            two.wl_keyboard.as_ref().unwrap().id(),
        )
    };

    let surface_one = Surface {
        wl_display: connection.display(),
        wl_surface: wl_surface_one.clone(),
        window_internal: Arc::clone(&window_one),
    };
    let surface_two = Surface {
        wl_display: connection.display(),
        wl_surface: wl_surface_two.clone(),
        window_internal: Arc::clone(&window_two),
    };
    let (mouse_one, keyboard_one) = surface_one.input();
    let (mouse_two, keyboard_two) = surface_two.input();
    let mouse_one_wakes = Arc::new(AtomicUsize::new(0));
    let keyboard_one_wakes = Arc::new(AtomicUsize::new(0));
    let mouse_two_wakes = Arc::new(AtomicUsize::new(0));
    let keyboard_two_wakes = Arc::new(AtomicUsize::new(0));
    for (input, wakes) in [
        (&mouse_one, Arc::clone(&mouse_one_wakes)),
        (&mouse_two, Arc::clone(&mouse_two_wakes)),
    ] {
        input.on_input_event(move || {
            wakes.fetch_add(1, Ordering::Relaxed);
        });
    }
    for (input, wakes) in [
        (&keyboard_one, Arc::clone(&keyboard_one_wakes)),
        (&keyboard_two, Arc::clone(&keyboard_two_wakes)),
    ] {
        input.on_input_event(move || {
            wakes.fetch_add(1, Ordering::Relaxed);
        });
    }

    server_command(&mut test_control, EMIT_FIRST);
    event_queue.roundtrip(&mut app).unwrap();
    assert_eq!(mouse_one_wakes.load(Ordering::Relaxed), 3);
    assert_eq!(keyboard_one_wakes.load(Ordering::Relaxed), 1);
    assert_eq!(mouse_two_wakes.load(Ordering::Relaxed), 0);
    assert_eq!(keyboard_two_wakes.load(Ordering::Relaxed), 0);
    assert!(mouse_one.button_state(MOUSE_BUTTON_LEFT));
    assert!(!mouse_two.button_state(MOUSE_BUTTON_LEFT));
    assert!(keyboard_one.is_pressed(KeyboardKey::A));
    assert!(!keyboard_two.is_pressed(KeyboardKey::A));

    server_command(&mut test_control, EMIT_SECOND);
    event_queue.roundtrip(&mut app).unwrap();
    assert_eq!(mouse_one_wakes.load(Ordering::Relaxed), 3);
    assert_eq!(keyboard_one_wakes.load(Ordering::Relaxed), 1);
    assert_eq!(mouse_two_wakes.load(Ordering::Relaxed), 3);
    assert_eq!(keyboard_two_wakes.load(Ordering::Relaxed), 1);
    assert!(mouse_two.button_state(MOUSE_BUTTON_RIGHT));
    assert!(keyboard_two.is_pressed(KeyboardKey::S));

    for (provenance, surface_id) in [
        (
            mouse_one.event_provenance(),
            wl_surface_one.id().protocol_id(),
        ),
        (
            mouse_two.event_provenance(),
            wl_surface_two.id().protocol_id(),
        ),
    ] {
        assert!(provenance.recent_events.iter().all(|event| {
            event.origin == InputEventOrigin::RealOs
                && event.window_protocol_id == Some(u64::from(surface_id))
        }));
    }
    for (provenance, surface_id) in [
        (
            keyboard_one.event_provenance(),
            wl_surface_one.id().protocol_id(),
        ),
        (
            keyboard_two.event_provenance(),
            wl_surface_two.id().protocol_id(),
        ),
    ] {
        assert!(provenance.recent_events.iter().all(|event| {
            event.origin == InputEventOrigin::RealOs
                && event.window_protocol_id == Some(u64::from(surface_id))
        }));
    }

    server_command(&mut test_control, REMOVE_CAPABILITIES);
    event_queue.roundtrip(&mut app).unwrap();
    assert!(window_one.lock().unwrap().wl_pointer.is_none());
    assert!(window_one.lock().unwrap().wl_keyboard.is_none());
    assert!(window_two.lock().unwrap().wl_pointer.is_none());
    assert!(window_two.lock().unwrap().wl_keyboard.is_none());
    event_queue.roundtrip(&mut app).unwrap();
    server_command(&mut test_control, ASSERT_RELEASED);

    server_command(&mut test_control, READD_CAPABILITIES);
    event_queue.roundtrip(&mut app).unwrap();
    event_queue.roundtrip(&mut app).unwrap();
    {
        let one = window_one.lock().unwrap();
        let two = window_two.lock().unwrap();
        assert_ne!(one.wl_pointer.as_ref().unwrap().id(), initial_device_ids.0);
        assert_ne!(one.wl_keyboard.as_ref().unwrap().id(), initial_device_ids.1);
        assert_ne!(two.wl_pointer.as_ref().unwrap().id(), initial_device_ids.2);
        assert_ne!(two.wl_keyboard.as_ref().unwrap().id(), initial_device_ids.3);
    }

    server_command(&mut test_control, EMIT_READDED);
    event_queue.roundtrip(&mut app).unwrap();
    assert_eq!(mouse_one_wakes.load(Ordering::Relaxed), 6);
    assert_eq!(keyboard_one_wakes.load(Ordering::Relaxed), 2);
    assert_eq!(mouse_two_wakes.load(Ordering::Relaxed), 6);
    assert_eq!(keyboard_two_wakes.load(Ordering::Relaxed), 2);
    assert!(!mouse_one.button_state(MOUSE_BUTTON_LEFT));
    assert!(!mouse_two.button_state(MOUSE_BUTTON_RIGHT));
    assert!(!keyboard_one.is_pressed(KeyboardKey::A));
    assert!(!keyboard_two.is_pressed(KeyboardKey::S));

    drop((seat_one, seat_two));
    server_command(&mut test_control, STOP);
    server.join().unwrap();
}
