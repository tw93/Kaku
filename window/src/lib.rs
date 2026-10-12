#![allow(clippy::cmp_owned)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::len_zero)]
#![allow(clippy::manual_contains)]
#![allow(clippy::missing_const_for_thread_local)]
#![allow(clippy::missing_transmute_annotations)]
#![allow(clippy::mut_from_ref)]
#![allow(clippy::map_clone)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::new_without_default)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::to_string_in_format_args)]
#![allow(clippy::to_string_trait_impl)]
#![allow(clippy::type_complexity)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(clippy::useless_asref)]

use async_trait::async_trait;
use bitflags::bitflags;
use config::window::WindowLevel;
use config::{ConfigHandle, Dimension, GeometryOrigin};
use promise::Future;
use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;
use thiserror::Error;
use url::Url;
pub mod bitmaps;
pub use wezterm_color_types as color;
mod configuration;
pub mod connection;
pub mod os;
pub mod screen;
mod spawn;

pub use raw_window_handle;

#[cfg(target_os = "macos")]
pub(crate) const DEFAULT_DPI: f64 = 72.0;
#[cfg(not(target_os = "macos"))]
pub(crate) const DEFAULT_DPI: f64 = 96.0;

pub fn default_dpi() -> f64 {
    match Connection::get() {
        Some(conn) => conn.default_dpi(),
        None => DEFAULT_DPI,
    }
}

pub fn drain_spawn_queue_burst(max_funcs: usize) -> bool {
    spawn::run_spawn_queue_burst(max_funcs)
}

mod egl;

pub use bitmaps::{BitmapImage, Image};
pub use connection::*;
pub use glium;
pub use os::*;
pub use wezterm_input_types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clipboard {
    Clipboard,
    PrimarySelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardData {
    Text(String),
    Files(Vec<PathBuf>),
    /// Clipboard contains an image (PNG/TIFF). The path points to a temp file
    /// written by the clipboard backend. Do not paste this as text.
    Image(std::path::PathBuf),
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::Clipboard
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub pixel_width: usize,
    pub pixel_height: usize,
    pub dpi: usize,
}

pub type ULength = euclid::Length<usize, PixelUnit>;
pub type Rect = euclid::Rect<isize, PixelUnit>;
pub type RectF = euclid::Rect<f32, PixelUnit>;
pub type Size = euclid::Size2D<isize, PixelUnit>;
pub type SizeF = euclid::Size2D<f32, PixelUnit>;
pub type ScreenRect = euclid::Rect<isize, ScreenPixelUnit>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseCursor {
    Arrow,
    Hand,
    Text,
    SizeUpDown,
    SizeLeftRight,
    /// Closed hand used while actively dragging something (e.g. reordering tabs).
    Grabbing,
}

/// Represents the preferred appearance of the windowing
/// environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Standard dark-text-on-light-background presentation
    Light,
    /// Dark mode, with predominantly dark or muted colors
    Dark,
    /// dark-text-on-light-background, but in a higher contrast
    /// more accesible palette
    LightHighContrast,
    /// darker background but with higher contrast than regular
    /// dark mode
    DarkHighContrast,
}

impl std::string::ToString for Appearance {
    fn to_string(&self) -> String {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::LightHighContrast => "LightHighContrast",
            Self::DarkHighContrast => "DarkHighContrast",
        }
        .to_string()
    }
}

bitflags! {
    #[derive(Default)]
    pub struct WindowState: u8 {
        /// Occupies the whole screen; cannot be resized while in this state.
        const FULL_SCREEN = 1<<1;
        /// Maximized along either or both of horizontal or vertical dimensions;
        /// cannot be resized while in this state.
        const MAXIMIZED = 1<<2;
        /// Minimized or in some kind of off-screen state. Cannot be repainted
        /// while in this state.
        const HIDDEN = 1<<3;
        /// Always on top (floating) window
        const ALWAYS_ON_TOP = 1<<4;
        /// Always on bottom (docked) window
        const ALWAYS_ON_BOTTOM = 1<<5;
        /// A hotkey drawer, not a transient fullscreen resize.
        const HOTKEY_WINDOW = 1<<6;
    }
}

