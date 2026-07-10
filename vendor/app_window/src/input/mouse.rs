// SPDX-License-Identifier: MPL-2.0
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(target_os = "macos")]
pub(crate) use macos as sys;
use std::ffi::c_void;
use std::fmt;
use std::hash::{Hash, Hasher};

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm as sys;

#[cfg(target_os = "windows")]
pub(crate) use windows as sys;

#[cfg(target_os = "linux")]
pub(crate) use linux as sys;

use crate::application::is_main_thread_running;
use crate::input::{InputEventOrigin, Window};
use atomic_float::AtomicF64;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::time::Instant;

type InputWakeCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// Mouse button constant for the left mouse button.
///
/// # Examples
///
/// ```
/// # async fn example() {
/// use app_window::input::mouse::{Mouse, MOUSE_BUTTON_LEFT};
///
/// let mouse = Mouse::coalesced().await;
/// let left_pressed = mouse.button_state(MOUSE_BUTTON_LEFT);
/// # }
/// ```
pub const MOUSE_BUTTON_LEFT: u8 = 0;

/// Mouse button constant for the right mouse button.
///
/// # Examples
///
/// ```
/// # async fn example() {
/// use app_window::input::mouse::{Mouse, MOUSE_BUTTON_RIGHT};
///
/// let mouse = Mouse::coalesced().await;
/// let right_pressed = mouse.button_state(MOUSE_BUTTON_RIGHT);
/// # }
/// ```
pub const MOUSE_BUTTON_RIGHT: u8 = 1;

/// Mouse button constant for the middle mouse button (wheel button).
///
/// # Examples
///
/// ```
/// # async fn example() {
/// use app_window::input::mouse::{Mouse, MOUSE_BUTTON_MIDDLE};
///
/// let mouse = Mouse::coalesced().await;
/// let middle_pressed = mouse.button_state(MOUSE_BUTTON_MIDDLE);
/// # }
/// ```
pub const MOUSE_BUTTON_MIDDLE: u8 = 2;

