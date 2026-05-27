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
use std::hash::{Hash, Hasher};

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm as sys;

#[cfg(target_os = "windows")]
pub(crate) use windows as sys;

#[cfg(target_os = "linux")]
pub(crate) use linux as sys;

use crate::application::is_main_thread_running;
use crate::input::Window;
use atomic_float::AtomicF64;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

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
#[derive(Debug, Clone, Copy)]
pub struct MouseWindowLocation {
    pos_x: f64,
    pos_y: f64,
    window_width: f64,
    window_height: f64,
    window: Option<Window>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEventProvenance {
    pub last_window_protocol_id: Option<u64>,
    pub motion_event_count: u64,
    pub button_event_count: u64,
    pub scroll_event_count: u64,
    pub total_event_count: u64,
    pub recent_button_events: Vec<MouseButtonEventRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseButtonEventRecord {
    pub sequence: u64,
    pub button: u8,
    pub pressed: bool,
    pub window_protocol_id: Option<u64>,
}

impl MouseWindowLocation {
    fn new(
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

#[derive(Debug)]
struct Shared {
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
}
impl Shared {
    fn new() -> Self {
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
        }
    }

    fn record_window_event(&self, window: *mut c_void, event_counter: &AtomicU64) {
        self.last_window.store(window, Ordering::Relaxed);
        self.last_window_protocol_id
            .store(window as usize as u64, Ordering::Relaxed);
        event_counter.fetch_add(1, Ordering::Relaxed);
        self.total_event_count.fetch_add(1, Ordering::Relaxed);
    }

    fn set_window_location(&self, location: MouseWindowLocation) {
        logwise::debuginternal_sync!(
            "Set mouse window location {location}",
            location = logwise::privacy::LogIt(&location)
        );
        *self.window.lock().unwrap() = Some(location);
        self.record_window_event(
            location.window.map(|e| e.0.as_ptr()).unwrap_or_default(),
            &self.motion_event_count,
        )
    }
    fn set_key_state(&self, key: u8, down: bool, window: *mut c_void) {
        logwise::debuginternal_sync!("Set mouse key {key} state {down}", key = key, down = down);
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
        });
        while recent_events.len() > 256 {
            recent_events.pop_front();
        }
    }

    fn add_scroll_delta(&self, delta_x: f64, delta_y: f64, window: *mut c_void) {
        logwise::debuginternal_sync!(
            "Add mouse scroll delta {delta_x},{delta_y}",
            delta_x = delta_x,
            delta_y = delta_y
        );
        self.scroll_delta_x
            .fetch_add(delta_x, std::sync::atomic::Ordering::Relaxed);
        self.scroll_delta_y
            .fetch_add(delta_y, std::sync::atomic::Ordering::Relaxed);
        self.record_window_event(window, &self.scroll_event_count);
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
        }
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
        self.shared.set_window_location(MouseWindowLocation::new(
            pos_x,
            pos_y,
            window_width,
            window_height,
            window,
        ));
    }

    /// Injects a deterministic in-process mouse button sample for app-owned
    /// window tests. See [`Self::inject_test_motion`] for the boundary.
    pub fn inject_test_button(&self, button: u8, down: bool, window_protocol_id: u64) {
        self.shared
            .set_key_state(button, down, window_protocol_id as usize as *mut c_void);
    }

    /// Injects deterministic in-process wheel deltas for app-owned window tests.
    /// See [`Self::inject_test_motion`] for the boundary.
    pub fn inject_test_scroll(&self, delta_x: f64, delta_y: f64, window_protocol_id: u64) {
        self.shared
            .add_scroll_delta(delta_x, delta_y, window_protocol_id as usize as *mut c_void);
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
    use crate::input::mouse::Mouse;

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
}