impl WindowState {
    pub fn can_resize(self) -> bool {
        !self.intersects(Self::FULL_SCREEN | Self::MAXIMIZED | Self::HOTKEY_WINDOW)
    }

    pub fn can_paint(self) -> bool {
        !self.contains(Self::HIDDEN)
    }

    pub fn as_window_level(self) -> WindowLevel {
        if self.contains(Self::ALWAYS_ON_TOP) {
            WindowLevel::AlwaysOnTop
        } else if self.contains(Self::ALWAYS_ON_BOTTOM) {
            WindowLevel::AlwaysOnBottom
        } else {
            WindowLevel::Normal
        }
    }
}

#[derive(Debug, Clone)]
pub enum WindowKeyEvent {
    RawKeyEvent(RawKeyEvent),
    KeyEvent(KeyEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadKeyStatus {
    /// Not in a dead key processing hold
    None,
    /// Holding until composition is done; the string is the uncommitted
    /// composition text to show as a placeholder
    Composing(String),
}

#[derive(Debug)]
pub enum WindowEvent {
    /// Called when the window close button is clicked.
    /// The window closure is deferred and this event is
    /// sent to your application to decide whether it will
    /// really close the window.
    CloseRequested,

    /// Called when the window is being destroyed by the window system
    Destroyed,

    /// Called when the window has been resized
    Resized {
        dimensions: Dimensions,
        window_state: WindowState,
        live_resizing: bool,
        /// True when this resize was triggered by the window moving to a
        /// different display or other per-screen DPI source change.
        screen_changed: bool,
    },

    /// Called when a program-requested set_inner_size() has finished
    SetInnerSizeCompleted,

    /// Called when the window has been invalidated and needs to
    /// be repainted
    NeedRepaint,

    /// Called when the window gains/loses focus
    FocusChanged(bool),

    /// Called when the window becomes visible or occluded on screen
    VisibilityChanged(bool),

    AdviseDeadKeyStatus(DeadKeyStatus),

    /// Called to handle a raw key event, prior to any dead key,
    /// keymap composition or other higher level treatment.
    /// If you handle this key event, you must call
    /// event.set_handled() to prevent additional processing.
    RawKeyEvent(RawKeyEvent),

    /// Called to handle a key event.
    KeyEvent(KeyEvent),

    MouseEvent(MouseEvent),
    MouseLeave,

    AppearanceChanged(Appearance),

    Notification(Box<dyn Any + Send + Sync>),

    // Called when the files are being dragged into the window
    DraggedFile(Vec<PathBuf>),

    // Called when the files are dropped into the window
    DroppedFile(Vec<PathBuf>),

    // Called when urls are dropped into the window
    DroppedUrl(Vec<Url>),

    // Called when text is dropped into the window
    DroppedString(String),

    /// Called by menubar dispatching stuff on some systems
    PerformKeyAssignment(config::keyassignment::KeyAssignment),

    /// Called by a pane context menu. The stable pane id prevents an action
    /// from falling through to a different active pane while the menu is open.
    PerformKeyAssignmentForPane {
        action: config::keyassignment::KeyAssignment,
        pane_id: usize,
    },

    AdviseModifiersLedStatus(Modifiers, KeyboardLedStatus),
}

#[derive(Default)]
struct WindowEventSenderState {
    window: Option<Window>,
    pending: VecDeque<WindowEvent>,
    dispatching: bool,
}

#[derive(Clone)]
pub struct WindowEventSender {
    handler: Rc<RefCell<Box<dyn FnMut(WindowEvent, &Window)>>>,
    state: Rc<RefCell<WindowEventSenderState>>,
}

impl WindowEventSender {
    pub fn new<F: 'static + FnMut(WindowEvent, &Window)>(handler: F) -> Self {
        Self {
            handler: Rc::new(RefCell::new(Box::new(handler))),
            state: Rc::new(RefCell::new(WindowEventSenderState::default())),
        }
    }

    pub(crate) fn assign_window(&self, window: Window) {
        self.state.borrow_mut().window.replace(window);
    }

    pub fn dispatch(&self, event: WindowEvent) {
        {
            let mut state = self.state.borrow_mut();
            if state.window.is_none() {
                return;
            }
            state.pending.push_back(event);
            if state.dispatching {
                return;
            }
            state.dispatching = true;
        }

        loop {
            let Some((event, window)) = ({
                let mut state = self.state.borrow_mut();
                match state.pending.pop_front() {
                    Some(event) => state.window.clone().map(|window| (event, window)),
                    None => {
                        state.dispatching = false;
                        None
                    }
                }
            }) else {
                return;
            };

            let event_name = format!("{event:?}");
            log::trace!("{event_name}");
            // AppKit callbacks abort if a panic crosses extern "C"; keep the
            // panic inside the Rust event boundary and leave dispatch usable.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                (self.handler.borrow_mut())(event, &window);
            })) {
                let message = if let Some(message) = payload.downcast_ref::<&'static str>() {
                    *message
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    message.as_str()
                } else {
                    "<non-string panic>"
                };
                log::error!("window event handler panicked while handling {event_name}: {message}");
                let mut state = self.state.borrow_mut();
                state.dispatching = false;
                state.pending.clear();
                return;
            }
        }
    }
}

