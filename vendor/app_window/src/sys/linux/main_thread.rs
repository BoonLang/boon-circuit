// SPDX-License-Identifier: MPL-2.0
use super::{App, AppState};
use crate::application::IS_MAIN_THREAD_RUNNING;
use libc::{
    EFD_SEMAPHORE, POLLERR, POLLHUP, POLLIN, POLLOUT, SYS_gettid, c_int, c_void, eventfd, getpid,
    pid_t, poll, pollfd, syscall,
};
use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, TryRecvError, channel};
use wayland_client::backend::WaylandError;
use wayland_client::globals::{GlobalList, registry_queue_init};
use wayland_client::protocol::wl_subcompositor::WlSubcompositor;
use wayland_client::protocol::{wl_compositor, wl_output::WlOutput, wl_shm::WlShm};
use wayland_client::{Connection, QueueHandle};

pub fn is_main_thread() -> bool {
    let current_pid = unsafe { getpid() };
    let main_thread_pid = unsafe { syscall(SYS_gettid) } as pid_t;
    current_pid == main_thread_pid
}

enum Message {
    Closure(Box<dyn FnOnce() + Send>),
    Stop,
}
struct MainThreadSender {
    sender: Sender<Message>,
    eventfd: c_int,
}

impl MainThreadSender {
    fn send(&self, message: Message) {
        self.sender.send(message).expect("Can't send closure");
        let val = 1_u64;
        let w = unsafe {
            libc::write(
                self.eventfd,
                &val as *const _ as *const c_void,
                std::mem::size_of_val(&val),
            )
        };
        assert_eq!(
            w,
            std::mem::size_of_val(&val) as isize,
            "Failed to write to eventfd: {err}",
            err = unsafe { *libc::__errno_location() }
        );
    }
}

static MAIN_THREAD_SENDER: OnceLock<MainThreadSender> = OnceLock::new();

pub(super) struct MainThreadInfo {
    pub globals: GlobalList,
    pub queue_handle: QueueHandle<App>,
    pub connection: Connection,
    pub app_state: std::sync::Arc<AppState>,
    pub subcompositor: WlSubcompositor,
}

thread_local! {
    pub static MAIN_THREAD_INFO: RefCell<Option<MainThreadInfo>> = const { RefCell::new(None) };
}

pub fn on_main_thread<F: FnOnce() + Send + 'static>(closure: F) {
    MAIN_THREAD_SENDER
        .get()
        .expect("Main thread sender not set")
        .send(Message::Closure(Box::new(closure)));
}

pub fn stop_main_thread() {
    MAIN_THREAD_SENDER
        .get()
        .expect("Main thread sender not set")
        .send(Message::Stop);
}

pub async fn alert(message: String) {
    todo!("alert not yet implemented for Linux: {}", message)
}