/// Mouse's location within a window, in points.
///
/// The coordinate system has its origin at the upper-left corner of the window.
/// The position is reported in logical points, not physical pixels.
///
/// # Examples
///
/// ```
/// # async fn example() {
/// use app_window::input::mouse::Mouse;
///
/// let mouse = Mouse::coalesced().await;
/// if let Some(location) = mouse.window_pos() {
///     println!("Mouse at ({}, {})", location.pos_x(), location.pos_y());
///     println!("Window size: {}x{}", location.window_width(), location.window_height());
/// }
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseWindowLocation {
    pos_x: f64,
    pos_y: f64,
    window_width: f64,
    window_height: f64,
    window: Option<Window>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseEventProvenance {
    pub last_window_protocol_id: Option<u64>,
    pub motion_event_count: u64,
    pub button_event_count: u64,
    pub scroll_event_count: u64,
    pub total_event_count: u64,
    pub recent_button_events: Vec<MouseButtonEventRecord>,
    pub recent_events: Vec<MouseInputEventRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseInputEventKind {
    Motion,
    Button { button: u8, pressed: bool },
    Scroll { delta_x: f64, delta_y: f64 },
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseInputEventRecord {
    pub sequence: u64,
    pub kind: MouseInputEventKind,
    pub origin: InputEventOrigin,
    pub window_protocol_id: Option<u64>,
    pub window_position: Option<MouseWindowLocation>,
    pub occurred_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseButtonEventRecord {
    pub sequence: u64,
    pub button: u8,
    pub pressed: bool,
    pub window_protocol_id: Option<u64>,
    pub origin: InputEventOrigin,
    /// Pointer position captured at the button edge, before later motion can
    /// overwrite the coalesced cursor position.
    pub window_position: Option<MouseWindowLocation>,
    pub occurred_at: Instant,
}

impl MouseWindowLocation {
    pub(crate) fn new(
        pos_x: f64,
        pos_y: f64,
        window_width: f64,
        window_height: f64,
        window: Option<Window>,
    ) -> Self {
        MouseWindowLocation {
            pos_x,
            pos_y,
            window_width,
            window_height,
            window,
        }
    }

    pub(crate) fn for_window_protocol_id(
        pos_x: f64,
        pos_y: f64,
        window_width: f64,
        window_height: f64,
        window_protocol_id: u64,
    ) -> Self {
        let window = std::ptr::NonNull::new(window_protocol_id as usize as *mut c_void).map(Window);
        Self::new(pos_x, pos_y, window_width, window_height, window)
    }

    /// Returns the X coordinate of the mouse position within the window.
    ///
    /// The X coordinate is measured from the left edge of the window.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::input::mouse::Mouse;
    /// # let mouse = Mouse::coalesced().await;
    /// if let Some(location) = mouse.window_pos() {
    ///     let x = location.pos_x();
    ///     println!("Mouse X: {}", x);
    /// }
    /// # }
    /// ```
    pub fn pos_x(&self) -> f64 {
        self.pos_x
    }

    /// Returns the Y coordinate of the mouse position within the window.
    ///
    /// The Y coordinate is measured from the top edge of the window.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::input::mouse::Mouse;
    /// # let mouse = Mouse::coalesced().await;
    /// if let Some(location) = mouse.window_pos() {
    ///     let y = location.pos_y();
    ///     println!("Mouse Y: {}", y);
    /// }
    /// # }
    /// ```
    pub fn pos_y(&self) -> f64 {
        self.pos_y
    }

    /// Returns the width of the window containing the mouse.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::input::mouse::Mouse;
    /// # let mouse = Mouse::coalesced().await;
    /// if let Some(location) = mouse.window_pos() {
    ///     let width = location.window_width();
    ///     println!("Window width: {}", width);
    /// }
    /// # }
    /// ```
    pub fn window_width(&self) -> f64 {
        self.window_width
    }

    /// Returns the height of the window containing the mouse.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::input::mouse::Mouse;
    /// # let mouse = Mouse::coalesced().await;
    /// if let Some(location) = mouse.window_pos() {
    ///     let height = location.window_height();
    ///     println!("Window height: {}", height);
    /// }
    /// # }
    /// ```
    pub fn window_height(&self) -> f64 {
        self.window_height
    }
}

pub(crate) struct Shared {
    window: std::sync::Mutex<Option<MouseWindowLocation>>,

    buttons: [AtomicBool; 255],
    scroll_delta_x: AtomicF64,
    scroll_delta_y: AtomicF64,
    last_window: AtomicPtr<c_void>,
    last_window_protocol_id: AtomicU64,
    motion_event_count: AtomicU64,
    button_event_count: AtomicU64,
    scroll_event_count: AtomicU64,
    total_event_count: AtomicU64,
    recent_button_events: Mutex<VecDeque<MouseButtonEventRecord>>,
    input_event_count: AtomicU64,
    recent_events: Mutex<VecDeque<MouseInputEventRecord>>,
    input_wake_callbacks: Mutex<Vec<InputWakeCallback>>,
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared")
            .field(
                "last_window_protocol_id",
                &self.last_window_protocol_id.load(Ordering::Relaxed),
            )
            .field(
                "motion_event_count",
                &self.motion_event_count.load(Ordering::Relaxed),
            )
            .field(
                "button_event_count",
                &self.button_event_count.load(Ordering::Relaxed),
            )
            .field(
                "scroll_event_count",
                &self.scroll_event_count.load(Ordering::Relaxed),
            )
            .field(
                "total_event_count",
                &self.total_event_count.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl Shared {
    pub(crate) fn new() -> Self {
        Shared {
            window: std::sync::Mutex::new(None),
            buttons: [const { AtomicBool::new(false) }; 255],
            scroll_delta_x: AtomicF64::new(0.0),
            scroll_delta_y: AtomicF64::new(0.0),
            last_window: AtomicPtr::new(std::ptr::null_mut()),
            last_window_protocol_id: AtomicU64::new(0),
            motion_event_count: AtomicU64::new(0),
            button_event_count: AtomicU64::new(0),
            scroll_event_count: AtomicU64::new(0),
            total_event_count: AtomicU64::new(0),
            recent_button_events: Mutex::new(VecDeque::new()),
            input_event_count: AtomicU64::new(0),
            recent_events: Mutex::new(VecDeque::new()),
            input_wake_callbacks: Mutex::new(Vec::new()),
        }
    }

    fn notify_input_event(&self) {
        let callbacks = self.input_wake_callbacks.lock().unwrap().clone();
        for callback in callbacks {
            callback();
        }
    }

    fn record_window_event(&self, window: *mut c_void, event_counter: &AtomicU64) {
        self.last_window.store(window, Ordering::Relaxed);
        self.last_window_protocol_id
            .store(window as usize as u64, Ordering::Relaxed);
        event_counter.fetch_add(1, Ordering::Relaxed);
        self.total_event_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_input_event(
        &self,
        kind: MouseInputEventKind,
        origin: InputEventOrigin,
        window: *mut c_void,
        window_position: Option<MouseWindowLocation>,
    ) {
        let sequence = self
            .input_event_count
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let mut recent_events = self.recent_events.lock().unwrap();
        let record = MouseInputEventRecord {
            sequence,
            kind,
            origin,
            window_protocol_id: (window as usize as u64 != 0).then_some(window as usize as u64),
            window_position,
            occurred_at: Instant::now(),
        };
        let coalesced = recent_events.back_mut().is_some_and(|previous| {
            if previous.origin != origin || previous.window_protocol_id != record.window_protocol_id
            {
                return false;
            }
            match (&mut previous.kind, record.kind) {
                (MouseInputEventKind::Motion, MouseInputEventKind::Motion) => {
                    *previous = record;
                    true
                }
                (
                    MouseInputEventKind::Scroll { delta_x, delta_y },
                    MouseInputEventKind::Scroll {
                        delta_x: next_x,
                        delta_y: next_y,
                    },
                ) => {
                    *delta_x += next_x;
                    *delta_y += next_y;
                    previous.sequence = record.sequence;
                    previous.window_position = record.window_position;
                    previous.occurred_at = record.occurred_at;
                    true
                }
                _ => false,
            }
        });
        if !coalesced {
            if recent_events.len() >= 512 {
                if let Some(index) = recent_events.iter().position(|event| {
                    matches!(
                        event.kind,
                        MouseInputEventKind::Motion | MouseInputEventKind::Scroll { .. }
                    )
                }) {
                    recent_events.remove(index);
                } else {
                    recent_events.pop_front();
                }
            }
            recent_events.push_back(record);
        }
        drop(recent_events);
        self.notify_input_event();
    }

    pub(crate) fn set_window_location(
        &self,
        location: MouseWindowLocation,
        origin: InputEventOrigin,
    ) {
        *self.window.lock().unwrap() = Some(location);
        let window = location.window.map(|e| e.0.as_ptr()).unwrap_or_default();
        self.record_window_event(window, &self.motion_event_count);
        self.record_input_event(MouseInputEventKind::Motion, origin, window, Some(location));
    }
    pub(crate) fn clear_window_location(&self, window: *mut c_void, origin: InputEventOrigin) {
        *self.window.lock().unwrap() = None;
        self.record_window_event(window, &self.motion_event_count);
        self.record_input_event(MouseInputEventKind::Leave, origin, window, None);
    }
    pub(crate) fn set_key_state(
        &self,
        key: u8,
        down: bool,
        window: *mut c_void,
        origin: InputEventOrigin,
    ) {
        let window_position = *self.window.lock().unwrap();
        self.buttons[key as usize].store(down, std::sync::atomic::Ordering::Relaxed);
        self.last_window.store(window, Ordering::Relaxed);
        let window_protocol_id = window as usize as u64;
        self.last_window_protocol_id
            .store(window_protocol_id, Ordering::Relaxed);
        let sequence = self
            .button_event_count
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.total_event_count.fetch_add(1, Ordering::Relaxed);
        let mut recent_events = self.recent_button_events.lock().unwrap();
        recent_events.push_back(MouseButtonEventRecord {
            sequence,
            button: key,
            pressed: down,
            window_protocol_id: (window_protocol_id != 0).then_some(window_protocol_id),
            origin,
            window_position,
            occurred_at: Instant::now(),
        });
        while recent_events.len() > 256 {
            recent_events.pop_front();
        }
        drop(recent_events);
        self.record_input_event(
            MouseInputEventKind::Button {
                button: key,
                pressed: down,
            },
            origin,
            window,
            window_position,
        );
    }

    pub(crate) fn add_scroll_delta(
        &self,
        delta_x: f64,
        delta_y: f64,
        window: *mut c_void,
        origin: InputEventOrigin,
    ) {
        let window_position = *self.window.lock().unwrap();
        self.scroll_delta_x
            .fetch_add(delta_x, std::sync::atomic::Ordering::Relaxed);
        self.scroll_delta_y
            .fetch_add(delta_y, std::sync::atomic::Ordering::Relaxed);
        self.record_window_event(window, &self.scroll_event_count);
        self.record_input_event(
            MouseInputEventKind::Scroll { delta_x, delta_y },
            origin,
            window,
            window_position,
        );
    }
}

/// Provides access to mouse input from all mice on the system.
///
/// This type coalesces input from all connected mice into a single interface.
/// It provides access to:
/// - Mouse position within windows
/// - Button states (left, right, middle, and others)
/// - Accumulated scroll deltas
///
/// # Examples
///
/// ```
/// # async fn example() {
/// use app_window::input::mouse::{Mouse, MOUSE_BUTTON_LEFT};
///
/// let mouse = Mouse::coalesced().await;
///
/// // Check if left button is pressed
/// if mouse.button_state(MOUSE_BUTTON_LEFT) {
///     println!("Left button is pressed");
/// }
///
/// // Get mouse position
/// if let Some(pos) = mouse.window_pos() {
///     println!("Mouse at ({}, {})", pos.pos_x(), pos.pos_y());
/// }
/// # }
/// ```
///
/// # Platform-specific behavior
///
/// On some platforms you must create a window before you can get mouse input.
#[derive(Debug)]
pub struct Mouse {
    shared: Arc<Shared>,
    _sys: sys::PlatformCoalescedMouse,
}

impl Mouse {
    #[cfg(target_os = "linux")]
    pub(crate) fn from_surface_shared(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            _sys: sys::PlatformCoalescedMouse::attached(),
        }
    }

    /// Creates a new `Mouse` instance that coalesces input from all mice on the system.
    ///
    /// This is the primary way to create a `Mouse` instance. The returned object
    /// will aggregate input from all connected mice.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// use app_window::input::mouse::Mouse;
    ///
    /// let mouse = Mouse::coalesced().await;
    /// // Now you can query mouse state
    /// # }
    /// ```
    pub async fn coalesced() -> Self {
        assert!(
            is_main_thread_running(),
            "Main thread must be started before creating coalesced mouse"
        );
        let shared = Arc::new(Shared::new());
        let coalesced = sys::PlatformCoalescedMouse::new(&shared).await;
        Mouse {
            shared,
            _sys: coalesced,
        }
    }

    #[allow(rustdoc::broken_intra_doc_links)] //references to the platform-specific code
    /**
        Returns the [MouseWindowLocation]

        # Platform specifics

        You may need to create a window first, using APIs in this crate.
    */
    pub fn window_pos(&self) -> Option<MouseWindowLocation> {
        *self.shared.window.lock().unwrap()
    }

    /// Determines if the specified mouse button is currently pressed.
    ///
    /// # Arguments
    ///
    /// * `button` - The button to check. Use constants like [`MOUSE_BUTTON_LEFT`],
    ///   [`MOUSE_BUTTON_RIGHT`], or [`MOUSE_BUTTON_MIDDLE`]. Other button values
    ///   (e.g., for mice with additional buttons) may be supported on a best-effort basis.
    ///
    /// # Returns
    ///
    /// `true` if the button is currently pressed, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// use app_window::input::mouse::{Mouse, MOUSE_BUTTON_LEFT, MOUSE_BUTTON_RIGHT};
    ///
    /// let mouse = Mouse::coalesced().await;
    ///
    /// if mouse.button_state(MOUSE_BUTTON_LEFT) {
    ///     println!("Left button is pressed");
    /// }
    ///
    /// if mouse.button_state(MOUSE_BUTTON_RIGHT) {
    ///     println!("Right button is pressed");
    /// }
    /// # }
    /// ```
    pub fn button_state(&self, button: u8) -> bool {
        self.shared.buttons[button as usize].load(Ordering::Relaxed)
    }

    /// Returns the accumulated scroll delta and resets it to zero.
    ///
    /// This method is useful for implementing scroll handling in your application.
    /// The scroll delta accumulates between calls, so you should call this
    /// periodically (e.g., once per frame) to process scroll events.
    ///
    /// # Returns
    ///
    /// A tuple `(delta_x, delta_y)` containing the horizontal and vertical
    /// scroll amounts since the last call to this method.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example() {
    /// use app_window::input::mouse::Mouse;
    ///
    /// let mut mouse = Mouse::coalesced().await;
    ///
    /// // In your update loop:
    /// let (scroll_x, scroll_y) = mouse.load_clear_scroll_delta();
    /// if scroll_y != 0.0 {
    ///     println!("Scrolled vertically by {}", scroll_y);
    /// }
    /// # }
    /// ```
    pub fn load_clear_scroll_delta(&mut self) -> (f64, f64) {
        let x = self.shared.scroll_delta_x.swap(0.0, Ordering::Relaxed);
        let y = self.shared.scroll_delta_y.swap(0.0, Ordering::Relaxed);
        (x, y)
    }

    /// Returns the accumulated scroll delta without resetting it.
    ///
    /// Demand-driven render loops use this to decide whether a frame is needed
    /// before handing the delta to application logic. Call
    /// [`Mouse::load_clear_scroll_delta`] only after the application accepts the
    /// event.
    pub fn scroll_delta(&self) -> (f64, f64) {
        (
            self.shared.scroll_delta_x.load(Ordering::Relaxed),
            self.shared.scroll_delta_y.load(Ordering::Relaxed),
        )
    }

    /// Consumes only the delta observed by a previously sampled input batch.
    /// Deltas arriving after that sample remain queued for the next batch.
    pub fn consume_scroll_delta(&self, delta_x: f64, delta_y: f64) {
        if delta_x != 0.0 {
            self.shared
                .scroll_delta_x
                .fetch_sub(delta_x, Ordering::Relaxed);
        }
        if delta_y != 0.0 {
            self.shared
                .scroll_delta_y
                .fetch_sub(delta_y, Ordering::Relaxed);
        }
    }

    pub fn event_provenance(&self) -> MouseEventProvenance {
        let last_window_protocol_id = self.shared.last_window_protocol_id.load(Ordering::Relaxed);
        MouseEventProvenance {
            last_window_protocol_id: (last_window_protocol_id != 0)
                .then_some(last_window_protocol_id),
            motion_event_count: self.shared.motion_event_count.load(Ordering::Relaxed),
            button_event_count: self.shared.button_event_count.load(Ordering::Relaxed),
            scroll_event_count: self.shared.scroll_event_count.load(Ordering::Relaxed),
            total_event_count: self.shared.total_event_count.load(Ordering::Relaxed),
            recent_button_events: self
                .shared
                .recent_button_events
                .lock()
                .unwrap()
                .iter()
                .copied()
                .collect(),
            recent_events: self
                .shared
                .recent_events
                .lock()
                .unwrap()
                .iter()
                .copied()
                .collect(),
        }
    }

    /// Registers a lightweight callback that runs whenever platform mouse input
    /// updates the coalesced state. The callback must not consume mouse deltas or
    /// block; demand-driven render loops use it only to interrupt an idle wait.
    pub fn on_input_event<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.shared
            .input_wake_callbacks
            .lock()
            .unwrap()
            .push(Arc::new(callback));
    }

    /// Injects a deterministic in-process mouse sample for app-owned window tests.
    ///
    /// This is not a compositor or hardware event. It is intended for render/input
    /// harnesses that need to exercise the same coalesced input boundary as normal
    /// platform events while staying isolated from the user's desktop.
    pub fn inject_test_motion(
        &self,
        pos_x: f64,
        pos_y: f64,
        window_width: f64,
        window_height: f64,
        window_protocol_id: u64,
    ) {
        let window_ptr = window_protocol_id as usize as *mut c_void;
        let window = std::ptr::NonNull::new(window_ptr).map(Window);
        self.shared.set_window_location(
            MouseWindowLocation::new(pos_x, pos_y, window_width, window_height, window),
            InputEventOrigin::Operator,
        );
    }

    /// Injects a deterministic in-process mouse button sample for app-owned
    /// window tests. See [`Self::inject_test_motion`] for the boundary.
    pub fn inject_test_button(&self, button: u8, down: bool, window_protocol_id: u64) {
        self.shared.set_key_state(
            button,
            down,
            window_protocol_id as usize as *mut c_void,
            InputEventOrigin::Operator,
        );
    }

    /// Injects deterministic in-process wheel deltas for app-owned window tests.
    /// See [`Self::inject_test_motion`] for the boundary.
    pub fn inject_test_scroll(&self, delta_x: f64, delta_y: f64, window_protocol_id: u64) {
        self.shared.add_scroll_delta(
            delta_x,
            delta_y,
            window_protocol_id as usize as *mut c_void,
            InputEventOrigin::Operator,
        );
    }
}

impl PartialEq for Mouse {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Eq for Mouse {}

impl Hash for Mouse {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.shared).hash(state);
    }
}

#[cfg(test)]
mod test {
    use crate::input::InputEventOrigin;
    use crate::input::mouse::{Mouse, MouseInputEventKind, MouseWindowLocation, Shared};

    #[test]
    fn test_send_sync() {
        //I think basically the platform keyboard type operates as a kind of lifetime marker
        //(the main function is drop).  Accordingly it shouldn't be too bad to expect platforms to
        //implement send if necessary.
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        fn assert_unpin<T: Unpin>() {}

        assert_send::<Mouse>();
        assert_sync::<Mouse>();
        assert_unpin::<Mouse>();
    }

    #[test]
    fn button_edge_keeps_the_position_from_event_time() {
        let shared = Shared::new();
        shared.set_window_location(
            MouseWindowLocation::new(24.0, 36.0, 640.0, 480.0, None),
            InputEventOrigin::RealOs,
        );
        shared.set_key_state(0, true, std::ptr::null_mut(), InputEventOrigin::RealOs);
        shared.set_window_location(
            MouseWindowLocation::new(400.0, 300.0, 640.0, 480.0, None),
            InputEventOrigin::RealOs,
        );

        let event = shared.recent_button_events.lock().unwrap()[0];
        let position = event.window_position.expect("button position snapshot");
        assert_eq!(position.pos_x(), 24.0);
        assert_eq!(position.pos_y(), 36.0);
        let events = shared.recent_events.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].kind, MouseInputEventKind::Motion);
        assert_eq!(
            events[1].kind,
            MouseInputEventKind::Button {
                button: 0,
                pressed: true
            }
        );
        assert_eq!(events[2].kind, MouseInputEventKind::Motion);
        assert!(
            events
                .iter()
                .all(|event| event.origin == InputEventOrigin::RealOs)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_wayland_callbacks_reach_registered_mouse_and_wake_consumer() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        use wayland_client::backend::ObjectId;

        let shared = Arc::new(Shared::new());
        let wake_count = Arc::new(AtomicU64::new(0));
        let wake_counter = Arc::clone(&wake_count);
        shared
            .input_wake_callbacks
            .lock()
            .unwrap()
            .push(Arc::new(move || {
                wake_counter.fetch_add(1, Ordering::Relaxed);
            }));
        crate::input::mouse::linux::reset_and_register_test_shared(&shared);
        crate::input::mouse::linux::xdg_toplevel_configure_event(640, 480);
        let surface = ObjectId::null();
        crate::input::mouse::linux::motion_event(1, 24.0, 36.0, surface.clone());
        crate::input::mouse::linux::button_event(2, 0x110, 1, surface);

        let provenance = crate::input::mouse::MouseEventProvenance {
            last_window_protocol_id: None,
            motion_event_count: shared.motion_event_count.load(Ordering::Relaxed),
            button_event_count: shared.button_event_count.load(Ordering::Relaxed),
            scroll_event_count: shared.scroll_event_count.load(Ordering::Relaxed),
            total_event_count: shared.total_event_count.load(Ordering::Relaxed),
            recent_button_events: shared
                .recent_button_events
                .lock()
                .unwrap()
                .iter()
                .copied()
                .collect(),
            recent_events: shared
                .recent_events
                .lock()
                .unwrap()
                .iter()
                .copied()
                .collect(),
        };
        assert_eq!(provenance.motion_event_count, 1);
        assert_eq!(provenance.button_event_count, 1);
        assert_eq!(wake_count.load(Ordering::Relaxed), 2);
        assert_eq!(provenance.recent_events.len(), 2);
        assert!(
            provenance
                .recent_events
                .iter()
                .all(|event| event.origin == InputEventOrigin::RealOs)
        );
    }
}