#[derive(Debug, Error)]
#[error("Graphics drivers lost context")]
pub struct GraphicsDriversLostContext {}

#[async_trait(?Send)]
pub trait WindowOps {
    /// Show a hidden window
    fn show(&self);

    fn notify<T: Any + Send + Sync>(&self, t: T)
    where
        Self: Sized;

    /// Setup opengl for rendering
    async fn enable_opengl(&self) -> anyhow::Result<Rc<glium::backend::Context>>;
    /// Advise the window that a frame is finished
    fn finish_frame(&self, frame: glium::Frame) -> anyhow::Result<()> {
        frame.finish()?;
        Ok(())
    }

    /// Hide a visible window (minimize to Dock)
    fn hide(&self);

    /// Remove the window from screen without minimize animation.
    /// Used by macOS close button to hide window while preserving running commands.
    fn order_out(&self) {
        // Default implementation falls back to hide for non-macOS platforms
        self.hide();
    }

    /// Schedule the window to be closed
    fn close(&self);

    /// Change the cursor
    fn set_cursor(&self, cursor: Option<MouseCursor>);

    /// Invalidate the window so that the entire client area will
    /// be repainted shortly
    fn invalidate(&self);

    /// Change the titlebar text for the window
    fn set_title(&self, title: &str);

    #[cfg(target_os = "macos")]
    fn show_context_menu(&self, menu: crate::os::macos::menu::Menu, point: ScreenPoint);

    /// Resize the inner or client area of the window
    fn set_inner_size(&self, width: usize, height: usize);

    /// Use for windows snap layouts
    fn set_maximize_button_position(&self, _rect: ScreenRect) {}

    /// Requests the windowing system to start a window drag.
    ///
    /// This is only implemented on backends that handle
    /// window movement on the server side (Wayland).
    fn request_drag_move(&self) {}

    /// Requests a native drag for a maximized/zoomed window when the platform
    /// can safely hand that gesture to the window manager.
    fn request_drag_move_from_maximized(&self) {
        self.request_drag_move();
    }

    /// Signal to the windowing system that the mouse is over
    /// a window dragging area.
    ///
    /// This is only implemented on backends that need to
    /// know if the mouse is in a drag area to handle the
    /// click before forwarding the event (Windows).
    fn set_window_drag_position(&self, _coords: ScreenPoint) {}

    /// Changes the location of the window on the screen.
    /// The coordinates are of the top left pixel of the
    /// client area.
    ///
    /// This is only implemented on backends that allow
    /// windows to move themselves (not Wayland).
    fn set_window_position(&self, _coords: ScreenPoint) {}

    /// inform the windowing system of the current textual
    /// cursor input location.  This is used primarily for
    /// the platform specific input method editor
    fn set_text_cursor_position(&self, _cursor: Rect) {}

    /// Initiate textual transfer from the clipboard
    fn get_clipboard(&self, clipboard: Clipboard) -> Future<String>;

    fn get_clipboard_data(&self, clipboard: Clipboard) -> Future<ClipboardData>;

    /// Set some text in the clipboard
    fn set_clipboard(&self, clipboard: Clipboard, text: String);