pub fn run_main_thread<F: FnOnce() + Send + 'static>(closure: F) {
    let (sender, receiver) = channel();
    let channel_read_event = unsafe { eventfd(0, EFD_SEMAPHORE) };
    assert_ne!(channel_read_event, -1, "Failed to create eventfd");
    MAIN_THREAD_SENDER.get_or_init(|| MainThreadSender {
        sender,
        eventfd: channel_read_event,
    });

    let connection = Connection::connect_to_env().expect("Failed to connect to wayland server");
    let (globals, mut event_queue) =
        registry_queue_init::<App>(&connection).expect("Can't initialize registry");
    let qh = event_queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 5..=6, ()).unwrap();
    let subcompositor: WlSubcompositor = globals.bind(&qh, 1..=1, ()).unwrap();
    //fedora 41 KDE uses version 1?
    let shm: WlShm = globals.bind(&qh, 1..=2, ()).unwrap();

    // Bind all available wl_output interfaces
    for global in globals.contents().clone_list() {
        if global.interface == "wl_output" {
            let _output: WlOutput = globals
                .bind(&qh, global.version..=global.version, global.name)
                .unwrap();
        }
    }

    let mut app = App(AppState::new(&qh, compositor, &connection, shm));
    let main_thread_info = MainThreadInfo {
        globals,
        queue_handle: qh,
        connection,
        app_state: app.0.clone(),
        subcompositor,
    };

    MAIN_THREAD_INFO.replace(Some(main_thread_info));
    _ = std::thread::Builder::new()
        .name("app_window closure".to_string())
        .spawn(closure);

    fn flush_or_defer(event_queue: &wayland_client::EventQueue<App>) -> bool {
        match event_queue.flush() {
            Ok(()) => false,
            Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => true,
            Err(error) => panic!("Failed to flush event queue: {error}"),
        }
    }

    let mut flush_pending = flush_or_defer(&event_queue);

    fn next_read_guard(
        event_queue: &mut wayland_client::EventQueue<App>,
        app: &mut App,
        flush_pending: &mut bool,
    ) -> wayland_client::backend::ReadEventsGuard {
        loop {
            let _read_guard = event_queue.prepare_read();

            match _read_guard {
                Some(guard) => {
                    break guard;
                }
                None => {
                    event_queue
                        .dispatch_pending(app)
                        .expect("Can't dispatch events");
                    *flush_pending = flush_or_defer(event_queue);
                    //try again
                    logwise::debuginternal_sync!("Retrying");
                }
            }
        }
    }

    //park
    loop {
        let read_guard = next_read_guard(&mut event_queue, &mut app, &mut flush_pending);
        let wayland_fd = read_guard.connection_fd().as_raw_fd();
        let mut fds = [
            pollfd {
                fd: wayland_fd,
                events: POLLIN | if flush_pending { POLLOUT } else { 0 },
                revents: 0,
            },
            pollfd {
                fd: channel_read_event,
                events: POLLIN,
                revents: 0,
            },
        ];
        loop {
            let result = unsafe { poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
            if result >= 0 {
                break;
            }
            let errno = unsafe { *libc::__errno_location() };
            if errno != libc::EINTR {
                panic!("Error polling main thread fds: {errno}");
            }
        }
        let wayland_events = fds[0].revents;
        let channel_events = fds[1].revents;
        let wayland_data_available = wayland_events & (POLLIN | POLLERR | POLLHUP) != 0;
        let wayland_writable = wayland_events & POLLOUT != 0;
        let channel_data_available = channel_events & (POLLIN | POLLERR | POLLHUP) != 0;
        if wayland_data_available {
            match read_guard.read() {
                Ok(_) => {}
                Err(e) => {
                    match e {
                        WaylandError::Io(e) => {
                            match e.kind() {
                                std::io::ErrorKind::WouldBlock => {
                                    //continue
                                }
                                _ => {
                                    panic!("Error reading from wayland: {err}", err = e);
                                }
                            }
                        }
                        WaylandError::Protocol(_) => {
                            panic!("Protocol error reading from wayland");
                        }
                    }
                }
            }
            event_queue
                .dispatch_pending(&mut app)
                .expect("Can't dispatch events");
            //prepare next read
            //ensure writes queued during dispatch_pending go out (such as proxy replies, etc)
            flush_pending = flush_or_defer(&event_queue);
        } else {
            drop(read_guard);
            if wayland_writable {
                flush_pending = flush_or_defer(&event_queue);
            }
        }
        if channel_data_available {
            let mut buf = [0u8; 8];
            let r = unsafe { libc::read(channel_read_event, buf.as_mut_ptr() as *mut c_void, 8) };
            assert_eq!(r, 8, "Failed to read from eventfd");
            match receiver.try_recv() {
                Ok(Message::Closure(closure)) => closure(),
                Ok(Message::Stop) => {
                    IS_MAIN_THREAD_RUNNING.store(false, Ordering::Relaxed);
                    return;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    IS_MAIN_THREAD_RUNNING.store(false, Ordering::Relaxed);
                    return;
                }
            }
            //let's ensure any writes went out to wayland
            event_queue
                .dispatch_pending(&mut app)
                .expect("can't dispatch events");
            flush_pending = flush_or_defer(&event_queue);
        }
    }
}
