// SPDX-License-Identifier: MPL-2.0

use crate::coordinates::Size;
use crate::input::{keyboard::Keyboard, mouse::Mouse};
use crate::sys;
use raw_window_handle::{DisplayHandle, RawDisplayHandle, RawWindowHandle, WindowHandle};

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SurfaceContentReport {
    pub external_surface_created: bool,
    pub shm_content_attach_count: u64,
    pub shm_content_attach_after_external_surface_count: u64,
    pub external_surface_configure_skip_count: u64,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceCursorIcon {
    Default,
    ColumnResize,
    RowResize,
    Pointer,
    Text,
}

/// A type that can be drawn on, e.g. by wgpu.
///
/// A `Surface` represents a drawable area within a window. It provides the necessary
/// handles for graphics APIs like wgpu to render content, and notifies about size changes.
///
/// # Creating a Surface
///
/// A Surface is created from a [`Window`](crate::window::Window) using the
/// [`Window::surface()`](crate::window::Window::surface) method. Only one Surface can
/// be created per window.
///
/// ```
/// # async fn example() {
/// use app_window::window::Window;
/// use app_window::coordinates::{Position, Size};
///
/// let mut window = Window::new(
///     Position::new(100.0, 100.0),
///     Size::new(800.0, 600.0),
///     "My Window".to_string()
/// ).await;
///
/// let surface = window.surface().await;
/// # }
/// ```
///
/// # Platform Implementation Details
///
/// The Surface abstraction exists for several reasons:
///
/// 1. **Future flexibility**: While currently limited to one surface per window, this
///    design allows for potential multi-surface support in the future.
/// 2. **Performance**: On some platforms, creating a drawable surface is more expensive
///    than creating a blank window. Applications that don't need to draw can skip this cost.
/// 3. **Compositing**: Platform window decorations (title bars, borders) often require
///    special handling when composited with the application's rendered content.
#[derive(Debug)]
#[must_use = "Dropping a surface may release resources"]
pub struct Surface {
    pub(super) sys: sys::Surface,
}

impl Surface {
    /// Returns input state bound to this surface.
    ///
    /// On Wayland this is registered when the window is created, before the
    /// compositor can send the first pointer or keyboard event.
    pub async fn input(&self) -> (Mouse, Keyboard) {
        #[cfg(target_os = "linux")]
        {
            self.sys.input()
        }
        #[cfg(not(target_os = "linux"))]
        {
            (Mouse::coalesced().await, Keyboard::coalesced().await)
        }
    }

    /// Returns the size and scale factor of the surface.
    ///
    /// The size is returned in logical pixels, which may differ from physical pixels
    /// on high-DPI displays. The scale factor indicates the ratio between logical
    /// and physical pixels.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The [`Size`] of the surface in logical pixels
    /// - The scale factor as a `f64` (e.g., 2.0 for a Retina display)
    ///
    /// # Example
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::window::Window;
    /// # let mut window: Window = todo!();
    /// let mut surface = window.surface().await;
    /// let (size, scale) = surface.size_scale().await;
    ///
    /// println!("Surface size: {}x{}", size.width(), size.height());
    /// println!("Scale factor: {}", scale);
    ///
    /// // Calculate physical pixel dimensions
    /// let physical_width = (size.width() * scale) as u32;
    /// let physical_height = (size.height() * scale) as u32;
    /// # }
    /// ```
    pub async fn size_scale(&self) -> (Size, f64) {
        self.sys.size_scale().await
    }

    /// Returns the size and scale factor of the surface from the main thread.
    ///
    /// This is the synchronous version of [`size_scale()`](Self::size_scale) that can
    /// only be called from the main thread. Use this when you're already on the main
    /// thread and need the size without awaiting.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The [`Size`] of the surface in logical pixels
    /// - The scale factor as a `f64` (e.g., 2.0 for a Retina display)
    ///
    /// # Panics
    ///
    /// Panics if called from a thread other than the main thread.
    ///
    /// # Example
    ///
    /// ```
    /// #[cfg(target_arch = "wasm32")] {
    ///     wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
    /// }
    /// use app_window::test_support::doctest_main;
    /// use some_executor::task::{Configuration, Task};
    ///
    /// doctest_main(|| {
    ///     Task::without_notifications(
    ///         "doctest".to_string(),
    ///         Configuration::default(),
    ///         async {
    ///             use app_window::application;
    ///
    ///             let mut window = app_window::window::Window::default().await;
    ///             let surface = window.surface().await;
    ///
    ///             // Only call from main thread
    ///             if application::is_main_thread() {
    ///                 let (size, scale) = surface.size_main();
    ///                 println!("Size: {}x{} at {}x scale", size.width(), size.height(), scale);
    ///             }
    ///         },
    ///     ).spawn_static_current();
    /// });
    /// ```
    pub fn size_main(&self) -> (Size, f64) {
        assert!(
            sys::is_main_thread(),
            "`size_main` must be called from the main thread"
        );
        self.sys.size_main()
    }

    /// Returns the raw window handle for this surface.
    ///
    /// This handle can be used with graphics APIs like wgpu to create a rendering surface.
    /// The handle is platform-specific and follows the [`raw-window-handle`](https://docs.rs/raw-window-handle)
    /// standard.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::window::Window;
    /// # let mut window: Window = todo!();
    /// let surface = window.surface().await;
    /// let raw_handle = surface.raw_window_handle();
    ///
    /// // Use with wgpu (requires 'wgpu' feature)
    /// # #[cfg(feature = "wgpu")]
    /// # {
    /// # use wgpu::{Instance, SurfaceTargetUnsafe};
    /// # let instance: Instance = todo!();
    /// unsafe {
    ///     let wgpu_surface = instance.create_surface_unsafe(
    ///         SurfaceTargetUnsafe::RawHandle {
    ///             raw_display_handle: surface.raw_display_handle(),
    ///             raw_window_handle: raw_handle,
    ///         }
    ///     );
    /// }
    /// # }
    /// # }
    /// ```
    pub fn raw_window_handle(&self) -> RawWindowHandle {
        self.sys.raw_window_handle()
    }

    /// Returns a borrowed window handle for this surface.
    ///
    /// This is a safe wrapper around [`raw_window_handle()`](Self::raw_window_handle) that
    /// provides a lifetime-bound handle following the [`raw-window-handle`](https://docs.rs/raw-window-handle)
    /// v0.6 API. This is the preferred method for passing window handles to graphics APIs
    /// that accept the newer `WindowHandle` type.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::window::Window;
    /// # let mut window: Window = todo!();
    /// let surface = window.surface().await;
    /// let handle = surface.window_handle();
    ///
    /// // Use with graphics APIs that accept WindowHandle
    /// // e.g., wgpu's create_surface_from_handle()
    /// # }
    /// ```
    pub fn window_handle(&self) -> WindowHandle<'_> {
        //should be safe because we own the raw handle
        unsafe { WindowHandle::borrow_raw(self.raw_window_handle()) }
    }

    /// Returns the raw display handle for this surface.
    ///
    /// This handle represents the display or connection that owns the window. It's required
    /// alongside the window handle when creating graphics surfaces. The handle is
    /// platform-specific and follows the [`raw-window-handle`](https://docs.rs/raw-window-handle)
    /// standard.
    ///
    /// # Platform Details
    ///
    /// - **Windows**: Returns a Windows display handle
    /// - **macOS**: Returns an AppKit display handle  
    /// - **Linux (Wayland)**: Returns a Wayland display handle
    /// - **Web**: Returns a web display handle
    ///
    /// # Example
    ///
    /// See [`raw_window_handle()`](Self::raw_window_handle) for usage with graphics APIs.
    pub fn raw_display_handle(&self) -> RawDisplayHandle {
        self.sys.raw_display_handle()
    }

    /// Returns a borrowed display handle for this surface.
    ///
    /// This is a safe wrapper around [`raw_display_handle()`](Self::raw_display_handle) that
    /// provides a lifetime-bound handle following the [`raw-window-handle`](https://docs.rs/raw-window-handle)
    /// v0.6 API. This is the preferred method for passing display handles to graphics APIs
    /// that accept the newer `DisplayHandle` type.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::window::Window;
    /// # let mut window: Window = todo!();
    /// let surface = window.surface().await;
    /// let display = surface.display_handle();
    /// let window = surface.window_handle();
    ///
    /// // Both handles are often used together when creating graphics surfaces
    /// # }
    /// ```
    pub fn display_handle(&self) -> DisplayHandle<'_> {
        //should be safe because we own the raw handle
        unsafe { DisplayHandle::borrow_raw(self.raw_display_handle()) }
    }
    /// Registers a callback to be invoked when the surface is resized.
    ///
    /// The callback will be called whenever the surface size changes, such as when the user
    /// resizes the window or the window is moved between displays with different DPI settings.
    /// The callback receives the new [`Size`] in logical pixels.
    ///
    /// # Thread Safety
    ///
    /// The callback must be `Send` and `'static` as it may be called from different threads
    /// depending on the platform. The callback should be efficient as it may be called
    /// frequently during resize operations.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn example() {
    /// # use app_window::window::Window;
    /// # use app_window::coordinates::Size;
    /// # let mut window: Window = todo!();
    /// let mut surface = window.surface().await;
    ///
    /// surface.size_update(|new_size: Size| {
    ///     println!("Surface resized to: {}x{}",
    ///              new_size.width(),
    ///              new_size.height());
    ///     
    ///     // Trigger a re-render or update your graphics pipeline
    ///     // with the new dimensions
    /// });
    /// # }
    /// ```
    ///
    /// # Platform Behavior
    ///
    /// - **All platforms**: The callback is invoked after the resize has occurred
    /// - **macOS**: May be called multiple times during a resize drag operation
    /// - **Windows/Linux**: Typically called at the end of a resize operation
    pub fn size_update<F: Fn(Size) + Send + 'static>(&mut self, update: F) {
        self.sys.size_update(update)
    }

    #[cfg(target_os = "linux")]
    pub fn content_report(&self) -> SurfaceContentReport {
        self.sys.content_report()
    }

    #[cfg(target_os = "linux")]
    pub fn set_cursor_icon(&self, icon: SurfaceCursorIcon) {
        self.sys.set_cursor_icon(icon)
    }

    #[cfg(target_os = "linux")]
    pub fn update_accessibility_if_active(&self, update: accesskit::TreeUpdate) {
        self.sys.update_accessibility_if_active(update)
    }

    #[cfg(target_os = "linux")]
    pub fn take_accessibility_action_requests(&self) -> Vec<accesskit::ActionRequest> {
        self.sys.take_accessibility_action_requests()
    }
}

#[cfg(test)]
mod tests {
    use crate::surface::Surface;

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn send_sync() {
        fn assert_send<T: Send + Sync>() {}
        assert_send::<Surface>();
    }
}