    /// Set window level. Depending on the environment and user preferences
    fn set_window_level(&self, _level: WindowLevel) {}

    /// Set the icon for the window.
    /// Depending on the system this may be shown in its titlebar
    /// and/or in the task manager/task switcher
    fn set_icon(&self, _image: Image) {}

    fn maximize(&self) {}
    fn restore(&self) {}
    fn focus(&self) {}
    /// Center the window within the visible frame of the screen it
    /// currently occupies. Implemented natively so the math stays in one
    /// coordinate space; screen rects from `Connection::screens()` and
    /// window pixel sizes use different scales on mixed-DPI setups.
    fn center(&self) {}

    fn toggle_fullscreen(&self) {}

    fn config_did_change(&self, _config: &config::ConfigHandle) {}

    /// Check if the window is in a zoom (maximize/restore) animation.
    /// Returns true if within the animation period to allow hiding content
    /// during the transition to avoid visual flickering.
    fn is_zoom_animation_active(&self) -> bool {
        false
    }

    /// Configure the Window so that the desktop environment
    /// will constrain resizes so that they are multiples of
    /// the x and y values specified.
    /// This may not be supported or respected by the desktop
    /// environment.
    fn set_resize_increments(&self, _incr: ResizeIncrement) {}

    fn get_os_parameters(
        &self,
        _config: &ConfigHandle,
        _window_state: WindowState,
    ) -> anyhow::Result<Option<os::parameters::Parameters>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestedWindowGeometry {
    pub width: Dimension,
    pub height: Dimension,
    pub x: Option<Dimension>,
    pub y: Option<Dimension>,
    /// Specifies basis for evaluating x/y coords.
    /// Also applies to width/height when computing % based dimensions
    pub origin: GeometryOrigin,
}

#[derive(Debug, Clone)]
pub struct ResolvedGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ResizeIncrement {
    pub x: u16,
    pub y: u16,
    pub base_width: u16,
    pub base_height: u16,
}

impl ResizeIncrement {
    /// Use this as a readable shorthand for disabling the feature
    pub fn disabled() -> Self {
        Self {
            x: 1,
            y: 1,
            base_width: 0,
            base_height: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn hotkey_drawer_font_scaling_keeps_fixed_window_geometry() {
        assert!(WindowState::default().can_resize());
        assert!(WindowState::ALWAYS_ON_TOP.can_resize());
        for extra in [
            WindowState::default(),
            WindowState::ALWAYS_ON_TOP,
            WindowState::HIDDEN,
        ] {
            assert!(!(WindowState::HOTKEY_WINDOW | extra).can_resize());
        }
        assert!(!WindowState::FULL_SCREEN.can_resize());
        assert!(!WindowState::MAXIMIZED.can_resize());
    }

    #[test]
    fn window_event_sender_queues_reentrant_dispatch() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sender = Rc::new(RefCell::new(None::<WindowEventSender>));

        let seen_clone = Rc::clone(&seen);
        let sender_clone = Rc::clone(&sender);
        let events = WindowEventSender::new(move |event, _window| {
            seen_clone.borrow_mut().push(format!("{event:?}"));
            if matches!(event, WindowEvent::MouseLeave) {
                sender_clone
                    .borrow()
                    .as_ref()
                    .expect("sender assigned")
                    .dispatch(WindowEvent::NeedRepaint);
            }
        });
        events.assign_window(Window::for_test(7));
        sender.borrow_mut().replace(events.clone());

        events.dispatch(WindowEvent::MouseLeave);

        assert_eq!(seen.borrow().as_slice(), ["MouseLeave", "NeedRepaint"]);
    }

    #[test]
    fn window_event_sender_recovers_after_handler_panic() {
        let calls = Rc::new(RefCell::new(0));
        let calls_clone = Rc::clone(&calls);
        let events = WindowEventSender::new(move |_event, _window| {
            let mut calls = calls_clone.borrow_mut();
            *calls += 1;
            if *calls == 1 {
                panic!("intentional test panic");
            }
        });
        events.assign_window(Window::for_test(7));

        events.dispatch(WindowEvent::NeedRepaint);
        events.dispatch(WindowEvent::NeedRepaint);

        assert_eq!(*calls.borrow(), 2);
    }
}
