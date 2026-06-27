// SPDX-License-Identifier: MPL-2.0

//! Cross-platform keyboard input handling.
//!
//! This module provides a unified interface for detecting keyboard key states across
//! different platforms (Windows, macOS, Linux, and WebAssembly). The system tracks
//! the state of all keyboard keys globally, coalescing input from all connected keyboards
//! into a single logical keyboard.
//!
//! # Architecture
//!
//! The keyboard input system follows a layered architecture:
//!
//! 1. **Platform Layer** (`sys` submodules): Platform-specific implementations that capture
//!    raw keyboard events from the operating system.
//! 2. **Translation Layer**: Converts platform-specific scancodes to the unified `KeyboardKey` enum.
//! 3. **State Management**: Thread-safe tracking of key states using atomic operations.
//! 4. **Public API**: The `Keyboard` struct provides a simple interface to query key states.
//!
//! All keyboards connected to the system are automatically coalesced into a single logical
//! keyboard. This means pressing 'A' on any connected keyboard will register as the same
//! key press.
//!
//! # Thread Safety
//!
//! The keyboard system is fully thread-safe. Key states are stored using atomic operations,
//! allowing lock-free access from multiple threads simultaneously. The `Keyboard` struct
//! is `Send + Sync` and can be safely shared between threads using `Arc` or cloned.
//!
//! # Example
//!
//! ```
//! # fn example() {
//! use app_window::input::keyboard::key::KeyboardKey;
//!
//! // In a real application, you would create the keyboard after initializing the main thread:
//! // let keyboard = Keyboard::coalesced().await;
//! // For this example, we'll show the key enum usage:
//!
//! // The KeyboardKey enum represents all supported keys
//! let space_key = KeyboardKey::Space;
//! let escape_key = KeyboardKey::Escape;
//!
//! // Keys can be compared
//! assert_ne!(space_key, escape_key);
//!
//! // Keys implement Copy and Debug
//! let key_copy = space_key;
//! println!("Key: {:?}", key_copy);
//! # }
//! ```
//!
//! # Game Input Example
//!
//! ```
//! # // ALLOW_NORUN_DOCTEST: Demonstrates usage patterns but requires runtime initialization
//! # fn game_example() {
//! use app_window::input::keyboard::key::KeyboardKey;
//!
//! // In a game loop, you would check key states like this:
//! // let keyboard = Keyboard::coalesced().await;
//!
//! // Define your control keys
//! let move_keys = [
//!     (KeyboardKey::W, (0.0, -1.0)), // Up
//!     (KeyboardKey::S, (0.0, 1.0)),  // Down
//!     (KeyboardKey::A, (-1.0, 0.0)), // Left
//!     (KeyboardKey::D, (1.0, 0.0)),  // Right
//! ];
//!
//! // You can iterate over keys
//! for (key, _direction) in &move_keys {
//!     // In real code: if keyboard.is_pressed(*key) { ... }
//!     println!("Checking key: {:?}", key);
//! }
//! # }
//! ```
//!
//! # Platform Integration Requirements
//!
//! Different platforms have different requirements for keyboard event integration:
//!
//! - **Windows**: You must call `window_proc` from your window procedure to forward events
//! - **Linux**: You must call `wl_keyboard_event` from your Wayland dispatch queue
//! - **macOS**: No special integration required - events are captured automatically
//! - **WebAssembly**: No special integration required - browser events are captured automatically
//!
//! When using the `app_window` crate's window management, this integration is handled
//! automatically.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64};

/// Keyboard key definitions and enumerations.
pub mod key;

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

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm as sys;

#[cfg(target_os = "windows")]
pub(crate) use windows as sys;

#[cfg(target_os = "linux")]
pub(crate) use linux as sys;

use crate::application::is_main_thread_running;
use crate::input::keyboard::key::KeyboardKey;
use crate::input::keyboard::sys::PlatformCoalescedKeyboard;
use std::fmt;

type InputWakeCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// Internal shared state for keyboard tracking.
///
/// This struct is shared between the public `Keyboard` API and the platform-specific
/// implementations. It maintains the current state of all keyboard keys using atomic
/// operations for thread safety.
struct Shared {
    /// Array of atomic booleans tracking the pressed state of each key.
    /// Indexed by the numeric value of `KeyboardKey`.
    key_states: Vec<AtomicBool>,
    /// Platform-specific window pointer that received the most recent keyboard event.
    window_ptr: AtomicPtr<c_void>,
    last_window_protocol_id: AtomicU64,
    key_event_count: AtomicU64,
    recent_events: Mutex<VecDeque<KeyboardEventRecord>>,
    input_wake_callbacks: Mutex<Vec<InputWakeCallback>>,
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared")
            .field(
                "last_window_protocol_id",
                &self
                    .last_window_protocol_id
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .field(
                "key_event_count",
                &self
                    .key_event_count
                    .load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardEventProvenance {
    pub last_window_protocol_id: Option<u64>,
    pub key_event_count: u64,
    pub recent_events: Vec<KeyboardEventRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardEventRecord {
    pub sequence: u64,
    pub key: KeyboardKey,
    pub pressed: bool,
    pub window_protocol_id: Option<u64>,
}

impl Shared {
    /// Creates a new shared keyboard state with all keys initially unpressed.
    ///
    /// Allocates an array of atomic booleans, one for each possible key variant.
    fn new() -> Self {
        let mut vec = Vec::with_capacity(key::KeyboardKey::all_keys().len());
        for _ in 0..key::KeyboardKey::all_keys().len() {
            vec.push(AtomicBool::new(false));
        }
        Shared {
            key_states: vec,
            window_ptr: AtomicPtr::new(std::ptr::null_mut()),
            last_window_protocol_id: AtomicU64::new(0),
            key_event_count: AtomicU64::new(0),
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

    /// Updates the state of a specific key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key whose state should be updated
    /// * `state` - The new state (true = pressed, false = released)
    /// * `window_ptr` - Platform-specific window pointer that received the event
    ///
    /// # Thread Safety
    ///
    /// This method uses relaxed atomic ordering for performance. The exact ordering
    /// of concurrent key state changes is not guaranteed, but each individual key's
    /// state will be eventually consistent.
    fn set_key_state(&self, key: KeyboardKey, state: bool, window_ptr: *mut c_void) {
        logwise::debuginternal_sync!(
            "Setting key {key} to {state}",
            key = logwise::privacy::LogIt(key),
            state = state
        );
        self.window_ptr
            .store(window_ptr, std::sync::atomic::Ordering::Relaxed);
        self.last_window_protocol_id.store(
            window_ptr as usize as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        let sequence = self
            .key_event_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .saturating_add(1);
        let mut recent_events = self.recent_events.lock().unwrap();
        recent_events.push_back(KeyboardEventRecord {
            sequence,
            key,
            pressed: state,
            window_protocol_id: (window_ptr as usize as u64 != 0)
                .then_some(window_ptr as usize as u64),
        });
        while recent_events.len() > 256 {
            recent_events.pop_front();
        }
        drop(recent_events);
        self.key_states[key as usize].store(state, std::sync::atomic::Ordering::Relaxed);
        self.notify_input_event();
    }
}

/// A cross-platform keyboard input handler.
///
/// `Keyboard` provides a unified interface for detecting keyboard key states across
/// different platforms. It represents all physical keyboards connected to the system
/// as a single logical keyboard, making it easy to handle input regardless of how many
/// keyboards are connected.
///
/// # Lifecycle
///
/// The keyboard instance must be kept alive for as long as you want to track keyboard
/// input. Dropping the `Keyboard` will stop tracking keyboard events on some platforms.
///
/// # Thread Safety
///
/// `Keyboard` is `Send + Sync` and can be safely shared between threads. Key state
/// queries are lock-free and use atomic operations internally, making them very fast
/// and suitable for high-frequency polling in game loops.
///
/// # Example
///
/// ```
/// # use std::sync::Arc;
/// # use std::sync::atomic::{AtomicBool, Ordering};
/// use app_window::input::keyboard::key::KeyboardKey;
///
/// // Demonstrate thread safety with Arc
/// let shared_state = Arc::new(AtomicBool::new(false));
/// let state_clone = Arc::clone(&shared_state);
///
/// // In a real app, you'd check keyboard.is_pressed(KeyboardKey::Escape)
/// // Here we demonstrate the thread-safety pattern
/// # #[cfg(not(target_arch = "wasm32"))]
/// std::thread::spawn(move || {
///     // This would be: if keyboard.is_pressed(KeyboardKey::Escape)
///     if state_clone.load(Ordering::Relaxed) {
///         println!("Key detected from background thread!");
///     }
/// });
///
/// // Keys are represented by the KeyboardKey enum
/// let key = KeyboardKey::A;
/// assert_eq!(key, KeyboardKey::A);
/// ```
///
/// # Modifier Keys
///
/// ```
/// use app_window::input::keyboard::key::KeyboardKey;
///
/// // All modifier keys are available as enum variants
/// let modifiers = [
///     KeyboardKey::Control,
///     KeyboardKey::RightControl,
///     KeyboardKey::Shift,
///     KeyboardKey::RightShift,
///     KeyboardKey::Option,  // Alt key
///     KeyboardKey::RightOption,
///     KeyboardKey::Command, // Cmd on macOS, Windows key elsewhere
///     KeyboardKey::RightCommand,
/// ];
///
/// // Check that all modifier keys are distinct
/// for (i, key1) in modifiers.iter().enumerate() {
///     for (j, key2) in modifiers.iter().enumerate() {
///         if i != j {
///             assert_ne!(key1, key2);
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Keyboard {
    shared: Arc<Shared>,
    _platform_coalesced_keyboard: PlatformCoalescedKeyboard,
}

impl Keyboard {
    /// Creates a keyboard instance representing all physical keyboards on the system.
    ///
    /// This constructor creates a single logical keyboard that coalesces input from all
    /// connected physical keyboards. This is typically what you want for most applications,
    /// as it allows users to use any connected keyboard interchangeably.
    ///
    /// # Requirements
    ///
    /// The application's main thread must be initialized before calling this function.
    /// This is done by calling `app_window::application::main()` at program startup.
    ///
    /// # Panics
    ///
    /// Panics if the main thread has not been initialized via `app_window::application::main()`.
    ///
    /// # Example
    ///
    /// ```
    /// # // ALLOW_NORUN_DOCTEST: Requires main thread initialization
    /// # fn example() {
    /// // In your main function:
    /// // app_window::application::main(|| {
    /// //     let task = async {
    /// //         let keyboard = app_window::input::keyboard::Keyboard::coalesced().await;
    /// //         // Use the keyboard...
    /// //     };
    /// //     // Run task with your executor
    /// // });
    /// # }
    /// ```
    ///
    /// # Multiple Keyboards
    ///
    /// ```
    /// # // ALLOW_NORUN_DOCTEST: Conceptual example showing coalescing behavior
    /// # fn multi_keyboard_example() {
    /// // Even with multiple physical keyboards connected,
    /// // we get a single logical keyboard.
    /// // Pressing 'A' on ANY connected keyboard will register
    /// // as the same key press when checking:
    /// // keyboard.is_pressed(KeyboardKey::A)
    /// # }
    /// ```
    pub async fn coalesced() -> Self {
        assert!(
            is_main_thread_running(),
            "Main thread must be started before creating coalesced keyboard"
        );
        let shared = Arc::new(Shared::new());
        let _platform_coalesced_keyboard = PlatformCoalescedKeyboard::new(&shared).await;
        Self {
            shared,
            _platform_coalesced_keyboard,
        }
    }

    /// Checks if the specified key is currently pressed.
    ///
    /// Returns `true` if the key is currently held down, `false` otherwise.
    /// This method uses atomic operations and is safe to call from any thread.
    /// The operation is lock-free and very fast, suitable for high-frequency
    /// polling in game loops or input handlers.
    ///
    /// # Arguments
    ///
    /// * `key` - The keyboard key to check
    ///
    /// # Returns
    ///
    /// * `true` if the key is currently pressed down
    /// * `false` if the key is not pressed or has been released
    ///
    /// # Performance
    ///
    /// This method performs a single atomic load with relaxed memory ordering,
    /// making it extremely fast and suitable for frequent polling.
    ///
    /// # Platform Integration
    ///
    /// * **macOS** and **WASM**: No special considerations required
    /// * **Windows**: You must call `window_proc` from your window procedure
    /// * **Linux**: You must call `wl_keyboard_event` from your Wayland dispatch queue
    ///
    pub fn is_pressed(&self, key: KeyboardKey) -> bool {
        self.shared.key_states[key as usize].load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn event_provenance(&self) -> KeyboardEventProvenance {
        let last_window_protocol_id = self
            .shared
            .last_window_protocol_id
            .load(std::sync::atomic::Ordering::Relaxed);
        KeyboardEventProvenance {
            last_window_protocol_id: (last_window_protocol_id != 0)
                .then_some(last_window_protocol_id),
            key_event_count: self
                .shared
                .key_event_count
                .load(std::sync::atomic::Ordering::Relaxed),
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

    /// Registers a lightweight callback that runs whenever platform keyboard
    /// input updates the coalesced state. The callback must not block; render
    /// loops use it only to interrupt an idle wait.
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

    /// Injects a deterministic in-process keyboard sample for app-owned window
    /// tests.
    ///
    /// This is not a compositor or hardware event. It is intended for isolated
    /// render/input harnesses that need to exercise the same coalesced input
    /// boundary as normal platform events without typing into the user's
    /// desktop.
    pub fn inject_test_key(&self, key: KeyboardKey, down: bool, window_protocol_id: u64) {
        self.shared
            .set_key_state(key, down, window_protocol_id as usize as *mut c_void);
    }
}

// Trait implementations for Keyboard

impl PartialEq for Keyboard {
    /// Compares two `Keyboard` instances by their internal shared state pointer.
    ///
    /// Two `Keyboard` instances are considered equal if they share the same
    /// underlying state, which only happens if one was cloned from the other.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Eq for Keyboard {}

impl Hash for Keyboard {
    /// Hashes the `Keyboard` based on its internal shared state pointer.
    ///
    /// This allows `Keyboard` instances to be used as keys in hash maps,
    /// though this is rarely needed in practice.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.shared).hash(state);
    }
}

// Note: Default trait implementation removed because Keyboard::coalesced() is now async.
// Users must explicitly call Keyboard::coalesced().await to create an instance.

#[cfg(test)]
mod test {
    use crate::input::keyboard::Keyboard;

    #[test]
    fn test_send_sync() {
        //I think basically the platform keyboard type operates as a kind of lifetime marker
        //(the main function is drop).  Accordingly it shouldn't be too bad to expect platforms to
        //implement send if necessary.
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        fn assert_unpin<T: Unpin>() {}

        assert_send::<Keyboard>();
        assert_sync::<Keyboard>();
        assert_unpin::<Keyboard>();
    }
}
