// let () = msg_send! is a common pattern for objc
#![allow(clippy::let_unit_value)]

use super::keycodes::*;
use super::{nsstring, nsstring_to_str};
use crate::clipboard::Clipboard as ClipboardContext;
use crate::connection::ConnectionOps;
use crate::os::macos::menu::{MenuItem, RepresentedItem};
use crate::parameters::{Border, Parameters, TitleBar};
use crate::{
    Clipboard, ClipboardData, Connection, DeadKeyStatus, Dimensions, Handled, KeyCode, KeyEvent,
    Modifiers, MouseButtons, MouseCursor, MouseEvent, MouseEventKind, MousePress, Point,
    RawKeyEvent, Rect, RequestedWindowGeometry, ResizeIncrement, ResolvedGeometry, ScreenPoint,
    Size, ULength, WindowDecorations, WindowEvent, WindowEventSender, WindowOps, WindowState,
};
use anyhow::{anyhow, ensure};
use async_trait::async_trait;
use cocoa::appkit::{
    self, CGFloat, NSApplication, NSApplicationActivateIgnoringOtherApps,
    NSApplicationPresentationOptions, NSBackingStoreBuffered, NSEvent, NSEventModifierFlags,
    NSOpenGLContext, NSOpenGLPixelFormat, NSPasteboard, NSRunningApplication, NSScreen, NSView,
    NSViewHeightSizable, NSViewWidthSizable, NSWindow, NSWindowStyleMask,
};
use cocoa::base::*;
use cocoa::foundation::{
    NSArray, NSAutoreleasePool, NSFastEnumeration, NSInteger, NSNotFound, NSPoint, NSRect, NSSize,
    NSString, NSUInteger,
};
use config::window::WindowLevel;
use config::{is_light_color, ConfigHandle, RgbaColor, SrgbaTuple};
use core_foundation::base::{CFTypeID, TCFType};
use core_foundation::bundle::{CFBundleGetBundleWithIdentifier, CFBundleGetFunctionPointerForName};
use core_foundation::data::{CFData, CFDataGetBytePtr, CFDataRef};
use core_foundation::string::{CFString, CFStringRef, UniChar};
use core_foundation::{declare_TCFType, impl_TCFType};
use objc::declare::ClassDecl;
use objc::rc::{StrongPtr, WeakPtr};
use objc::runtime::{Class, Object, Protocol, Sel};
use objc::*;
use promise::Future;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::ffi::{c_void, CStr};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wezterm_font::FontConfiguration;
use wezterm_input_types::{is_ascii_control, IntegratedTitleButtonStyle, KeyboardLedStatus};

static APP_TERMINATING: AtomicBool = AtomicBool::new(false);
// Cached opacity state, updated on config change. Avoids a Mutex lock
// on every compositor call to isOpaque (60-120Hz on ProMotion displays).
static VIEW_IS_OPAQUE: AtomicBool = AtomicBool::new(true);

#[allow(non_upper_case_globals)]
const NSViewLayerContentsPlacementTopLeft: NSInteger = 11;
#[allow(non_upper_case_globals)]
const NSViewLayerContentsRedrawDuringViewResize: NSInteger = 2;
const FULLSCREEN_ENTER_HIDE_CONTENT_MS: u64 = 30;
const FULLSCREEN_EXIT_HIDE_CONTENT_MS: u64 = 20;
const ZOOM_HIDE_CONTENT_MS: u64 = 20;
const FULLSCREEN_DISPLAY_CHANGE_OPENGL_PRESENT_DEFER_MS: u64 = 300;
const WINDOWED_DISPLAY_CHANGE_OPENGL_PRESENT_DEFER_MS: u64 = 150;
const MOVE_PERSIST_DELAY_SECS: f64 = 0.35;
// Keep these accessibility strings stable. Some voice input tools match
// TextArea semantics and descriptions heuristically to decide whether the
// view is editable text.
const AX_ROLE_TEXT_AREA: &[u8] = b"AXTextArea\0";
const AX_ROLE_DESCRIPTION_TERMINAL_TEXT_AREA: &[u8] = b"terminal text area\0";

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSMainConnectionID() -> id;
    fn CGSSetWindowBackgroundBlurRadius(
        connection_id: id,
        window_id: NSInteger,
        radius: i64,
    ) -> i32;
}

/// Returns the macOS product version major number (e.g. 15, 26).
/// Uses sysctlbyname("kern.osproductversion") to avoid msg_send! struct ABI issues.
/// Result is cached for the process lifetime.
fn macos_version_major() -> u32 {
    static CACHED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        extern "C" {
            fn sysctlbyname(
                name: *const std::os::raw::c_char,
                oldp: *mut std::os::raw::c_void,
                oldlenp: *mut usize,
                newp: *mut std::os::raw::c_void,
                newlen: usize,
            ) -> std::os::raw::c_int;
        }
        let mut buf = [0u8; 32];
        let mut len = buf.len();
        let ret = unsafe {
            sysctlbyname(
                b"kern.osproductversion\0".as_ptr() as *const _,
                buf.as_mut_ptr() as *mut _,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret != 0 || len == 0 {
            return 0;
        }
        std::str::from_utf8(&buf[..len.saturating_sub(1)])
            .unwrap_or("0")
            .split('.')
            .next()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    })
}

fn round_away_from_zerof(value: f64) -> f64 {
    if value > 0. {
        value.max(1.).round()
    } else {
        value.min(-1.).round()
    }
}

fn round_away_from_zero(value: f64) -> i16 {
    if value > 0. {
        value.max(1.).round() as i16
    } else {
        value.min(-1.).round() as i16
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ImeDisposition {
    /// Nothing happened
    None,
    /// IME triggered an action
    Acted,
    /// We decided to continue with key dispatch
    Continue,
}

#[repr(C)]
struct NSRange(cocoa::foundation::NSRange);

#[derive(Debug)]
#[repr(C)]
struct NSRangePointer(*mut NSRange);

impl std::fmt::Debug for NSRange {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        fmt.debug_struct("NSRange")
            .field("location", &self.0.location)
            .field("length", &self.0.length)
            .finish()
    }
}

unsafe impl objc::Encode for NSRange {
    fn encode() -> objc::Encoding {
        let encoding = format!(
            "{{NSRange={}{}}}",
            NSUInteger::encode().as_str(),
            NSUInteger::encode().as_str()
        );
        unsafe { objc::Encoding::from_str(&encoding) }
    }
}

unsafe impl objc::Encode for NSRangePointer {
    fn encode() -> objc::Encoding {
        unsafe { objc::Encoding::from_str(&format!("^{}", NSRange::encode().as_str())) }
    }
}

impl NSRange {
    fn new(location: u64, length: u64) -> Self {
        Self(cocoa::foundation::NSRange { location, length })
    }
}

#[derive(Clone)]
pub enum BackendImpl {
    Cgl(Rc<cglbits::GlState>),
    Egl(Rc<crate::egl::GlState>),
}

impl BackendImpl {
    pub fn update(&self) {
        if let Self::Cgl(be) = self {
            be.update();
        }
    }
}

#[derive(Clone)]
pub struct GlContextPair {
    pub context: Rc<glium::backend::Context>,
    pub backend: BackendImpl,
}

impl GlContextPair {
    /// on macOS we first try to initialize EGL by dynamically loading it.
    /// The system doesn't provide an EGL implementation, but the ANGLE
    /// project (and MetalANGLE) both provide implementations.
    /// The ANGLE EGL implementation wants a CALayer descendant passed
    /// as the EGLNativeWindowType.
    pub fn create(view: id) -> anyhow::Result<Self> {
        let behavior = if cfg!(debug_assertions) {
            glium::debug::DebugCallbackBehavior::DebugMessageOnError
        } else {
            glium::debug::DebugCallbackBehavior::Ignore
        };

        // Let's first try to initialize EGL...
        let (context, backend) = match if config::configuration().prefer_egl {
            // ANGLE wants a layer, so tell the view to create one.
            // Importantly, we must set its scale to 1.0 prior to initializing
            // EGL to prevent undesirable scaling.
            let layer: id;
            let layer_opaque = if config::configuration().window_background_opacity >= 1.0 {
                YES
            } else {
                NO
            };
            unsafe {
                let _: () = msg_send![view, setWantsLayer: YES];
                layer = msg_send![view, layer];
                let _: () = msg_send![layer, setContentsScale: 1.0f64];
                let _: () = msg_send![layer, setOpaque: layer_opaque];
            };

            let conn = Connection::get().ok_or_else(|| anyhow!("connection not initialized"))?;

            let state = match conn.gl_connection.borrow().as_ref() {
                None => crate::egl::GlState::create(None, layer as *const c_void),
                Some(glconn) => crate::egl::GlState::create_with_existing_connection(
                    glconn,
                    layer as *const c_void,
                ),
            };

            if state.is_ok() {
                conn.gl_connection
                    .borrow_mut()
                    .replace(Rc::clone(state.as_ref().unwrap().get_connection()));

                // ANGLE will create a CAMetalLayer as a sublayer of our provided
                // layer.  Even though CALayer defaults to !opaque, CAMetalLayer
                // defaults to opaque, so we need to find that layer and fix
                // the opacity so that our alpha values are respected.
                unsafe {
                    let sublayers: id = msg_send![layer, sublayers];
                    let layer_count = sublayers.count();
                    for i in 0..layer_count {
                        let sublayer = sublayers.objectAtIndex(i);
                        let _: () = msg_send![sublayer, setOpaque: layer_opaque];
                    }
                }
            }

            state
        } else {
            Err(anyhow!("prefers not to use EGL"))
        } {
            Ok(backend) => {
                let backend = Rc::new(backend);
                let context =
                    unsafe { glium::backend::Context::new(Rc::clone(&backend), true, behavior) }?;
                (context, BackendImpl::Egl(backend))
            }
            // ... and then fallback to the deprecated platform provided CGL
            Err(err) => {
                log::debug!("EGL init failed: {:#}, falling back to CGL", err);
                // CGL doesn't need a layer-backed NSView. Keeping one around
                // retains extra AppKit backing IOSurfaces for no benefit.
                unsafe {
                    let _: () = msg_send![view, setWantsLayer: NO];
                }
                let backend = Rc::new(cglbits::GlState::create(view)?);
                let context =
                    unsafe { glium::backend::Context::new(Rc::clone(&backend), true, behavior) }?;
                (context, BackendImpl::Cgl(backend))
            }
        };

        Ok(Self { context, backend })
    }
}

mod cglbits {
    use super::*;

    pub struct GlState {
        _pixel_format: StrongPtr,
        gl_context: StrongPtr,
    }

    impl GlState {
        pub fn create(view: id) -> anyhow::Result<Self> {
            let make_pixel_format = |require_accelerated: bool| unsafe {
                let mut attrs = vec![
                    appkit::NSOpenGLPFAOpenGLProfile as u32,
                    appkit::NSOpenGLProfileVersion3_2Core as u32,
                    appkit::NSOpenGLPFAClosestPolicy as u32,
                    appkit::NSOpenGLPFAColorSize as u32,
                    32,
                    appkit::NSOpenGLPFAAlphaSize as u32,
                    8,
                    appkit::NSOpenGLPFADepthSize as u32,
                    24,
                    appkit::NSOpenGLPFAStencilSize as u32,
                    8,
                    appkit::NSOpenGLPFAAllowOfflineRenderers as u32,
                ];
                if require_accelerated {
                    attrs.push(appkit::NSOpenGLPFAAccelerated as u32);
                }
                attrs.push(appkit::NSOpenGLPFADoubleBuffer as u32);
                attrs.push(0);
                StrongPtr::new(NSOpenGLPixelFormat::alloc(nil).initWithAttributes_(&attrs))
            };

            log::trace!("Calling NSOpenGLPixelFormat::initWithAttributes");
            let mut pixel_format = make_pixel_format(true);
            if pixel_format.is_null() {
                log::warn!(
                    "No accelerated NSOpenGL pixel format available; retrying without NSOpenGLPFAAccelerated"
                );
                pixel_format = make_pixel_format(false);
            }
            log::trace!("NSOpenGLPixelFormat::initWithAttributes returned");
            ensure!(
                !pixel_format.is_null(),
                "failed to create NSOpenGLPixelFormat; this can happen in virtual machines without GPU acceleration. Try front_end='WebGpu' or enable VM GPU acceleration."
            );

            // Allow using retina resolutions; without this we're forced into low res
            // and the system will scale us up, resulting in blurry rendering
            unsafe {
                let _: () = msg_send![view, setWantsBestResolutionOpenGLSurface: YES];
            }

            let gl_context = unsafe {
                StrongPtr::new(
                    NSOpenGLContext::alloc(nil).initWithFormat_shareContext_(*pixel_format, nil),
                )
            };
            ensure!(!gl_context.is_null(), "failed to create NSOpenGLContext");

            unsafe {
                let opaque: cgl::GLint = 0;
                gl_context.setValues_forParameter_(
                    &opaque,
                    cocoa::appkit::NSOpenGLContextParameter::NSOpenGLCPSurfaceOpacity,
                );

                gl_context.setView_(view);

                // Explicitly disable vsync; we'll manage throttling frames at
                // the application level
                let swap_interval: cgl::GLint = 0;
                gl_context.setValues_forParameter_(
                    &swap_interval,
                    cocoa::appkit::NSOpenGLContextParameter::NSOpenGLCPSwapInterval,
                );
            }

            Ok(Self {
                _pixel_format: pixel_format,
                gl_context,
            })
        }

        /// Calls NSOpenGLContext update; we need to do this on resize
        pub fn update(&self) {
            unsafe {
                let _: () = msg_send![*self.gl_context, update];
            }
        }

        fn should_defer_flush_buffer(&self, view: id, window: id) -> bool {
            if crate::os::macos::app::is_system_sleeping() {
                log::trace!("skip flushBuffer: system is sleeping/waking");
                return true;
            }
            unsafe {
                let screen: id = msg_send![window, screen];
                if screen.is_null() {
                    log::trace!("skip flushBuffer: NSWindow has no NSScreen");
                    return true;
                }
                let is_visible: BOOL = msg_send![window, isVisible];
                if is_visible == NO {
                    log::trace!("skip flushBuffer: NSWindow is not visible");
                    return true;
                }
                if let Some(window_view) = WindowView::get_this(&*view) {
                    if window_view.native_fullscreen_transition_active.get()
                        || window_view.simple_fullscreen_transition_active.get()
                    {
                        log::trace!("skip flushBuffer: fullscreen transition active");
                        return true;
                    }
                    if let Some(until) = window_view.display_change_opengl_present_until.get() {
                        if Instant::now() < until {
                            log::trace!("skip flushBuffer: deferred after display change or wake");
                            return true;
                        }
                        window_view.display_change_opengl_present_until.set(None);
                    }
                }
            }
            false
        }
    }

    unsafe impl glium::backend::Backend for GlState {
        fn resize(&self, _: (u32, u32)) {
            todo!()
        }

        fn swap_buffers(&self) -> Result<(), glium::SwapBuffersError> {
            unsafe {
                let pool = NSAutoreleasePool::new(nil);
                // Rebind the drawable before presenting. During sleep/wake or
                // display reconfiguration macOS can invalidate the previous
                // surface while keeping the context current.
                let _: () = msg_send![*self.gl_context, update];
                let view = self.gl_context.view();
                if !view.is_null() {
                    let window: id = msg_send![view, window];
                    if !window.is_null() {
                        if !self.should_defer_flush_buffer(view, window) {
                            self.gl_context.flushBuffer();
                        }
                    } else {
                        log::trace!("skip flushBuffer: NSView has no NSWindow");
                    }
                } else {
                    log::trace!("skip flushBuffer: NSOpenGLContext has no view");
                }
                let _: () = msg_send![pool, release];
            }
            Ok(())
        }

        unsafe fn get_proc_address(&self, symbol: &str) -> *const c_void {
            let symbol_name: CFString = FromStr::from_str(symbol).unwrap();
            let framework_name: CFString = FromStr::from_str("com.apple.opengl").unwrap();
            let framework = CFBundleGetBundleWithIdentifier(framework_name.as_concrete_TypeRef());
            let symbol =
                CFBundleGetFunctionPointerForName(framework, symbol_name.as_concrete_TypeRef());
            symbol as *const _
        }

        fn get_framebuffer_dimensions(&self) -> (u32, u32) {
            unsafe {
                let view = self.gl_context.view();
                let frame = NSView::frame(view);
                let backing_frame = NSView::convertRectToBacking(view, frame);
                (
                    backing_frame.size.width as u32,
                    backing_frame.size.height as u32,
                )
            }
        }

        fn is_current(&self) -> bool {
            unsafe {
                let pool = NSAutoreleasePool::new(nil);
                let current = NSOpenGLContext::currentContext(nil);
                let res = if current != nil {
                    let is_equal: BOOL = msg_send![current, isEqual: *self.gl_context];
                    is_equal != NO
                } else {
                    false
                };
                let _: () = msg_send![pool, release];
                res
            }
        }

        unsafe fn make_current(&self) {
            let _: () = msg_send![*self.gl_context, update];
            self.gl_context.makeCurrentContext();
        }
    }
}

pub(crate) struct WindowInner {
    view: StrongPtr,
    window: StrongPtr,
    config: ConfigHandle,
}

fn function_key_to_keycode(function_key: char) -> KeyCode {
    // FIXME: CTRL-C is 0x3, should it be normalized to C here
    // using the unmod string?  Or should be normalize the 0x3
    // as the canonical representation of that input?
    match function_key as u16 {
        appkit::NSUpArrowFunctionKey => KeyCode::UpArrow,
        appkit::NSDownArrowFunctionKey => KeyCode::DownArrow,
        appkit::NSLeftArrowFunctionKey => KeyCode::LeftArrow,
        appkit::NSRightArrowFunctionKey => KeyCode::RightArrow,
        appkit::NSHomeFunctionKey => KeyCode::Home,
        appkit::NSEndFunctionKey => KeyCode::End,
        appkit::NSPageUpFunctionKey => KeyCode::PageUp,
        appkit::NSPageDownFunctionKey => KeyCode::PageDown,
        appkit::NSClearLineFunctionKey => KeyCode::NumLock,
        value @ appkit::NSF1FunctionKey..=appkit::NSF35FunctionKey => {
            KeyCode::Function((value - appkit::NSF1FunctionKey + 1) as u8)
        }
        appkit::NSInsertFunctionKey => KeyCode::Insert,
        appkit::NSDeleteFunctionKey => KeyCode::Char('\u{7f}'),
        appkit::NSPrintScreenFunctionKey => KeyCode::PrintScreen,
        appkit::NSScrollLockFunctionKey => KeyCode::ScrollLock,
        appkit::NSPauseFunctionKey => KeyCode::Pause,
        appkit::NSBreakFunctionKey => KeyCode::Cancel,
        appkit::NSPrintFunctionKey => KeyCode::Print,
        _ => KeyCode::Char(function_key),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Window {
    id: usize,
}

#[cfg(test)]
impl Window {
    pub(crate) fn for_test(id: usize) -> Self {
        Self { id }
    }
}

fn set_window_position(window: *mut Object, coords: ScreenPoint) {
    unsafe {
        let cartesian = screen_point_to_cartesian(coords);
        let frame = NSWindow::frame(window);
        let content_frame = NSWindow::contentRectForFrameRect_(window, frame);
        let delta_x = content_frame.origin.x - frame.origin.x;
        let delta_y = content_frame.origin.y - frame.origin.y;
        let point = NSPoint::new(
            cartesian.x as f64 - delta_x,
            cartesian.y as f64 - delta_y - content_frame.size.height,
        );
        NSWindow::setFrameOrigin_(window, point);
    }
}

const MIN_RESTORE_WIDTH: usize = 200;
const MIN_RESTORE_HEIGHT: usize = 120;

thread_local! {
    static LAST_CLOSED_WINDOW_POSITION: RefCell<Option<ScreenPoint>> = RefCell::new(None);
    // Sync drag flag: set by request_drag_move(), checked by mouse_down to execute performWindowDragWithEvent:
    static PENDING_DRAG_MOVE: Cell<bool> = Cell::new(false);
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedWindowSize {
    width: usize,
    height: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedWindowPosition {
    x: isize,
    y: isize,
    #[serde(default)]
    screen_id: Option<u32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    #[serde(default)]
    config_version: Option<u64>,
    #[serde(default)]
    window_geometry: Option<PersistedWindowSize>,
    #[serde(default)]
    window_position: Option<PersistedWindowPosition>,
}

#[derive(Debug, Default)]
struct PersistedRestore {
    position: Option<ScreenPoint>,
    skip_persisted_size: bool,
}

fn config_dir_file(name: &str) -> PathBuf {
    config::CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| config::HOME_DIR.join(".config").join("kaku"))
        .join(name)
}

fn state_file() -> PathBuf {
    config_dir_file("state.json")
}

fn legacy_window_geometry_file() -> PathBuf {
    config_dir_file(".kaku_window_geometry")
}

fn window_position(window: *mut Object) -> Option<ScreenPoint> {
    if window.is_null() {
        return None;
    }

    unsafe {
        let frame = NSWindow::frame(window);
        let content_frame = NSWindow::contentRectForFrameRect_(window, frame);
        let top_left = NSPoint::new(
            content_frame.origin.x,
            content_frame.origin.y + content_frame.size.height,
        );
        Some(cartesian_to_screen_point(top_left))
    }
}

fn remember_last_closed_window_position(window: *mut Object) {
    if window.is_null() {
        return;
    }

    let style_mask = unsafe { NSWindow::styleMask(window) };
    if style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
        return;
    }

    if let Some(pos) = window_position(window) {
        LAST_CLOSED_WINDOW_POSITION.with(|last_pos| {
            last_pos.borrow_mut().replace(pos);
        });
    }
}

fn last_closed_window_position() -> Option<ScreenPoint> {
    LAST_CLOSED_WINDOW_POSITION.with(|last_pos| *last_pos.borrow())
}

fn should_perform_native_window_drag(
    in_fullscreen: bool,
    is_zoomed: bool,
    fills_visible_frame: bool,
) -> bool {
    !in_fullscreen && !is_zoomed && !fills_visible_frame
}

fn nsrect_approx_eq(a: NSRect, b: NSRect, tolerance: f64) -> bool {
    (a.origin.x - b.origin.x).abs() <= tolerance
        && (a.origin.y - b.origin.y).abs() <= tolerance
        && (a.size.width - b.size.width).abs() <= tolerance
        && (a.size.height - b.size.height).abs() <= tolerance
}

fn window_fills_visible_frame(window: id) -> bool {
    if window.is_null() {
        return false;
    }

    unsafe {
        let screen: id = msg_send![window, screen];
        if screen.is_null() {
            return false;
        }
        let frame = NSWindow::frame(window);
        let visible_frame: NSRect = msg_send![screen, visibleFrame];
        nsrect_approx_eq(frame, visible_frame, 1.0)
    }
}

fn fit_frame_to_visible_frame(frame: NSRect, visible_frame: NSRect) -> Option<NSRect> {
    if visible_frame.size.width <= 0.0 || visible_frame.size.height <= 0.0 {
        return None;
    }

    let mut adjusted = frame;
    adjusted.size.width = adjusted.size.width.min(visible_frame.size.width);
    adjusted.size.height = adjusted.size.height.min(visible_frame.size.height);

    let visible_max_x = visible_frame.origin.x + visible_frame.size.width;
    let visible_max_y = visible_frame.origin.y + visible_frame.size.height;

    if adjusted.origin.x < visible_frame.origin.x {
        adjusted.origin.x = visible_frame.origin.x;
    }
    if adjusted.origin.y < visible_frame.origin.y {
        adjusted.origin.y = visible_frame.origin.y;
    }
    if adjusted.origin.x + adjusted.size.width > visible_max_x {
        adjusted.origin.x = visible_max_x - adjusted.size.width;
    }
    if adjusted.origin.y + adjusted.size.height > visible_max_y {
        adjusted.origin.y = visible_max_y - adjusted.size.height;
    }

    if nsrect_approx_eq(frame, adjusted, 0.5) {
        None
    } else {
        Some(adjusted)
    }
}

fn visible_frame_for_window(window: id) -> Option<NSRect> {
    if window.is_null() {
        return None;
    }

    unsafe {
        let screen: id = msg_send![window, screen];
        let screen = if screen.is_null() {
            NSScreen::mainScreen(nil)
        } else {
            screen
        };
        if screen.is_null() {
            None
        } else {
            Some(msg_send![screen, visibleFrame])
        }
    }
}

fn window_size(window: *mut Object) -> PersistedWindowSize {
    unsafe {
        let frame = NSWindow::frame(window);
        let content_frame = NSWindow::contentRectForFrameRect_(window, frame);
        PersistedWindowSize {
            width: content_frame.size.width.round().max(1.0) as usize,
            height: content_frame.size.height.round().max(1.0) as usize,
        }
    }
}

fn restorable_window_position(pos: ScreenPoint) -> Option<ScreenPoint> {
    unsafe {
        let cart = screen_point_to_cartesian(pos);
        let screens = NSScreen::screens(nil);
        let count = screens.count();

        for idx in 0..count {
            let screen = screens.objectAtIndex(idx);
            let frame: NSRect = msg_send![screen, visibleFrame];
            let max_x = frame.origin.x + frame.size.width;
            let max_y = frame.origin.y + frame.size.height;
            if cart.x >= frame.origin.x
                && cart.x <= max_x
                && cart.y >= frame.origin.y
                && cart.y <= max_y
            {
                return Some(pos);
            }
        }
    }

    None
}

fn screen_identifier_for_screen(screen: id) -> Option<u32> {
    if screen.is_null() {
        return None;
    }

    unsafe {
        let description: id = msg_send![screen, deviceDescription];
        if description.is_null() {
            return None;
        }

        let key = nsstring("NSScreenNumber");
        let number: id = msg_send![description, objectForKey:*key];
        if number.is_null() {
            return None;
        }

        let value: u32 = msg_send![number, unsignedIntValue];
        Some(value)
    }
}

fn screen_identifier_for_window(window: *mut Object) -> Option<u32> {
    if window.is_null() {
        return None;
    }

    unsafe {
        let screen: id = msg_send![window, screen];
        if screen.is_null() {
            None
        } else {
            screen_identifier_for_screen(screen)
        }
    }
}

fn has_screen_identifier(screen_id: u32) -> bool {
    unsafe {
        let screens = NSScreen::screens(nil);
        let count = screens.count();
        for idx in 0..count {
            let screen = screens.objectAtIndex(idx);
            if screen_identifier_for_screen(screen) == Some(screen_id) {
                return true;
            }
        }
    }

    false
}

fn parse_legacy_window_size(s: &str) -> Option<PersistedWindowSize> {
    let parts: Vec<_> = s.trim().split(',').map(str::trim).collect();
    match parts.as_slice() {
        [width, height] => Some(PersistedWindowSize {
            width: width.parse::<usize>().ok()?,
            height: height.parse::<usize>().ok()?,
        }),
        [_, _, width, height] => Some(PersistedWindowSize {
            width: width.parse::<usize>().ok()?,
            height: height.parse::<usize>().ok()?,
        }),
        _ => None,
    }
}

fn migrate_legacy_window_state() -> Option<PersistedState> {
    let file_name = legacy_window_geometry_file();
    let contents = std::fs::read_to_string(&file_name).ok()?;
    let size = parse_legacy_window_size(&contents)?;

    let state = PersistedState {
        config_version: None,
        window_geometry: Some(size),
        window_position: None,
    };

    let state_file_name = state_file();
    if let Some(parent) = state_file_name.parent() {
        let _ = config::create_user_owned_dirs(parent);
    }

    if let Ok(encoded) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::write(&state_file_name, format!("{}\n", encoded));
    }

    let _ = std::fs::remove_file(&file_name);

    Some(state)
}

fn load_persisted_state() -> Option<PersistedState> {
    let file_name = state_file();
    if let Ok(contents) = std::fs::read_to_string(file_name) {
        if let Ok(state) = serde_json::from_str(&contents) {
            return Some(state);
        }
    }

    migrate_legacy_window_state()
}

fn load_persisted_window_size() -> Option<PersistedWindowSize> {
    load_persisted_state()?.window_geometry
}

fn load_persisted_restore() -> PersistedRestore {
    let mut restore = PersistedRestore::default();

    let state = match load_persisted_state() {
        Some(state) => state,
        None => return restore,
    };

    if let Some(pos) = state.window_position {
        if let Some(screen_id) = pos.screen_id {
            if !has_screen_identifier(screen_id) {
                // The previously used display is disconnected.
                // Fall back to centered default startup behavior.
                restore.skip_persisted_size = true;
                return restore;
            }
        }

        restore.position = restorable_window_position(ScreenPoint::new(pos.x, pos.y));
    }

    restore
}

fn persist_window_state(window: *mut Object, persist_position: bool) -> bool {
    if window.is_null() {
        return false;
    }

    let content_view: id = unsafe { msg_send![window, contentView] };
    if !content_view.is_null() {
        if let Some(window_view) = unsafe { WindowView::get_this(&*content_view) } {
            if window_view.simple_fullscreen_active.get()
                || window_view.simple_fullscreen_transition_active.get()
                || window_view.native_fullscreen_transition_active.get()
            {
                return false;
            }
        }
    }

    let style_mask = unsafe { NSWindow::styleMask(window) };
    if style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask) {
        return false;
    }

    let file_name = state_file();
    let size = window_size(window);
    if let Some(parent) = file_name.parent() {
        if config::create_user_owned_dirs(parent).is_err() {
            return false;
        }
    }

    let mut state = load_persisted_state().unwrap_or_default();
    state.window_geometry = Some(size);
    if persist_position {
        let screen_id = screen_identifier_for_window(window);
        state.window_position = window_position(window).map(|pos| PersistedWindowPosition {
            x: pos.x,
            y: pos.y,
            screen_id,
        });
    }

    let encoded = match serde_json::to_string_pretty(&state) {
        Ok(value) => value,
        Err(_) => return false,
    };

    std::fs::write(&file_name, format!("{}\n", encoded)).is_ok()
}

fn persist_window_size_and_position(window: *mut Object) -> bool {
    persist_window_state(window, true)
}

/// Called from the app delegate when the user confirms quit.
/// Persists size and position from tracked terminal windows before the event loop stops.
pub(crate) fn on_app_terminating() {
    APP_TERMINATING.store(true, Ordering::Relaxed);
    if let Some(conn) = Connection::get() {
        let mut windows = vec![];
        for window_inner in conn.windows.borrow().values() {
            let window = *window_inner.borrow().window;
            if !window.is_null() {
                windows.push(window);
            }
        }

        let key_window = windows.iter().copied().find(|window| unsafe {
            let is_key: BOOL = msg_send![*window, isKeyWindow];
            is_key != NO
        });
        let main_window = windows.iter().copied().find(|window| unsafe {
            let is_main: BOOL = msg_send![*window, isMainWindow];
            is_main != NO
        });

        let mut candidates = vec![];
        if let Some(window) = key_window {
            candidates.push(window);
        }
        if let Some(window) = main_window {
            if !candidates.iter().any(|candidate| *candidate == window) {
                candidates.push(window);
            }
        }
        for window in windows {
            if !candidates.iter().any(|candidate| *candidate == window) {
                candidates.push(window);
            }
        }

        for window in candidates {
            if persist_window_size_and_position(window) {
                return;
            }
        }
    }
}

impl Window {
    pub async fn new_window<F>(
        _class_name: &str,
        name: &str,
        geometry: RequestedWindowGeometry,
        config: Option<&ConfigHandle>,
        _font_config: Rc<FontConfiguration>,
        event_handler: F,
    ) -> anyhow::Result<Window>
    where
        F: 'static + FnMut(WindowEvent, &Window),
    {
        let config = match config {
            Some(c) => c.clone(),
            None => config::configuration(),
        };

        let conn = Connection::get().expect("new_window called on gui thread");
        let ResolvedGeometry {
            width,
            height,
            x,
            y,
        } = conn.resolve_geometry(geometry);

        let scale_factor = (conn.default_dpi() / crate::DEFAULT_DPI) as usize;
        let mut width = width / scale_factor;
        let mut height = height / scale_factor;
        let x = x.map(|x| x / scale_factor as i32);
        let y = y.map(|y| y / scale_factor as i32);

        let explicit_initial_pos = match (x, y) {
            (Some(x), Some(y)) => Some(ScreenPoint::new(x as isize, y as isize)),
            _ => None,
        };
        let is_first_window = conn.windows.borrow().is_empty();
        let remembered_initial_pos = if explicit_initial_pos.is_none() && is_first_window {
            last_closed_window_position()
        } else {
            None
        };
        let persisted_restore = if explicit_initial_pos.is_none()
            && is_first_window
            && remembered_initial_pos.is_none()
        {
            load_persisted_restore()
        } else {
            PersistedRestore::default()
        };
        if explicit_initial_pos.is_none()
            && is_first_window
            && !persisted_restore.skip_persisted_size
        {
            if let Some(size) = load_persisted_window_size() {
                if size.width >= MIN_RESTORE_WIDTH && size.height >= MIN_RESTORE_HEIGHT {
                    width = size.width;
                    height = size.height;
                }
            }
        }

        unsafe {
            let style_mask = decoration_to_mask(
                config.window_decorations,
                config.integrated_title_button_style,
            );
            let rect = NSRect::new(
                NSPoint::new(0., 0.),
                NSSize::new(width as f64, height as f64),
            );

            let conn = Connection::get().expect("Connection::init has not been called");

            let window_id = conn.next_window_id();
            let events = WindowEventSender::new(event_handler);

            let inner = Rc::new(RefCell::new(Inner {
                events,
                view_id: None,
                window_id,
                window: None,
                screen_changed: false,
                paint_throttled: false,
                invalidated: true,
                gl_context_pair: None,
                text_cursor_position: Rect::new(Point::new(0, 0), Size::new(0, 0)),
                tracking_rect_tag: 0,
                hscroll_remainder: 0.,
                vscroll_remainder: 0.,
                last_wheel: Instant::now(),
                key_is_down: None,
                dead_pending: None,
                fullscreen: None,
                config: config.clone(),
                ime_state: ImeDisposition::None,
                ime_last_event: None,
                live_resizing: false,
                last_reported_dpi: None,
                last_reported_window_state: WindowState::default(),
                ime_text: String::new(),
            }));

            let window: id = msg_send![get_window_class(), alloc];
            let window = StrongPtr::new(NSWindow::initWithContentRect_styleMask_backing_defer_(
                window,
                rect,
                style_mask,
                NSBackingStoreBuffered,
                NO,
            ));

            apply_decorations_to_window(
                &window,
                config.window_decorations,
                config.integrated_title_button_style,
                config.native_macos_fullscreen_mode,
            );

            // Prevent Cocoa native tabs from being used
            let _: () = msg_send![*window, setTabbingMode:2 /* NSWindowTabbingModeDisallowed */];
            let _: () = msg_send![*window, setRestorable: NO];

            window.setReleasedWhenClosed_(NO);
            // Opt out of AppKit's Windows menu: on macOS 26 a closed window can
            // leave a dangling NSWindowRepresentingMenuItem, and a later
            // setTitle: PAC-faults in _rebuildWindowsMenu. Kaku has its own
            // tab/window switching, so we don't need the system menu.
            let _: () = msg_send![*window, setExcludedFromWindowsMenu: YES];
            window.setBackgroundColor_(cocoa::appkit::NSColor::clearColor(nil));

            apply_window_appearance(&window, &config);

            // Tell Cocoa that we output in sRGB, so it handles color space
            // conversion for non-sRGB displays.
            window.setColorSpace_(cocoa::appkit::NSColorSpace::sRGBColorSpace(nil));

            // We could set this, but it makes the entire window, including
            // its titlebar, opaque to this fixed degree.
            // window.setAlphaValue_(0.4);

            if let Some(pos) = explicit_initial_pos {
                // Put it where they asked it to be.
                set_window_position(*window, pos);
            } else if let Some(pos) = remembered_initial_pos {
                // Re-open after closing last window (Cmd+W) should preserve
                // recent position without adding cold-start file I/O.
                set_window_position(*window, pos);
            } else if let Some(pos) = persisted_restore.position {
                // Cold start: restore persisted position when it is still visible.
                set_window_position(*window, pos);
            } else {
                // No position memory/cascade: keep startup deterministic.
                window.center();
            }

            window.setTitle_(*nsstring(&name));
            window.setAcceptsMouseMovedEvents_(YES);

            let view = WindowView::init_with_frame(&inner, rect)?;
            view.setAutoresizingMask_(NSViewHeightSizable | NSViewWidthSizable);

            let () = msg_send![
                *view,
                setLayerContentsPlacement: NSViewLayerContentsPlacementTopLeft
            ];

            if config.macos_window_background_blur > 0 {
                CGSSetWindowBackgroundBlurRadius(
                    CGSMainConnectionID(),
                    window.windowNumber(),
                    config.macos_window_background_blur,
                );
            }
            window.setContentView_(*view);
            window.setDelegate_(*view);

            view.setWantsLayer(YES);
            let () = msg_send![
                *view,
                setLayerContentsRedrawPolicy: NSViewLayerContentsRedrawDuringViewResize
            ];

            // register for drag and drop operations.
            let () = msg_send![
                *window,
                registerForDraggedTypes:
                    NSArray::arrayWithObject(nil, appkit::NSFilenamesPboardType)
            ];

            let frame = NSView::frame(*view);
            let backing_frame = NSView::convertRectToBacking(*view, frame);
            let width = backing_frame.size.width;
            let height = backing_frame.size.height;

            let dpi = dpi_for_window_screen(*window, &config)
                .unwrap_or(crate::DEFAULT_DPI * (backing_frame.size.width / frame.size.width))
                as usize;

            let weak_window = window.weak();
            let window_handle = Window { id: window_id };
            let window_inner = Rc::new(RefCell::new(WindowInner {
                window,
                view,
                config: config.clone(),
            }));
            inner.borrow_mut().window.replace(weak_window);
            conn.windows
                .borrow_mut()
                .insert(window_id, Rc::clone(&window_inner));

            inner.borrow().events.assign_window(window_handle.clone());

            window_handle.config_did_change(&config);

            // Synthesize a resize event immediately; this allows
            // the embedding application an opportunity to discover
            // the dpi and adjust for display scaling
            let events = inner.borrow().events.clone();
            events.dispatch(WindowEvent::Resized {
                dimensions: Dimensions {
                    pixel_width: width as usize,
                    pixel_height: height as usize,
                    dpi,
                },
                window_state: WindowState::default(),
                live_resizing: false,
                screen_changed: false,
            });

            Ok(window_handle)
        }
    }

    fn with_window_inner<R>(&self, f: impl FnOnce(&WindowInner) -> R) -> Option<R> {
        let conn = Connection::get()?;
        let handle = conn.window_by_id(self.id)?;
        let inner = match handle.try_borrow() {
            Ok(inner) => inner,
            Err(_) => return None,
        };
        Some(f(&inner))
    }

    fn ns_view(&self) -> Option<*mut Object> {
        self.with_window_inner(|inner| *inner.view)
    }

    fn ns_window(&self) -> Option<*mut Object> {
        self.with_window_inner(|inner| *inner.window)
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        unsafe {
            Ok(DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(
                AppKitDisplayHandle::new(),
            )))
        }
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ns_view = self.ns_view().ok_or(HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(NonNull::new(ns_view as *mut _).expect("non-null"));
        unsafe { Ok(WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle))) }
    }
}

/// Notify every open window that the system Light/Dark appearance changed.
///
/// Windows pin their `NSWindow` appearance (see `apply_window_appearance`),
/// which means a system appearance change no longer alters their views'
/// `effectiveAppearance`, so `viewDidChangeEffectiveAppearance` never fires
/// for already-open windows. Newly created windows pick up the appearance at
/// creation time, which is why only stale windows looked wrong. This re-runs
/// the same `AppearanceChanged` path that the view callback would have used.
pub(crate) fn broadcast_system_appearance_change() {
    let Some(conn) = Connection::get() else {
        return;
    };
    let appearance = conn.get_appearance();
    log::debug!("system appearance changed to {appearance:?}, notifying open windows");
    let windows: Vec<_> = conn.windows.borrow().values().cloned().collect();
    for window in windows {
        if let Ok(inner) = window.try_borrow() {
            if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                window_view.dispatch_event(WindowEvent::AppearanceChanged(appearance));
            }
        }
    }
}

/// @see https://developer.apple.com/documentation/appkit/nswindow/level
pub type NSWindowLevel = i64;

pub fn nswindow_level_to_window_level(nswindow_level: NSWindowLevel) -> WindowLevel {
    match nswindow_level {
        -1 => WindowLevel::AlwaysOnBottom,
        0 => WindowLevel::Normal,
        3 => WindowLevel::AlwaysOnTop,
        _ => panic!("Invalid window level: {}", nswindow_level),
    }
}

pub fn window_level_to_nswindow_level(level: WindowLevel) -> NSWindowLevel {
    match level {
        WindowLevel::AlwaysOnBottom => -1,
        WindowLevel::Normal => 0,
        WindowLevel::AlwaysOnTop => 3,
    }
}

#[async_trait(?Send)]
impl WindowOps for Window {
    async fn enable_opengl(&self) -> anyhow::Result<Rc<glium::backend::Context>> {
        let window_id = self.id;
        promise::spawn::spawn(async move {
            let conn = Connection::get().ok_or_else(|| anyhow!("connection not initialized"))?;
            let handle = conn
                .window_by_id(window_id)
                .ok_or_else(|| anyhow!("invalid window"))?;
            let mut inner = handle.borrow_mut();
            inner.enable_opengl()
        })
        .await
    }

    fn notify<T: Any + Send + Sync>(&self, t: T)
    where
        Self: Sized,
    {
        Connection::with_window_inner(self.id, move |inner| {
            if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                window_view.dispatch_event(WindowEvent::Notification(Box::new(t)));
            }
            Ok(())
        });
    }

    fn close(&self) {
        Connection::with_window_inner(self.id, |inner| {
            inner.close();
            Ok(())
        });
    }

    fn focus(&self) {
        Connection::with_window_inner(self.id, |inner| {
            inner.focus();
            Ok(())
        });
    }

    fn hide(&self) {
        Connection::with_window_inner(self.id, |inner| {
            inner.hide();
            Ok(())
        });
    }

    fn order_out(&self) {
        Connection::with_window_inner(self.id, |inner| {
            // orderOut on a native-fullscreen window leaves the Space behind
            // as a stranded black overlay. Exit the Space first; the hide is
            // completed from did_exit_fullscreen once AppKit tears it down.
            if inner.is_native_fullscreen() {
                if let Some(window_view) =
                    unsafe { WindowView::get_this(&**inner.view) }
                {
                    window_view.order_out_on_fullscreen_exit.set(true);
                }
                inner.exit_native_fullscreen();
            } else {
                inner.order_out();
            }
            Ok(())
        });
    }

    fn show(&self) {
        // Try synchronous show first when called from the main thread;
        // fall back to the deferred spawn path otherwise.
        if let Some(conn) = Connection::get() {
            if let Some(handle) = conn.window_by_id(self.id) {
                handle.borrow_mut().show();
                return;
            }
        }
        Connection::with_window_inner(self.id, |inner| {
            inner.show();
            Ok(())
        });
    }

    fn set_cursor(&self, cursor: Option<MouseCursor>) {
        Connection::with_window_inner(self.id, move |inner| {
            let _ = inner.set_cursor(cursor);
            Ok(())
        });
    }

    fn invalidate(&self) {
        Connection::with_window_inner(self.id, |inner| {
            inner.invalidate();
            Ok(())
        });
    }

    fn set_title(&self, title: &str) {
        let title = title.to_owned();
        Connection::with_window_inner(self.id, move |inner| {
            inner.set_title(&title);
            Ok(())
        });
    }

    fn set_window_level(&self, level: WindowLevel) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.set_window_level(level);
            Ok(())
        });
    }

    fn set_inner_size(&self, width: usize, height: usize) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.set_inner_size(width, height);
            if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                window_view
                    .inner
                    .borrow_mut()
                    .events
                    .dispatch(WindowEvent::SetInnerSizeCompleted);
            }
            Ok(())
        });
    }

    fn request_drag_move(&self) {
        // Set flag to be checked by mouse_down and execute performWindowDragWithEvent: synchronously.
        // Avoids async dispatch to prevent modal drag loop from swallowing subsequent events.
        PENDING_DRAG_MOVE.with(|flag| flag.set(true));
    }

    fn set_window_position(&self, coords: ScreenPoint) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.set_window_position(coords);
            Ok(())
        });
    }

    fn set_text_cursor_position(&self, cursor: Rect) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.set_text_cursor_position(cursor);
            Ok(())
        });
    }

    fn get_clipboard(&self, _clipboard: Clipboard) -> Future<String> {
        Future::result(
            ClipboardContext::new()
                .read()
                .map_err(|e| anyhow!("Failed to get clipboard:{}", e)),
        )
    }

    fn get_clipboard_data(&self, _clipboard: Clipboard) -> Future<ClipboardData> {
        Future::result(
            ClipboardContext::new()
                .read_data()
                .map_err(|e| anyhow!("Failed to get clipboard data:{}", e)),
        )
    }

    fn set_clipboard(&self, _clipboard: Clipboard, text: String) {
        if let Err(e) = ClipboardContext::new().write(text) {
            log::error!("Failed to write to clipboard: {e:#}");
        }
    }

    fn toggle_fullscreen(&self) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.toggle_fullscreen();
            Ok(())
        });
    }

    fn maximize(&self) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.maximize();
            Ok(())
        });
    }

    fn restore(&self) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.restore();
            Ok(())
        });
    }

    fn set_resize_increments(&self, incr: ResizeIncrement) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.set_resize_increments(incr);
            Ok(())
        });
    }

    fn config_did_change(&self, config: &ConfigHandle) {
        let config = config.clone();
        Connection::with_window_inner(self.id, move |inner| {
            inner.config_did_change(&config);
            Ok(())
        });
    }

    fn get_os_parameters(
        &self,
        config: &ConfigHandle,
        _window_state: WindowState,
    ) -> anyhow::Result<Option<Parameters>> {
        // We implement this method primarily to provide Notch-avoidance for
        // systems with a notch.
        // We only need this for non-native full screen mode.

        let native_full_screen = self
            .ns_window()
            .map(|ns_window| {
                let style_mask = unsafe { NSWindow::styleMask(ns_window) };
                style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask)
            })
            .unwrap_or(false);

        // For simple fullscreen, window_state may lag one frame behind the style/frame update.
        // Track the state in WindowView::simple_fullscreen_active (Cell<bool>) so we can
        // read it without borrowing the RefCell and avoid borrow conflicts during resize.
        let simple_full_screen = self
            .ns_view()
            .and_then(|ns_view| unsafe { WindowView::get_this(&*ns_view) })
            .map(|view| view.simple_fullscreen_active.get())
            .unwrap_or(false);

        let should_apply_notch_padding = simple_full_screen
            && !native_full_screen
            && !config.macos_fullscreen_extend_behind_notch;

        let border_dimensions = if should_apply_notch_padding {
            let screen = if let Some(ns_window) = self.ns_window() {
                unsafe {
                    let window_screen: id = msg_send![ns_window, screen];
                    if window_screen.is_null() {
                        NSScreen::mainScreen(nil)
                    } else {
                        window_screen
                    }
                }
            } else {
                unsafe { NSScreen::mainScreen(nil) }
            };

            if screen.is_null() {
                None
            } else if let Some(insets) = get_screen_safe_area_insets(screen) {
                let screen_frame = unsafe { NSScreen::frame(screen) };
                let scale = unsafe {
                    let backing_frame = NSScreen::convertRectToBacking_(screen, screen_frame);
                    backing_frame.size.height / screen_frame.size.height
                };

                let top = (insets.top.ceil() * scale) as usize;
                let border_color = config
                    .resolved_palette
                    .background
                    .map(|c| c.to_linear())
                    .unwrap_or(crate::color::LinearRgba::with_components(0., 0., 0., 1.));
                Some(Border {
                    top: ULength::new(top),
                    left: ULength::new(0),
                    right: ULength::new(0),
                    bottom: ULength::new(0),
                    color: border_color,
                })
            } else {
                None
            }
        } else {
            None
        };

        Ok(Some(Parameters {
            title_bar: TitleBar {
                padding_left: ULength::new(0),
                padding_right: ULength::new(0),
                height: None,
                font_and_size: None,
            },
            border_dimensions,
        }))
    }

    fn is_zoom_animation_active(&self) -> bool {
        if let Some(ns_view) = self.ns_view() {
            unsafe {
                if let Some(view) = WindowView::get_this(&*ns_view) {
                    // Hide content during entire native fullscreen transition
                    // (both enter and exit). During exit the GL buffer is deferred
                    // anyway, so full paint cycles are wasted work.
                    if view.native_fullscreen_transition_active.get() {
                        return true;
                    }
                    if let Some(until) = view.transition_hide_until.get() {
                        if Instant::now() < until {
                            return true;
                        }
                        view.transition_hide_until.set(None);
                    }
                }
            }
        }
        false
    }
}

/// Convert from a macOS screen coordinate with the origin in the bottom left
/// to a pixel coordinate with its origin in the top left
fn cartesian_to_screen_point(cartesian: NSPoint) -> ScreenPoint {
    unsafe {
        let screens = NSScreen::screens(nil);
        let primary = screens.objectAtIndex(0);
        let frame = NSScreen::frame(primary);
        let backing_frame = NSScreen::convertRectToBacking_(primary, frame);
        let scale = backing_frame.size.height / frame.size.height;
        ScreenPoint::new(
            (cartesian.x * scale) as isize,
            ((frame.size.height - cartesian.y) * scale) as isize,
        )
    }
}

/// Convert from a pixel coordinate in the top left to a macOS screen
/// coordinate with its origin in the bottom left
fn screen_point_to_cartesian(point: ScreenPoint) -> NSPoint {
    unsafe {
        let screens = NSScreen::screens(nil);
        let primary = screens.objectAtIndex(0);
        let frame = NSScreen::frame(primary);
        let backing_frame = NSScreen::convertRectToBacking_(primary, frame);
        let scale = backing_frame.size.height / frame.size.height;
        NSPoint::new(
            point.x as f64 / scale,
            frame.size.height - (point.y as f64 / scale),
        )
    }
}

impl WindowInner {
    fn arm_transition_content_hide(&mut self, duration_ms: u64, _reason: &str, sync_now: bool) {
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            let now = Instant::now();
            if let Some(until) = window_view.transition_hide_until.get() {
                if until > now {
                    return;
                }
            }
            window_view
                .transition_hide_until
                .set(Some(now + Duration::from_millis(duration_ms)));
            let window_id = {
                let mut inner = window_view.inner.borrow_mut();
                // Bypass frame-throttle so the hide frame is actually painted now.
                inner.paint_throttled = false;
                inner.invalidated = true;
                let events = inner.events.clone();
                let window_id = inner.window_id;
                drop(inner);
                events.dispatch(WindowEvent::NeedRepaint);
                window_id
            };
            unsafe {
                let _: () = msg_send![*self.view, setNeedsDisplay: YES];
                if sync_now {
                    let _: () = msg_send![*self.window, displayIfNeeded];
                }
            }

            promise::spawn::spawn(async move {
                async_io::Timer::after(Duration::from_millis(duration_ms)).await;
                Connection::with_window_inner(window_id, move |inner| {
                    if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                        let mut state = window_view.inner.borrow_mut();
                        // Ensure the unhide frame is also not swallowed by throttling.
                        state.paint_throttled = false;
                        state.invalidated = true;
                        let events = state.events.clone();
                        drop(state);
                        events.dispatch(WindowEvent::NeedRepaint);
                    }
                    unsafe {
                        let _: () = msg_send![*inner.view, setNeedsDisplay: YES];
                    }
                    Ok(())
                });
            })
            .detach();
        }
    }

    fn enable_opengl(&mut self) -> anyhow::Result<Rc<glium::backend::Context>> {
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            window_view.inner.borrow_mut().enable_opengl()
        } else {
            anyhow::bail!("window invalid");
        }
    }

    pub(crate) fn is_fullscreen(&mut self) -> bool {
        if self.is_native_fullscreen() {
            true
        } else if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            // Use try_borrow to avoid a double-borrow panic when is_fullscreen is called
            // while an event handler already holds borrow_mut on window_view.inner.
            // simple_fullscreen_active is a Cell<bool> maintained in sync with
            // inner.fullscreen and is safe to read without a borrow.
            if let Ok(inner) = window_view.inner.try_borrow() {
                inner.fullscreen.is_some()
            } else {
                window_view.simple_fullscreen_active.get()
            }
        } else {
            false
        }
    }

    fn apply_decorations(&mut self) {
        if !self.is_fullscreen() {
            apply_decorations_to_window(
                &self.window,
                self.config.window_decorations,
                self.config.integrated_title_button_style,
                self.config.native_macos_fullscreen_mode,
            );
        }
    }

    fn is_native_fullscreen(&self) -> bool {
        let style_mask = unsafe { NSWindow::styleMask(*self.window) };
        style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask)
    }

    fn toggle_native_fullscreen(&mut self) {
        unsafe {
            NSWindow::toggleFullScreen_(*self.window, nil);
        }
    }

    /// If we were in native full screen mode, exit it and return true.
    /// Otherwise, return false.
    pub(crate) fn exit_native_fullscreen(&mut self) -> bool {
        if self.is_native_fullscreen() {
            if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
                window_view.native_fullscreen_transition_active.set(true);
                window_view.native_fullscreen_target.set(Some(false));
                window_view
                    .native_fullscreen_transition_start
                    .set(Some(Instant::now()));
            }
            self.toggle_native_fullscreen();
            true
        } else {
            false
        }
    }

    /// If we were in simple full screen mode, exit it and return true.
    /// Otherwise, return false
    pub(crate) fn exit_simple_fullscreen(&mut self) -> bool {
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            let is_fullscreen = window_view.inner.borrow().fullscreen.is_some();
            if is_fullscreen {
                self.toggle_simple_fullscreen();
            }
            is_fullscreen
        } else {
            false
        }
    }

    fn toggle_simple_fullscreen(&mut self) {
        let current_app = unsafe { NSApplication::sharedApplication(nil) };

        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            let fullscreen = window_view.inner.borrow().fullscreen;
            match fullscreen {
                Some(saved_rect) => unsafe {
                    self.arm_transition_content_hide(
                        FULLSCREEN_EXIT_HIDE_CONTENT_MS,
                        "simple_fullscreen_exit",
                        false,
                    );
                    window_view.simple_fullscreen_transition_active.set(true);
                    window_view.inner.borrow_mut().live_resizing = true;
                    // Restore prior dimensions
                    apply_decorations_to_window(
                        &self.window,
                        self.config.window_decorations,
                        self.config.integrated_title_button_style,
                        self.config.native_macos_fullscreen_mode,
                    );
                    self.window.setFrame_display_(saved_rect, YES);
                    let clear: id = msg_send![class!(NSColor), clearColor];
                    let opaque = if self.config.window_background_opacity >= 1.0 {
                        YES
                    } else {
                        NO
                    };
                    let _: () = msg_send![*self.window, setOpaque: opaque];
                    let _: () = msg_send![*self.window, setBackgroundColor: clear];
                    current_app.setPresentationOptions_(
                        NSApplicationPresentationOptions::NSApplicationPresentationDefault,
                    );
                    window_view.inner.borrow_mut().fullscreen.take();
                    window_view.simple_fullscreen_active.set(false);
                    window_view.inner.borrow_mut().invalidated = true;
                },
                None => unsafe {
                    self.arm_transition_content_hide(
                        FULLSCREEN_ENTER_HIDE_CONTENT_MS,
                        "simple_fullscreen_enter",
                        false,
                    );
                    window_view.simple_fullscreen_transition_active.set(true);
                    window_view.inner.borrow_mut().live_resizing = true;
                    // Go full screen
                    let saved_rect = NSWindow::frame(*self.window);
                    window_view
                        .inner
                        .borrow_mut()
                        .fullscreen
                        .replace(saved_rect);
                    window_view.simple_fullscreen_active.set(true);

                    let main_screen = NSScreen::mainScreen(nil);
                    let screen_rect = simple_fullscreen_target_rect(
                        main_screen,
                        self.config.macos_fullscreen_extend_behind_notch,
                    );

                    // Respect window_background_opacity in fullscreen mode
                    let opaque = if self.config.window_background_opacity >= 1.0 {
                        YES
                    } else {
                        NO
                    };
                    self.window.setOpaque_(opaque);
                    let bg_color: id = if self.config.window_background_opacity >= 1.0 {
                        msg_send![class!(NSColor), blackColor]
                    } else {
                        msg_send![class!(NSColor), clearColor]
                    };
                    let _: () = msg_send![*self.window, setBackgroundColor: bg_color];
                    self.window
                        .setStyleMask_(NSWindowStyleMask::NSBorderlessWindowMask);
                    self.window.setHasShadow_(NO);
                    self.window.setFrame_display_(screen_rect, YES);
                    current_app.setPresentationOptions_(
                        NSApplicationPresentationOptions::NSApplicationPresentationAutoHideMenuBar
                            | NSApplicationPresentationOptions::NSApplicationPresentationAutoHideDock
                    );
                    window_view.inner.borrow_mut().invalidated = true;
                },
            }
        }
    }

    fn update_window_shadow(&mut self) {
        let opaque = self.config.window_background_opacity >= 1.0;
        VIEW_IS_OPAQUE.store(opaque, Ordering::Relaxed);
        let is_opaque = if opaque { YES } else { NO };
        unsafe {
            self.window.setOpaque_(is_opaque);
            // when transparent, also turn off the window shadow,
            // because having the shadow enabled seems to correlate
            // with ghostly remnants see:
            // https://github.com/wezterm/wezterm/issues/310.
            // But allow overriding the shadows independent of opacity as well:
            // <https://github.com/wezterm/wezterm/issues/2669>
            let shadow = if self
                .config
                .window_decorations
                .contains(WindowDecorations::MACOS_FORCE_ENABLE_SHADOW)
            {
                YES
            } else if self
                .config
                .window_decorations
                .contains(WindowDecorations::MACOS_FORCE_DISABLE_SHADOW)
            {
                NO
            } else {
                is_opaque
            };
            self.window.setHasShadow_(shadow);

            // On macOS 26+ (Liquid Glass) the window server clips corners at the
            // compositor level.  The init code sets backgroundColor to clearColor,
            // which leaves transparent arcs at the rounded corners.  Fill those
            // corners by setting the window background to the theme color for
            // opaque windows.  Transparent windows keep clearColor so
            // see-through rendering is preserved.
            let layer_bg_cg = if is_opaque == YES {
                let bg = self
                    .config
                    .resolved_palette
                    .background
                    .unwrap_or(RgbaColor::from(SrgbaTuple(0., 0., 0., 1.0)));
                let ns_bg: id = msg_send![class!(NSColor),
                    colorWithSRGBRed: bg.0 as CGFloat
                    green: bg.1 as CGFloat
                    blue: bg.2 as CGFloat
                    alpha: 1.0 as CGFloat
                ];
                let _: () = msg_send![*self.window, setBackgroundColor: ns_bg];
                objc2_core_graphics::CGColor::new_srgb(bg.0.into(), bg.1.into(), bg.2.into(), 1.0)
            } else {
                let clear: id = msg_send![class!(NSColor), clearColor];
                let _: () = msg_send![*self.window, setBackgroundColor: clear];
                objc2_core_graphics::CGColor::new_srgb(0., 0., 0., 0.)
            };

            // Match our Metal layer's corner radius to the window frame's corner
            // radius so compositor-clipped corners don't leave transparent arcs.
            // On macOS 26+ NSThemeFrame.layer.cornerRadius returns 0 because
            // rounding is handled by the compositor, so fall back to 10pt.
            // MACOS_FORCE_SQUARE_CORNERS opts out entirely: required on macOS 26
            // where the compositor leaves the NSWindow occupancy rectangular, so
            // tiled neighbor apps would otherwise poke into the clipped arcs.
            let force_square = self
                .config
                .window_decorations
                .contains(WindowDecorations::MACOS_FORCE_SQUARE_CORNERS);
            if !APP_TERMINATING.load(Ordering::Relaxed) {
                let layer: id = msg_send![*self.view, layer];
                if !layer.is_null() {
                    let content_view: id = msg_send![*self.window, contentView];
                    let frame_view: id = msg_send![content_view, superview];
                    let frame_layer: id = msg_send![frame_view, layer];
                    let mut corner_radius: CGFloat = if force_square {
                        0.0
                    } else if !frame_layer.is_null() {
                        msg_send![frame_layer, cornerRadius]
                    } else {
                        0.0
                    };
                    if !force_square && corner_radius == 0.0 && macos_version_major() >= 26 {
                        corner_radius = 10.0;
                    }
                    log::trace!(
                        "update_window_shadow: applying corner_radius={corner_radius} to view layer and sublayers"
                    );
                    let () = msg_send![layer, setCornerRadius: corner_radius];
                    let () = msg_send![layer, setMasksToBounds: (corner_radius > 0.0) as BOOL];
                    let () = msg_send![layer, setOpaque: is_opaque];
                    let () = msg_send![layer, setBackgroundColor: layer_bg_cg.clone()];
                    let sublayers: id = msg_send![layer, sublayers];
                    if !sublayers.is_null() {
                        let count = sublayers.count();
                        for i in 0..count {
                            let sublayer = sublayers.objectAtIndex(i);
                            let () = msg_send![sublayer, setCornerRadius: corner_radius];
                            let () = msg_send![sublayer, setMasksToBounds: (corner_radius > 0.0) as BOOL];
                            let () = msg_send![sublayer, setOpaque: is_opaque];
                            let () = msg_send![sublayer, setBackgroundColor: layer_bg_cg.clone()];
                        }
                    }
                }
            }
        }
    }

    fn update_titlebar_background(&self) {
        // Skip native NSTitlebarContainerView coloring when the user has not
        // opted in. In INTEGRATED_BUTTONS top-tab layouts our Metal layer already
        // paints the tab bar across the titlebar area, and a native CALayer
        // fill on the titlebar container will composite on top of the GPU
        // surface and visually erase the tab text/icons. Only color the
        // titlebar when the user explicitly requested it via the
        // MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR decoration, or when the
        // window is transparent (where the strip is needed to hide the gap
        // between the Metal layer and the titlebar inset).
        let should_color_titlebar = self
            .config
            .window_decorations
            .contains(WindowDecorations::MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR)
            || self.config.window_background_opacity < 1.0;
        if !should_color_titlebar {
            return;
        }

        // When the window is transparent and uses integrated buttons, our Metal
        // rendering already paints a semi-transparent fill strip in the titlebar
        // area.  Setting the NSTitlebarContainerView layer to a non-clear color
        // would double-composite on top of that strip, making the titlebar region
        // appear more opaque than the rest of the window.  Use a fully clear
        // background so that only the Metal layer contributes.
        let color = if self.config.window_background_opacity < 1.0 {
            RgbaColor::from(SrgbaTuple(0., 0., 0., 0.))
        } else {
            // Set the titlebar background to the theme color falling back to
            // black if there is no specified color scheme.
            self.config
                .resolved_palette
                .background
                .unwrap_or(RgbaColor::from(SrgbaTuple(0., 0., 0., 1.0)))
        };

        unsafe {
            if let Some(titlebar_view_container) = get_titlebar_view_container(&self.window) {
                let layer: id = msg_send![*titlebar_view_container.load(), layer];

                if layer.is_null() {
                    return;
                }

                // We need to make sure to convert the config color into an sRGB CGColor or the color will be slightly off
                let srgb_cgcolor = objc2_core_graphics::CGColor::new_srgb(
                    color.0.into(),
                    color.1.into(),
                    color.2.into(),
                    color.3.into(),
                );

                let _: () = msg_send![layer, setBackgroundColor: srgb_cgcolor];
            } else {
                log::trace!("failed to get titlebar view container from window");
            }
        }
    }

    fn update_window_background_blur(&mut self) {
        unsafe {
            CGSSetWindowBackgroundBlurRadius(
                CGSMainConnectionID(),
                self.window.windowNumber(),
                self.config.macos_window_background_blur,
            );
        }
    }
}

impl WindowInner {
    /// Refresh the OpenGL context after a display reconfiguration.
    /// This prevents crashes when AppKit tries to flush a stale OpenGL surface
    /// and forces the window to recalculate screen-dependent state.
    pub(crate) fn refresh_after_display_change(&mut self) -> bool {
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            let transition_active = window_view.native_fullscreen_transition_active.get()
                || window_view.simple_fullscreen_transition_active.get();

            let native_fullscreen = self.is_native_fullscreen();
            // Skip triggering a resize during a fullscreen transition. The transition
            // callbacks (did_enter/exit_fullscreen) will dispatch the final resize when
            // stable. Firing an extra screen-change resize during the animation causes
            // flickering with status bar apps (e.g. sketchybar) that emit
            // NSApplicationDidChangeScreenParametersNotification as they adjust.
            if let Ok(mut inner) = window_view.inner.try_borrow_mut() {
                let window_id = inner.window_id;
                let fullscreen_like = transition_active
                    || native_fullscreen
                    || window_view.simple_fullscreen_active.get();
                if inner
                    .gl_context_pair
                    .as_ref()
                    .is_some_and(|pair| matches!(&pair.backend, BackendImpl::Cgl(_)))
                {
                    let defer_ms = if fullscreen_like {
                        FULLSCREEN_DISPLAY_CHANGE_OPENGL_PRESENT_DEFER_MS
                    } else {
                        WINDOWED_DISPLAY_CHANGE_OPENGL_PRESENT_DEFER_MS
                    };
                    arm_display_change_opengl_present_defer(window_view, window_id, defer_ms);
                }
                if transition_active {
                    return true;
                }
                if let Some(gl_context_pair) = inner.gl_context_pair.as_ref() {
                    log::debug!(
                        "refreshing OpenGL context for window after display change (window_id={})",
                        inner.window_id
                    );
                    gl_context_pair.backend.update();
                }
                inner.screen_changed = true;
                // Trigger a repaint to ensure the window content is refreshed.
                // Defer dispatch to the next run-loop turn so we don't re-enter
                // AppKit from inside a CFNotificationCenter observer callout
                // (triggered by _NSCGSDisplayConfigurationDidReconfigureNotificationHandler
                // on lock-screen return). Doing heavy render work synchronously here
                // can cause a main-thread busy-loop hang.
                inner.invalidated = true;
                let is_fullscreen = fullscreen_like;
                Connection::with_window_inner(window_id, move |inner| {
                    if !is_fullscreen {
                        // Reapply window decorations so the title bar is
                        // restored after a monitor disconnection.
                        inner.apply_decorations();

                        // Display topology changes can leave a window with a
                        // frame from the old screen. Clamp both origin and size
                        // to the new visible frame so the title bar and content
                        // stay reachable.
                        unsafe {
                            let frame = NSWindow::frame(*inner.window);
                            if let Some(visible_frame) = visible_frame_for_window(*inner.window) {
                                if let Some(adjusted) =
                                    fit_frame_to_visible_frame(frame, visible_frame)
                                {
                                    inner.window.setFrame_display_(adjusted, YES);
                                }
                            }
                        }
                    }
                    if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                        window_view
                            .inner
                            .borrow_mut()
                            .events
                            .dispatch(WindowEvent::NeedRepaint);
                    }
                    Ok(())
                });
                return true;
            }
        }
        false
    }

    fn show(&mut self) {
        unsafe {
            let current_app = NSRunningApplication::currentApplication(nil);
            current_app.activateWithOptions_(NSApplicationActivateIgnoringOtherApps);

            // Stupid hack: adjust the window style mask and set it back
            // to what it was.
            // Without this, the CAMetalLayer used by webgpu seems to get
            // stuck with a scale factor of 2 despite us having configured 1.
            self.window
                .setStyleMask_(NSWindowStyleMask::NSBorderlessWindowMask);

            apply_decorations_to_window(
                &self.window,
                self.config.window_decorations,
                self.config.integrated_title_button_style,
                self.config.native_macos_fullscreen_mode,
            );

            self.update_titlebar_background();
            apply_window_appearance(&self.window, &self.config);

            self.window.makeKeyAndOrderFront_(nil);
        }
        self.dispatch_resize_event();
    }

    fn close(&mut self) {
        unsafe {
            self.window.close();
        }
    }

    pub(crate) fn focus(&mut self) {
        unsafe {
            self.window.makeKeyAndOrderFront_(nil);
        }
    }

    pub(crate) fn is_key_window(&self) -> bool {
        unsafe {
            let is_key: BOOL = msg_send![*self.window, isKeyWindow];
            is_key != NO
        }
    }

    pub(crate) fn is_main_window(&self) -> bool {
        unsafe {
            let is_main: BOOL = msg_send![*self.window, isMainWindow];
            is_main != NO
        }
    }

    /// Prepare a regular window to be shown by the global hotkey from any
    /// active macOS Space (including another app's fullscreen Space).
    pub(crate) fn prepare_for_global_hotkey_show(&mut self) {
        if self.is_fullscreen() {
            return;
        }
        unsafe {
            let mut behavior = self.window.collectionBehavior();
            behavior.insert(
                appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
                    | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
            );
            self.window.setCollectionBehavior_(behavior);
        }
    }

    /// Restore the default collection behavior after a global hotkey reveal
    /// so native Cmd+` window cycling stays scoped to the current Space.
    pub(crate) fn restore_after_global_hotkey_show(&mut self) {
        if self.is_fullscreen() {
            return;
        }
        unsafe {
            let mut behavior = self.window.collectionBehavior();
            behavior.remove(
                appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
                    | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
            );
            self.window.setCollectionBehavior_(behavior);
        }
    }

    fn hide(&mut self) {
        unsafe {
            NSWindow::miniaturize_(*self.window, *self.window);
            // We could literally set it invisible like this, but
            // then there is no UI to make it visible again later.
            //let () = msg_send![*self.window, setIsVisible: NO];
        }
    }

    /// Remove the window from screen without changing fullscreen state.
    /// Used by the global hotkey to hide a fullscreen window; the window
    /// retains its fullscreen style mask and will restore when focused.
    pub(crate) fn order_out(&mut self) {
        unsafe {
            let () = msg_send![*self.window, orderOut: nil];
        }
    }

    fn set_cursor(&mut self, cursor: Option<MouseCursor>) {
        unsafe {
            let ns_cursor_cls = class!(NSCursor);
            if let Some(cursor) = cursor {
                // Unconditionally apply the requested cursor, as there are
                // cases where macOS can decide to change the cursor to something
                // that we don't know about.
                let instance: id = match cursor {
                    MouseCursor::Arrow => msg_send![ns_cursor_cls, arrowCursor],
                    MouseCursor::Text => msg_send![ns_cursor_cls, IBeamCursor],
                    MouseCursor::Hand => msg_send![ns_cursor_cls, pointingHandCursor],
                    MouseCursor::SizeUpDown => msg_send![ns_cursor_cls, resizeUpDownCursor],
                    MouseCursor::SizeLeftRight => msg_send![ns_cursor_cls, resizeLeftRightCursor],
                    MouseCursor::Grabbing => msg_send![ns_cursor_cls, closedHandCursor],
                };
                let () = msg_send![ns_cursor_cls, setHiddenUntilMouseMoves: NO];
                let () = msg_send![instance, set];
            } else {
                let () = msg_send![ns_cursor_cls, setHiddenUntilMouseMoves: YES];
            }
        }
    }

    fn invalidate(&mut self) {
        unsafe {
            let () = msg_send![*self.view, setNeedsDisplay: YES];
            if let Some(window_view) = WindowView::get_this(&**self.view) {
                window_view.inner.borrow_mut().invalidated = true;
            }
        }
    }

    fn dispatch_resize_event(&mut self) {
        unsafe {
            WindowView::did_resize(&mut **self.view, sel!(windowDidResize:), nil);
        }
    }

    fn set_title(&mut self, title: &str) {
        let title = nsstring(title);
        unsafe {
            NSWindow::setTitle_(*self.window, *title);
        }
    }

    fn set_window_level(&mut self, level: WindowLevel) {
        unsafe {
            NSWindow::setLevel_(*self.window, window_level_to_nswindow_level(level));
        }
        // Dispatch a resize event with the updated window state
        self.dispatch_resize_event();
    }

    fn set_inner_size(&mut self, width: usize, height: usize) {
        unsafe {
            let frame = NSView::frame(*self.view as *mut _);
            let backing_frame = NSView::convertRectToBacking(*self.view as *mut _, frame);
            let scale = backing_frame.size.width / frame.size.width;

            NSWindow::setContentSize_(
                *self.window,
                NSSize::new(width as f64 / scale, height as f64 / scale),
            );

            // setContentSize_ doesn't explicitly invalidate,
            // so we need to do it ourselves
            self.invalidate();
        }
    }

    fn set_window_position(&self, coords: ScreenPoint) {
        set_window_position(*self.window, coords);
    }

    // request_drag_move moved to mouse_down for synchronous execution to avoid
    // modal drag loop swallowing subsequent events

    fn set_text_cursor_position(&mut self, cursor: Rect) {
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            window_view.inner.borrow_mut().text_cursor_position = cursor;
        }
        if self.config.use_ime {
            unsafe {
                let input_context: id = msg_send![&**self.view, inputContext];
                let () = msg_send![input_context, invalidateCharacterCoordinates];
            }
        }
    }

    fn is_zoomed(&self) -> bool {
        unsafe { msg_send![*self.window, isZoomed] }
    }

    fn maximize(&mut self) {
        // Always call zoom when explicitly requested by the user.
        // NSWindow::isZoomed can falsely return YES after screen changes,
        // which would cause the zoom call to be skipped even though the
        // window is not actually maximized.
        // <https://github.com/tw93/Kaku/issues/131>
        self.arm_transition_content_hide(ZOOM_HIDE_CONTENT_MS, "zoom_maximize", false);
        unsafe {
            NSWindow::zoom_(*self.window, nil);
        }
    }

    fn restore(&mut self) {
        if self.is_zoomed() {
            self.arm_transition_content_hide(ZOOM_HIDE_CONTENT_MS, "zoom_restore", false);
            unsafe {
                NSWindow::zoom_(*self.window, nil);
            }
        }
    }

    fn toggle_fullscreen(&mut self) {
        if self.config.native_macos_fullscreen_mode {
            if self.exit_simple_fullscreen() {
                return;
            }
            if self.exit_native_fullscreen() {
                return;
            }
            if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
                window_view.native_fullscreen_transition_active.set(true);
                window_view.native_fullscreen_target.set(Some(true));
                window_view
                    .native_fullscreen_transition_start
                    .set(Some(Instant::now()));
            }
            self.toggle_native_fullscreen();
            return;
        }

        if self.exit_simple_fullscreen() {
            return;
        }

        if self.exit_native_fullscreen() {
            return;
        }

        self.toggle_simple_fullscreen();
    }

    fn set_resize_increments(&self, incr: ResizeIncrement) {
        let min_width = incr.base_width + incr.x;
        let min_height = incr.base_height + incr.y;
        unsafe {
            self.window
                .setResizeIncrements_(NSSize::new(incr.x.into(), incr.y.into()));
            let () = msg_send![
                *self.window,
                setContentMinSize: NSSize::new(min_width.into(), min_height.into())
            ];
        }
    }

    fn config_did_change(&mut self, config: &ConfigHandle) {
        let dpi_changed =
            self.config.dpi != config.dpi || self.config.dpi_by_screen != config.dpi_by_screen;

        self.config = config.clone();
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            let mut inner = window_view.inner.borrow_mut();
            inner.config = config.clone();
            if dpi_changed {
                inner.screen_changed = true;
            }
        }
        self.update_window_shadow();
        self.update_window_background_blur();
        self.update_titlebar_background();
        apply_window_appearance(&self.window, &self.config);
        self.apply_decorations();
    }
}

/// Pin the NSWindow's appearance to match the resolved color scheme. Without
/// this, NSTitlebarContainerView's vibrancy material renders one frame using
/// the system appearance before the GPU layer presents real content, causing
/// a light strip to flash at the top of a dark-themed window on cold start
/// (and vice versa). When no theme background is set, fall back to nil so the
/// window keeps following the system, preserving the prior behavior.
fn apply_window_appearance(window: &StrongPtr, config: &ConfigHandle) {
    let appearance_name = config.resolved_palette.background.map(|bg| {
        if is_light_color(&bg) {
            "NSAppearanceNameAqua"
        } else {
            "NSAppearanceNameDarkAqua"
        }
    });

    unsafe {
        let appearance: id = match appearance_name {
            Some(name) => msg_send![class!(NSAppearance), appearanceNamed: *nsstring(name)],
            None => nil,
        };
        // setAppearance: with a new value fires viewDidChangeEffectiveAppearance
        // on the content view, which re-enters the AppearanceChanged path.
        let current: id = msg_send![**window, appearance];
        let same = if appearance.is_null() && current.is_null() {
            true
        } else if appearance.is_null() || current.is_null() {
            false
        } else {
            let eq: BOOL = msg_send![appearance, isEqual: current];
            eq == YES
        };
        if !same {
            let _: () = msg_send![**window, setAppearance: appearance];
        }
    }
}

fn effective_decorations(
    mut decorations: WindowDecorations,
    integrated_title_button_style: IntegratedTitleButtonStyle,
) -> WindowDecorations {
    if integrated_title_button_style != IntegratedTitleButtonStyle::MacOsNative {
        decorations.remove(WindowDecorations::INTEGRATED_BUTTONS);
    }
    decorations
}

fn apply_decorations_to_window(
    window: &StrongPtr,
    decorations: WindowDecorations,
    integrated_title_button_style: IntegratedTitleButtonStyle,
    native_macos_fullscreen_mode: bool,
) {
    let mask = decoration_to_mask(decorations, integrated_title_button_style);
    let decorations = effective_decorations(decorations, integrated_title_button_style);
    unsafe {
        window.setStyleMask_(mask);
        let mut behavior = window.collectionBehavior();
        if native_macos_fullscreen_mode {
            behavior.remove(
                appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
            );
            behavior.insert(
                appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary,
            );
            window.setCollectionBehavior_(behavior);
        } else {
            behavior.remove(
                appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
                    | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
            );
            window.setCollectionBehavior_(behavior);
        }

        let hidden = if decorations.contains(WindowDecorations::TITLE)
            || decorations.contains(WindowDecorations::INTEGRATED_BUTTONS)
        {
            NO
        } else {
            YES
        };

        for titlebar_button in &[
            appkit::NSWindowButton::NSWindowMiniaturizeButton,
            appkit::NSWindowButton::NSWindowCloseButton,
            appkit::NSWindowButton::NSWindowZoomButton,
        ] {
            let button = window.standardWindowButton_(*titlebar_button);
            let _: () = msg_send![button, setHidden: hidden];
        }

        window.setTitleVisibility_(if decorations.contains(WindowDecorations::TITLE) {
            appkit::NSWindowTitleVisibility::NSWindowTitleVisible
        } else {
            appkit::NSWindowTitleVisibility::NSWindowTitleHidden
        });

        if decorations.contains(WindowDecorations::INTEGRATED_BUTTONS)
            || decorations.contains(WindowDecorations::MACOS_USE_BACKGROUND_COLOR_AS_TITLEBAR_COLOR)
        {
            window.setTitlebarAppearsTransparent_(YES);
            // NSTitlebarSeparatorStyleNone = 1; removes the 1px separator
            // line that macOS draws at the bottom of the titlebar area.
            let _: () = msg_send![**window, setTitlebarSeparatorStyle: 1i64];
        } else {
            window.setTitlebarAppearsTransparent_(hidden);
        }
    }
}

fn decoration_to_mask(
    decorations: WindowDecorations,
    integrated_title_button_style: IntegratedTitleButtonStyle,
) -> NSWindowStyleMask {
    let decorations = effective_decorations(decorations, integrated_title_button_style);
    let decorations = decorations.difference(
        WindowDecorations::MACOS_FORCE_DISABLE_SHADOW
            | WindowDecorations::MACOS_FORCE_ENABLE_SHADOW,
    );
    if decorations == WindowDecorations::TITLE | WindowDecorations::RESIZE {
        NSWindowStyleMask::NSTitledWindowMask
            | NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask
            | NSWindowStyleMask::NSResizableWindowMask
    } else if decorations
        == WindowDecorations::MACOS_FORCE_SQUARE_CORNERS | WindowDecorations::RESIZE
    {
        NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask
            | NSWindowStyleMask::NSResizableWindowMask
            | NSWindowStyleMask::NSFullSizeContentViewWindowMask
    } else if decorations == WindowDecorations::RESIZE
        || decorations == WindowDecorations::INTEGRATED_BUTTONS
        || decorations == WindowDecorations::INTEGRATED_BUTTONS | WindowDecorations::RESIZE
    {
        NSWindowStyleMask::NSTitledWindowMask
            | NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask
            | NSWindowStyleMask::NSResizableWindowMask
            | NSWindowStyleMask::NSFullSizeContentViewWindowMask
    } else if decorations == WindowDecorations::NONE {
        NSWindowStyleMask::NSBorderlessWindowMask
    } else if decorations == WindowDecorations::TITLE {
        NSWindowStyleMask::NSTitledWindowMask
            | NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask
    } else if decorations == WindowDecorations::MACOS_FORCE_SQUARE_CORNERS {
        NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask
            | NSWindowStyleMask::NSFullSizeContentViewWindowMask
    } else {
        NSWindowStyleMask::NSTitledWindowMask
            | NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSMiniaturizableWindowMask
            | NSWindowStyleMask::NSResizableWindowMask
    }
}

unsafe fn get_view_class_name(id: id) -> Option<String> {
    if id.is_null() {
        return None;
    }

    let class_name: id = msg_send![id, className];

    if class_name.is_null() {
        return None;
    }

    let cstr = CStr::from_ptr(class_name.UTF8String()).to_str();

    match cstr {
        Ok(s) => Some(s.to_string()),
        Err(_) => None,
    }
}

fn get_titlebar_view_container(window: &StrongPtr) -> Option<WeakPtr> {
    // The view container for the titlebar on macos is found next to the primary window view
    // so we need to traverse up to the super view to find it
    let super_view = get_view_superview(window)?;

    let sub_views = get_view_subviews(&super_view.load())?;

    let count = unsafe { sub_views.load().count() };

    for i in 0..count {
        let sub_view: id = unsafe { sub_views.load().objectAtIndex(i) };

        if sub_view.is_null() {
            continue;
        }

        let class_name = unsafe { get_view_class_name(sub_view)? };

        if class_name == TITLEBAR_VIEW_NAME {
            let titlebar_view = unsafe { WeakPtr::new(sub_view) };
            return Some(titlebar_view);
        }
    }

    None
}

fn get_view_superview(view: &StrongPtr) -> Option<WeakPtr> {
    let super_view_id: id = unsafe { msg_send![view.contentView(), superview] };

    if super_view_id.is_null() {
        return None;
    }

    let super_view = unsafe { WeakPtr::new(super_view_id) };

    Some(super_view)
}

fn get_view_subviews(view: &StrongPtr) -> Option<WeakPtr> {
    let sub_views_id: id = unsafe { msg_send![**view, subviews] };
    if sub_views_id.is_null() {
        return None;
    }

    let sub_views = unsafe { WeakPtr::new(sub_views_id) };
    Some(sub_views)
}

#[derive(Debug)]
struct DeadKeyState {
    /// The private dead key state preserved from UCKeyTranslate
    dead_state: u32,
}

struct Inner {
    events: WindowEventSender,
    view_id: Option<WeakPtr>,
    window: Option<WeakPtr>,
    screen_changed: bool,
    paint_throttled: bool,
    window_id: usize,
    invalidated: bool,
    gl_context_pair: Option<GlContextPair>,
    text_cursor_position: Rect,
    tracking_rect_tag: NSInteger,
    hscroll_remainder: f64,
    vscroll_remainder: f64,
    last_wheel: Instant,
    /// We use this to avoid double-emitting events when
    /// procesing key-up events.
    key_is_down: Option<bool>,

    /// First in a dead-key sequence
    dead_pending: Option<DeadKeyState>,

    /// When using simple fullscreen mode, this tracks
    /// the window dimensions that need to be restored
    fullscreen: Option<NSRect>,

    config: ConfigHandle,

    /// Used to signal when IME really just swallowed a key
    ime_state: ImeDisposition,
    /// Captures the last event that had ImeDisposition::Acted,
    /// so that we can use it to generate a repeat in the cases
    /// where the IME mysteriously swallows repeats but only
    /// for certain keys.
    ime_last_event: Option<KeyEvent>,

    /// Whether we're in live resize
    live_resizing: bool,
    /// Last stable dpi dispatched to the gui layer.
    last_reported_dpi: Option<usize>,
    /// Last window state dispatched to the gui layer.
    last_reported_window_state: WindowState,

    ime_text: String,
}

#[repr(C)]
pub struct __InputSource {
    _dummy: i32,
}
pub type InputSourceRef = *const __InputSource;

declare_TCFType!(InputSource, InputSourceRef);
impl_TCFType!(InputSource, InputSourceRef, TISInputSourceGetTypeID);

#[repr(C)]
struct UCKeyboardLayout {
    _dummy: i32,
}

type UniCharCount = std::os::raw::c_ulong;

/// key is going down
#[allow(non_upper_case_globals)]
const kUCKeyActionDown: u16 = 0;
/// key is going up
#[allow(non_upper_case_globals, dead_code)]
const kUCKeyActionUp: u16 = 1;
/// auto-key down
#[allow(non_upper_case_globals, dead_code)]
const kUCKeyActionAutoKey: u16 = 2;
/// get information for key display (as in Key Caps)
#[allow(non_upper_case_globals)]
const kUCKeyActionDisplay: u16 = 3;

extern "C" {
    fn TISInputSourceGetTypeID() -> CFTypeID;
    fn TISCopyCurrentKeyboardInputSource() -> InputSourceRef;
    fn TISGetInputSourceProperty(source: InputSourceRef, propertyKey: CFStringRef) -> CFDataRef;

    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;

    fn UCKeyTranslate(
        layout: *const UCKeyboardLayout,
        virtualKeyCode: u16,
        keyAction: u16,
        modifierKeyState: u32,
        keyboardType: u32,
        keyTranslateOptions: u32,
        deadKeyState: *mut u32,
        maxStringLength: UniCharCount,
        actualStringLength: *mut UniCharCount,
        unicodeString: *mut UniChar,
    ) -> u32;

    fn LMGetKbdType() -> u8;
}

#[derive(Debug)]
enum TranslateStatus {
    Composing(String),
    Composed(String),
    NotDead,
}

/// Represents the current keyboard layout.
/// Holds state needed to perform keymap translation.
struct Keyboard {
    _kbd: InputSource,
    layout_data: Option<CFData>,
}

/// Slightly more intelligible parameters for keymap translation
struct TranslateParams {
    virtual_key_code: u16,
    modifier_flags: NSEventModifierFlags,
    dead_state: u32,
    ignore_dead_keys: bool,
    display: bool,
}

/// The results of a keymap translation
#[derive(Debug)]
struct TranslateResults {
    dead_state: u32,
    text: String,
}

fn is_unicode_noncharacter(c: char) -> bool {
    let cp = c as u32;
    (0xFDD0..=0xFDEF).contains(&cp) || (cp & 0xFFFE) == 0xFFFE
}

fn is_dead_key_placeholder_text(s: &str) -> bool {
    // Some keyboard layouts return multiple Unicode noncharacters for dead keys.
    // Treat the string as a dead-key placeholder if it's non-empty and
    // consists entirely of Unicode noncharacters.
    !s.is_empty() && s.chars().all(is_unicode_noncharacter)
}

impl Keyboard {
    pub fn new() -> Self {
        let _kbd =
            unsafe { InputSource::wrap_under_create_rule(TISCopyCurrentKeyboardInputSource()) };

        let layout_data = unsafe {
            let data = TISGetInputSourceProperty(
                _kbd.as_concrete_TypeRef(),
                kTISPropertyUnicodeKeyLayoutData,
            );
            if data.is_null() {
                None
            } else {
                Some(CFData::wrap_under_get_rule(data))
            }
        };
        Self { _kbd, layout_data }
    }

    /// A wrapper around UCKeyTranslate
    pub fn translate(&self, params: TranslateParams) -> anyhow::Result<TranslateResults> {
        let layout_data = match &self.layout_data {
            Some(data) => unsafe {
                CFDataGetBytePtr(data.as_concrete_TypeRef()) as *const UCKeyboardLayout
            },
            None => std::ptr::null(),
        };

        let modifier_key_state: u32 = (params.modifier_flags.bits() >> 16) as u32 & 0xFF;

        let kbd_type = unsafe { LMGetKbdType() } as _;
        #[allow(non_upper_case_globals)]
        const kUCKeyTranslateNoDeadKeysBit: u32 = 0;

        let mut unicode_buffer = [0u16; 32];
        let mut length = 0;
        let mut dead_state = params.dead_state;
        unsafe {
            UCKeyTranslate(
                layout_data,
                params.virtual_key_code,
                if params.display {
                    kUCKeyActionDisplay
                } else {
                    kUCKeyActionDown
                },
                modifier_key_state,
                kbd_type,
                if params.ignore_dead_keys {
                    1 << kUCKeyTranslateNoDeadKeysBit
                } else {
                    0
                },
                &mut dead_state,
                unicode_buffer.len() as _,
                &mut length,
                unicode_buffer.as_mut_ptr(),
            )
        };

        let mut text = String::from_utf16(unsafe {
            std::slice::from_raw_parts(unicode_buffer.as_mut_ptr(), length as _)
        })?;
        if is_dead_key_placeholder_text(&text) {
            text.clear();
        }

        Ok(TranslateResults { text, dead_state })
    }
}

impl Inner {
    fn enable_opengl(&mut self) -> anyhow::Result<Rc<glium::backend::Context>> {
        let view = self.view_id.as_ref().unwrap().load();
        let glium_context = GlContextPair::create(*view)?;

        self.gl_context_pair.replace(glium_context.clone());

        Ok(glium_context.context)
    }

    /// <https://stackoverflow.com/a/22677690>
    /// <https://stackoverflow.com/a/12548163>
    /// <https://stackoverflow.com/a/8263841>
    /// <https://developer.apple.com/documentation/coreservices/1390584-uckeytranslate?language=objc>
    fn translate_key_event(
        &mut self,
        virtual_key_code: u16,
        modifier_flags: NSEventModifierFlags,
        force_dead_keys: bool,
    ) -> anyhow::Result<TranslateStatus> {
        let keyboard = Keyboard::new();

        let mods = key_modifiers(modifier_flags);

        let config = &self.config;

        let use_dead_keys = if !config.use_dead_keys {
            false
        } else if force_dead_keys {
            true
        } else if mods.contains(Modifiers::LEFT_ALT) {
            config.send_composed_key_when_left_alt_is_pressed
        } else if mods.contains(Modifiers::RIGHT_ALT) {
            config.send_composed_key_when_right_alt_is_pressed
        } else {
            true
        };

        if let Some(DeadKeyState { dead_state }) = self.dead_pending.take() {
            let result = keyboard.translate(TranslateParams {
                virtual_key_code,
                modifier_flags,
                dead_state,
                ignore_dead_keys: false,
                display: true,
            })?;

            // If length == 0 it means that they double-pressed the dead key.
            // We treat that the same as the dead key disabled state:
            // we want to clock through a space keypress so that we clear
            // the state and output the original keypress.
            let generate_space = !use_dead_keys || result.text.len() == 0;

            if generate_space {
                // synthesize a SPACE press to
                // elicit the underlying key code and get out
                // of the dead key state
                let result = keyboard.translate(TranslateParams {
                    virtual_key_code,
                    modifier_flags,
                    dead_state: result.dead_state,
                    ignore_dead_keys: false,
                    display: false,
                })?;
                Ok(TranslateStatus::Composed(result.text))
            } else {
                Ok(TranslateStatus::Composed(result.text))
            }
        } else if use_dead_keys {
            let result = keyboard.translate(TranslateParams {
                virtual_key_code,
                modifier_flags,
                dead_state: 0,
                ignore_dead_keys: false,
                display: false,
            })?;

            self.dead_pending.replace(DeadKeyState {
                dead_state: result.dead_state,
            });

            // Get the non-dead-key rendition to show as the composing state
            let composing = keyboard.translate(TranslateParams {
                virtual_key_code,
                modifier_flags,
                dead_state: 0,
                ignore_dead_keys: true,
                display: true,
            })?;

            Ok(TranslateStatus::Composing(composing.text))
        } else {
            Ok(TranslateStatus::NotDead)
        }
    }
}

const VIEW_CLS_NAME: &str = "KakuWindowView";
const WINDOW_CLS_NAME: &str = "KakuWindow";
const TITLEBAR_VIEW_NAME: &str = "NSTitlebarContainerView";

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct NSEdgeInsets {
    top: CGFloat,
    left: CGFloat,
    bottom: CGFloat,
    right: CGFloat,
}

fn get_screen_safe_area_insets(screen: id) -> Option<NSEdgeInsets> {
    let has_safe_area_insets: BOOL =
        unsafe { msg_send![screen, respondsToSelector: sel!(safeAreaInsets)] };
    if has_safe_area_insets == YES {
        let insets: NSEdgeInsets = unsafe { msg_send![screen, safeAreaInsets] };
        Some(insets)
    } else {
        None
    }
}

fn simple_fullscreen_target_rect(screen: id, extend_behind_notch: bool) -> NSRect {
    unsafe {
        let screen_rect = NSScreen::frame(screen);
        let _ = extend_behind_notch;
        screen_rect
    }
}

struct WindowView {
    inner: Rc<RefCell<Inner>>,
    /// Window ID stored outside RefCell so window_will_close can access it even
    /// when a borrow is already held by a caller higher on the stack.
    window_id: Cell<usize>,
    /// Tracks simple fullscreen state without requiring RefCell borrow.
    simple_fullscreen_active: Cell<bool>,
    /// Tracks simple fullscreen transition state without requiring RefCell borrow.
    simple_fullscreen_transition_active: Cell<bool>,
    /// Keep pane content hidden for a short time during fullscreen transitions.
    transition_hide_until: Cell<Option<Instant>>,
    /// Delay CGL presents briefly after any display change or system wake so
    /// AppKit can rebuild the backing drawable before we call flushBuffer.
    display_change_opengl_present_until: Cell<Option<Instant>>,
    /// Tracks native fullscreen transition state so we can stabilize resize behavior.
    native_fullscreen_transition_active: Cell<bool>,
    /// Target fullscreen state while native transition is running.
    native_fullscreen_target: Cell<Option<bool>>,
    native_fullscreen_transition_start: Cell<Option<Instant>>,
    resize_retry_scheduled: Cell<bool>,
    /// Set when window_will_close fires; prevents fullscreen transition
    /// handlers from dispatching events to an already-destroyed window.
    is_closing: Cell<bool>,
    /// Queued by order_out for a native-fullscreen window; consumed by
    /// did_exit_fullscreen to finish hiding once the Space exits.
    order_out_on_fullscreen_exit: Cell<bool>,
}

fn arm_display_change_opengl_present_defer(
    window_view: &WindowView,
    window_id: usize,
    duration_ms: u64,
) {
    let now = Instant::now();
    let until = now + Duration::from_millis(duration_ms);
    if window_view
        .display_change_opengl_present_until
        .get()
        .is_some_and(|current| current >= until)
    {
        return;
    }

    window_view
        .display_change_opengl_present_until
        .set(Some(until));

    promise::spawn::spawn(async move {
        async_io::Timer::after(Duration::from_millis(duration_ms)).await;
        if let Err(err) = Connection::with_window_inner(window_id, move |inner| {
            if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                let Some(until) = window_view.display_change_opengl_present_until.get() else {
                    return Ok(());
                };
                if Instant::now() < until {
                    return Ok(());
                }
                window_view.display_change_opengl_present_until.set(None);
                if let Ok(mut state) = window_view.inner.try_borrow_mut() {
                    state.paint_throttled = false;
                    state.invalidated = true;
                    let events = state.events.clone();
                    drop(state);
                    events.dispatch(WindowEvent::NeedRepaint);
                }
            }
            unsafe {
                let _: () = msg_send![*inner.view, setNeedsDisplay: YES];
            }
            Ok(())
        })
        .await
        {
            log::trace!(
                "skipping deferred display-change repaint for window {}: {}",
                window_id,
                err
            );
        }
    })
    .detach();
}

pub fn superclass(this: &Object) -> &'static Class {
    unsafe {
        let superclass: id = msg_send![this, superclass];
        &*(superclass as *const _)
    }
}

fn dpi_for_window_screen(ns_window: *mut Object, config: &ConfigHandle) -> Option<f64> {
    if config.dpi_by_screen.is_empty() {
        return config.dpi;
    }

    let screen = unsafe { msg_send![ns_window, screen] };
    let info = crate::os::macos::connection::nsscreen_to_screen_info(screen);

    config.dpi_by_screen.get(&info.name).copied()
}

#[allow(clippy::identity_op)]
fn decode_mouse_buttons(mask: u64) -> MouseButtons {
    let mut buttons = MouseButtons::NONE;

    if (mask & (1 << 0)) != 0 {
        buttons |= MouseButtons::LEFT;
    }
    if (mask & (1 << 1)) != 0 {
        buttons |= MouseButtons::RIGHT;
    }
    if (mask & (1 << 2)) != 0 {
        buttons |= MouseButtons::MIDDLE;
    }
    if (mask & (1 << 3)) != 0 {
        buttons |= MouseButtons::X1;
    }
    if (mask & (1 << 4)) != 0 {
        buttons |= MouseButtons::X2;
    }
    buttons
}

fn key_modifiers(flags: NSEventModifierFlags) -> Modifiers {
    let mut mods = Modifiers::NONE;

    if flags.contains(NSEventModifierFlags::NSShiftKeyMask) {
        mods |= Modifiers::SHIFT;
    }
    if flags.contains(NSEventModifierFlags::NSAlternateKeyMask) && (flags.bits() & 0x20) != 0 {
        mods |= Modifiers::LEFT_ALT | Modifiers::ALT;
    }
    if flags.contains(NSEventModifierFlags::NSAlternateKeyMask) && (flags.bits() & 0x40) != 0 {
        mods |= Modifiers::RIGHT_ALT | Modifiers::ALT;
    }
    if flags.contains(NSEventModifierFlags::NSControlKeyMask) {
        mods |= Modifiers::CTRL;
    }
    if flags.contains(NSEventModifierFlags::NSCommandKeyMask) {
        mods |= Modifiers::SUPER;
    }

    mods
}

fn is_function_virtual_key(vkey: u16) -> bool {
    [
        kVK_F1, kVK_F2, kVK_F3, kVK_F4, kVK_F5, kVK_F6, kVK_F7, kVK_F8, kVK_F9, kVK_F10, kVK_F11,
        kVK_F12, kVK_F13, kVK_F14, kVK_F15, kVK_F16, kVK_F17, kVK_F18, kVK_F19, kVK_F20,
    ]
    .contains(&vkey)
}

fn is_navigation_virtual_key(vkey: u16) -> bool {
    [
        kVK_LeftArrow,
        kVK_RightArrow,
        kVK_UpArrow,
        kVK_DownArrow,
        kVK_Home,
        kVK_End,
        kVK_PageUp,
        kVK_PageDown,
        kVK_ForwardDelete,
        kVK_Help,
    ]
    .contains(&vkey)
}

/// Returns true for virtual key codes that never correspond to macOS menu
/// shortcuts: arrows, navigation keys (Home/End/PageUp/PageDown/Delete/Help),
/// and function keys (F1-F20). Safe to intercept in performKeyEquivalent
/// when Command is held without breaking standard menu items.
fn is_non_menu_virtual_key(vkey: u16) -> bool {
    is_navigation_virtual_key(vkey) || is_function_virtual_key(vkey)
}

fn is_symbol_virtual_key(vkey: u16) -> bool {
    [
        kVK_ANSI_Comma,
        kVK_ANSI_Period,
        kVK_ANSI_Slash,
        kVK_ANSI_Semicolon,
        kVK_ANSI_Quote,
        kVK_ANSI_LeftBracket,
        kVK_ANSI_RightBracket,
        kVK_ANSI_Backslash,
        kVK_ANSI_Grave,
        kVK_ANSI_Minus,
        kVK_ANSI_Equal,
    ]
    .contains(&vkey)
}

fn is_alnum_virtual_key(vkey: u16) -> bool {
    [
        kVK_ANSI_A, kVK_ANSI_B, kVK_ANSI_C, kVK_ANSI_D, kVK_ANSI_E, kVK_ANSI_F, kVK_ANSI_G,
        kVK_ANSI_H, kVK_ANSI_I, kVK_ANSI_J, kVK_ANSI_K, kVK_ANSI_L, kVK_ANSI_M, kVK_ANSI_N,
        kVK_ANSI_O, kVK_ANSI_P, kVK_ANSI_Q, kVK_ANSI_R, kVK_ANSI_S, kVK_ANSI_T, kVK_ANSI_U,
        kVK_ANSI_V, kVK_ANSI_W, kVK_ANSI_X, kVK_ANSI_Y, kVK_ANSI_Z, kVK_ANSI_0, kVK_ANSI_1,
        kVK_ANSI_2, kVK_ANSI_3, kVK_ANSI_4, kVK_ANSI_5, kVK_ANSI_6, kVK_ANSI_7, kVK_ANSI_8,
        kVK_ANSI_9,
    ]
    .contains(&vkey)
}

fn is_ascii_punctuation_text(s: &str) -> bool {
    let mut chars = s.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_punctuation())
}

fn is_ascii_letter_text(s: &str) -> bool {
    let mut chars = s.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_alphabetic())
}

fn is_command_alnum_shortcut(unmod: &str, modifiers: Modifiers, virtual_key: u16) -> bool {
    // Require Cmd (SUPER), disallow Alt/Ctrl (Shift is permitted).
    // Prevents Cmd+alnum shortcuts from failing when a non-Latin IME is active,
    // because macOS NSMenu keyEquivalent matching can return the wrong character.
    // Some keyboard layouts (for example AZERTY) place punctuation on ANSI
    // letter vkeys such as kVK_ANSI_M. If the unmodified text is punctuation,
    // treat it as a symbol shortcut instead of forcing the alnum path.
    let must_have = Modifiers::SUPER;
    let must_not = Modifiers::ALT | Modifiers::CTRL | Modifiers::LEFT_ALT | Modifiers::RIGHT_ALT;
    modifiers.contains(must_have)
        && !modifiers.intersects(must_not)
        && is_alnum_virtual_key(virtual_key)
        && !is_ascii_punctuation_text(unmod)
}

fn should_intercept_special_shortcut(chars: &str, modifiers: Modifiers, virtual_key: u16) -> bool {
    let command_period = virtual_key == kVK_ANSI_Period && modifiers == Modifiers::SUPER;
    let command_shift_symbol = modifiers == (Modifiers::SUPER | Modifiers::SHIFT)
        && is_symbol_virtual_key(virtual_key)
        // Preserve macOS built-in Cmd+` and Cmd+Shift+` window cycling.
        && virtual_key != kVK_ANSI_Grave;
    // Intercept Cmd+Shift+Space so the system "select previous input source"
    // shortcut cannot consume it before Kaku's own kaku-ai-chat binding fires.
    let command_shift_space =
        modifiers == (Modifiers::SUPER | Modifiers::SHIFT) && virtual_key == kVK_Space;

    command_period
        || command_shift_symbol
        || command_shift_space
        || (chars == "\u{1b}" && modifiers == Modifiers::CTRL)
        || (chars == "\t" && modifiers == Modifiers::CTRL)
        || (chars == "\x19"/* Shift-Tab: See issue #1902 */)
}

fn should_intercept_perform_key_equivalent(
    chars: &str,
    unmod: &str,
    modifiers: Modifiers,
    virtual_key: u16,
) -> bool {
    // Route these combinations through key_common so command shortcuts remain
    // stable even when NSMenu keyEquivalent matching is unreliable under IME.
    let special_shortcut = should_intercept_special_shortcut(chars, modifiers, virtual_key);
    let command_alnum_shortcut = is_command_alnum_shortcut(unmod, modifiers, virtual_key);
    let command_non_menu_key =
        modifiers.contains(Modifiers::SUPER) && is_non_menu_virtual_key(virtual_key);

    special_shortcut || command_non_menu_key || command_alnum_shortcut
}

fn should_clear_modifiers_for_empty_unmod(
    unmod: &str,
    modifiers: Modifiers,
    virtual_key: u16,
) -> bool {
    unmod.is_empty() && !is_command_alnum_shortcut(unmod, modifiers, virtual_key)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn non_menu_virtual_keys_are_recognized() {
        let keys = [
            kVK_LeftArrow,
            kVK_RightArrow,
            kVK_UpArrow,
            kVK_DownArrow,
            kVK_Home,
            kVK_End,
            kVK_PageUp,
            kVK_PageDown,
            kVK_ForwardDelete,
            kVK_Help,
            kVK_F1,
            kVK_F2,
            kVK_F3,
            kVK_F4,
            kVK_F5,
            kVK_F6,
            kVK_F7,
            kVK_F8,
            kVK_F9,
            kVK_F10,
            kVK_F11,
            kVK_F12,
            kVK_F13,
            kVK_F14,
            kVK_F15,
            kVK_F16,
            kVK_F17,
            kVK_F18,
            kVK_F19,
            kVK_F20,
        ];

        for &key in &keys {
            assert!(
                is_non_menu_virtual_key(key),
                "vkey {:#x} must be recognized",
                key
            );
        }
    }

    #[test]
    fn menu_or_character_virtual_keys_are_not_recognized() {
        let keys = [
            kVK_ANSI_A,
            kVK_ANSI_Grave,
            kVK_Tab,
            kVK_Return,
            kVK_Escape,
            kVK_Command,
            kVK_Option,
            // JIS Eisu/Kana key positions should not be captured by default.
            0x66,
            0x68,
        ];

        for &key in &keys {
            assert!(
                !is_non_menu_virtual_key(key),
                "vkey {:#x} must not be recognized",
                key
            );
        }
    }

    #[test]
    fn special_shortcut_matching_uses_stable_symbol_vkeys() {
        assert!(should_intercept_special_shortcut(
            ".",
            Modifiers::SUPER,
            kVK_ANSI_Period,
        ));
        assert!(should_intercept_special_shortcut(
            ",",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_Comma,
        ));
        assert!(should_intercept_special_shortcut(
            "<",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_Comma,
        ));

        assert!(!should_intercept_special_shortcut(
            ",",
            Modifiers::SUPER,
            kVK_ANSI_Comma,
        ));
        // Cmd+Shift+. should be intercepted (symbol key)
        assert!(should_intercept_special_shortcut(
            ">",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_Period,
        ));
        // Cmd+Shift+` should NOT be intercepted (window cycling)
        assert!(!should_intercept_special_shortcut(
            "~",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_Grave,
        ));
    }

    #[test]
    fn command_alnum_shortcuts_are_stable_by_virtual_key() {
        // Cmd+letter
        assert!(is_command_alnum_shortcut("w", Modifiers::SUPER, kVK_ANSI_W));
        assert!(is_command_alnum_shortcut("k", Modifiers::SUPER, kVK_ANSI_K));
        assert!(is_command_alnum_shortcut("1", Modifiers::SUPER, kVK_ANSI_1));
        // Cmd+Shift+letter (Shift is allowed)
        assert!(is_command_alnum_shortcut(
            "D",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_D,
        ));
        assert!(is_command_alnum_shortcut(
            "A",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_A,
        ));
        // Cmd+Alt+letter → false (Alt combos serve different purposes)
        assert!(!is_command_alnum_shortcut(
            "w",
            Modifiers::SUPER | Modifiers::ALT | Modifiers::LEFT_ALT,
            kVK_ANSI_W,
        ));
        // Non-alnum key → false
        assert!(!is_command_alnum_shortcut(
            "`",
            Modifiers::SUPER,
            kVK_ANSI_Grave,
        ));
    }

    #[test]
    fn command_alnum_shortcut_ignores_layout_symbol_on_alnum_vkey() {
        // Layouts like AZERTY can place "," on kVK_ANSI_M. Do not coerce
        // this into Cmd+M alnum handling.
        assert!(!is_command_alnum_shortcut(
            ",",
            Modifiers::SUPER,
            kVK_ANSI_M
        ));
    }

    #[test]
    fn command_alnum_shortcut_still_matches_letter_on_remapped_vkey() {
        // AZERTY "a" sits on kVK_ANSI_Q; must still intercept so NSMenu's
        // non-Latin-IME mismatch path cannot run.
        assert!(is_command_alnum_shortcut("a", Modifiers::SUPER, kVK_ANSI_Q));
        assert!(is_command_alnum_shortcut("z", Modifiers::SUPER, kVK_ANSI_W));
    }

    #[test]
    fn ascii_letter_text_matches_only_single_latin_letters() {
        assert!(is_ascii_letter_text("a"));
        assert!(is_ascii_letter_text("Z"));
        assert!(!is_ascii_letter_text(""));
        assert!(!is_ascii_letter_text("ab"));
        assert!(!is_ascii_letter_text(","));
        assert!(!is_ascii_letter_text("1"));
        assert!(!is_ascii_letter_text("\u{3148}"));
        assert!(!is_ascii_letter_text("\u{00e9}"));
    }

    #[test]
    fn perform_key_equivalent_intercept_matrix_for_cmd_alnum() {
        // Core Cmd+alnum combinations should be intercepted.
        assert!(should_intercept_perform_key_equivalent(
            "w",
            "w",
            Modifiers::SUPER,
            kVK_ANSI_W,
        ));
        assert!(should_intercept_perform_key_equivalent(
            "D",
            "D",
            Modifiers::SUPER | Modifiers::SHIFT,
            kVK_ANSI_D,
        ));
        assert!(should_intercept_perform_key_equivalent(
            "1",
            "1",
            Modifiers::SUPER,
            kVK_ANSI_1,
        ));

        // Cmd with extra modifiers (Alt/Ctrl) should not be treated as Cmd+alnum.
        assert!(!should_intercept_perform_key_equivalent(
            "w",
            "w",
            Modifiers::SUPER | Modifiers::ALT | Modifiers::LEFT_ALT,
            kVK_ANSI_W,
        ));
        assert!(!should_intercept_perform_key_equivalent(
            "w",
            "w",
            Modifiers::SUPER | Modifiers::CTRL,
            kVK_ANSI_W,
        ));

        // Preserve macOS window cycling on Cmd+`.
        assert!(!should_intercept_perform_key_equivalent(
            "`",
            "`",
            Modifiers::SUPER,
            kVK_ANSI_Grave,
        ));

        // AZERTY-like punctuation on an alnum vkey should not be intercepted
        // as Cmd+alnum, so the correct menu shortcut can run.
        assert!(!should_intercept_perform_key_equivalent(
            ",",
            ",",
            Modifiers::SUPER,
            kVK_ANSI_M,
        ));
    }

    #[test]
    fn empty_unmod_modifier_clearing_respects_cmd_alnum() {
        assert!(should_clear_modifiers_for_empty_unmod(
            "",
            Modifiers::CTRL,
            kVK_ANSI_RightBracket,
        ));
        assert!(!should_clear_modifiers_for_empty_unmod(
            "",
            Modifiers::SUPER,
            kVK_ANSI_W,
        ));
        assert!(!should_clear_modifiers_for_empty_unmod(
            "w",
            Modifiers::SUPER,
            kVK_ANSI_W,
        ));
    }

    #[test]
    fn native_drag_is_disabled_for_fullscreen_and_maximized_frames() {
        assert!(should_perform_native_window_drag(false, false, false));
        assert!(!should_perform_native_window_drag(true, false, false));
        assert!(!should_perform_native_window_drag(false, true, false));
        assert!(!should_perform_native_window_drag(false, false, true));
    }

    #[test]
    fn oversized_window_frame_is_fit_to_visible_frame() {
        let frame = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(1920.0, 1080.0));
        let visible = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0));

        let adjusted = fit_frame_to_visible_frame(frame, visible).unwrap();

        assert_eq!(adjusted.origin.x, 0.0);
        assert_eq!(adjusted.origin.y, 0.0);
        assert_eq!(adjusted.size.width, 1440.0);
        assert_eq!(adjusted.size.height, 900.0);
    }

    #[test]
    fn offscreen_window_frame_is_moved_into_visible_frame() {
        let frame = NSRect::new(NSPoint::new(1800.0, 900.0), NSSize::new(800.0, 600.0));
        let visible = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0));

        let adjusted = fit_frame_to_visible_frame(frame, visible).unwrap();

        assert_eq!(adjusted.origin.x, 640.0);
        assert_eq!(adjusted.origin.y, 300.0);
        assert_eq!(adjusted.size.width, 800.0);
        assert_eq!(adjusted.size.height, 600.0);
    }

    #[test]
    fn none_decorations_use_a_borderless_window() {
        let mask = decoration_to_mask(
            WindowDecorations::NONE,
            IntegratedTitleButtonStyle::MacOsNative,
        );

        assert_eq!(mask, NSWindowStyleMask::NSBorderlessWindowMask);
    }
}

/// We register our own subclass of NSWindow so that we can override
/// canBecomeKeyWindow so that our simple fullscreen style can keep
/// focus once the titlebar has been removed; the default behavior of
/// NSWindow is to reject focus when it doesn't have a titlebar!
fn get_window_class() -> &'static Class {
    Class::get(WINDOW_CLS_NAME).unwrap_or_else(|| {
        let mut cls = ClassDecl::new(WINDOW_CLS_NAME, class!(NSWindow))
            .expect("Unable to register Window class");

        extern "C" fn yes(_: &mut Object, _: Sel) -> BOOL {
            YES
        }

        extern "C" fn redirect_toggle_fullscreen(this: &mut Object, _sel: Sel, sender: id) {
            let content_view: id = unsafe { msg_send![this, contentView] };
            if !content_view.is_null() {
                if let Some(window_view) = unsafe { WindowView::get_this(&*content_view) } {
                    let (window_id, use_native) = {
                        let inner = window_view.inner.borrow();
                        (inner.window_id, inner.config.native_macos_fullscreen_mode)
                    };
                    if use_native {
                        unsafe {
                            let () =
                                msg_send![super(this, class!(NSWindow)), toggleFullScreen: sender];
                        }
                        return;
                    }
                    Connection::with_window_inner(window_id, move |inner| {
                        inner.toggle_fullscreen();
                        Ok(())
                    });
                    return;
                }
            }

            unsafe {
                let () = msg_send![super(this, class!(NSWindow)), toggleFullScreen: sender];
            }
        }

        /// Override constrainFrameRect:toScreen: to accept any requested frame
        /// without modification. This prevents tiling window managers (AeroSpace,
        /// yabai, etc.) from entering a feedback loop where the WM requests an
        /// exact size, AppKit adjusts it (resize increments or screen clamping),
        /// the WM detects the mismatch and re-requests, causing flicker.
        /// <https://github.com/tw93/Kaku/issues/131>
        /// <https://github.com/tw93/Kaku/issues/183>
        extern "C" fn constrain_frame_rect(
            _this: &mut Object,
            _sel: Sel,
            frame_rect: NSRect,
            _screen: id,
        ) -> NSRect {
            frame_rect
        }

        unsafe {
            cls.add_method(
                sel!(canBecomeKeyWindow),
                yes as extern "C" fn(&mut Object, Sel) -> BOOL,
            );
            cls.add_method(
                sel!(canBecomeMainWindow),
                yes as extern "C" fn(&mut Object, Sel) -> BOOL,
            );
            cls.add_method(
                sel!(toggleFullScreen:),
                redirect_toggle_fullscreen as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(constrainFrameRect:toScreen:),
                constrain_frame_rect as extern "C" fn(&mut Object, Sel, NSRect, id) -> NSRect,
            );
        }

        cls.register()
    })
}

impl WindowView {
    fn cancel_pending_perform_requests(view: *mut Object) {
        unsafe {
            let _: () = msg_send![
                class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: view
                selector: sel!(kakuPersistWindowStateAfterMove:)
                object: nil
            ];
            let _: () = msg_send![
                class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: view
                selector: sel!(windowDidResize:)
                object: nil
            ];
        }
    }

    extern "C" fn dealloc(this: &mut Object, _sel: Sel) {
        Self::cancel_pending_perform_requests(this as *mut Object);
        Self::detach_backing_layer(this);
        Self::drop_inner(this);
        unsafe {
            let superclass = superclass(this);
            let () = msg_send![super(this, superclass), dealloc];
        }
    }

    fn detach_backing_layer(view: &mut Object) {
        unsafe {
            // On newer macOS builds we can crash in QuartzCore while the app is
            // quitting and CoreAnimation is tearing down the layer tree. Clear
            // the delegate and sublayers while the view is still valid so that
            // CA doesn't touch stale pointers during the final transaction
            // flush.
            let layer: id = msg_send![view, layer];
            if layer.is_null() {
                return;
            }

            let _: () = msg_send![layer, removeAllAnimations];
            let _: () = msg_send![layer, setDelegate: nil];
            let _: () = msg_send![layer, setSublayers: nil];
        }
    }

    fn drop_inner(this: &mut Object) {
        unsafe {
            let myself: *mut c_void = *this.get_ivar(VIEW_CLS_NAME);
            this.set_ivar(VIEW_CLS_NAME, std::ptr::null_mut() as *mut c_void);

            if !myself.is_null() {
                let myself = Box::from_raw(myself as *mut Self);
                drop(myself);
            }
        }
    }

    fn events(&self) -> WindowEventSender {
        self.inner.borrow().events.clone()
    }

    fn dispatch_event(&self, event: WindowEvent) {
        self.events().dispatch(event);
    }

    // Called by the inputContext manager when the IME processes events.
    // We need to translate the selector back into appropriate key
    // sequences
    extern "C" fn do_command_by_selector(this: &mut Object, _sel: Sel, a_selector: Sel) {
        let selector = format!("{:?}", a_selector);
        log::trace!("do_command_by_selector {:?}", selector);

        if let Some(myself) = Self::get_this(this) {
            let mut inner = myself.inner.borrow_mut();

            if selector == "insertNewline:" {
                // Handle newline from IME/dictation by dispatching Enter key event.
                // Use ImeDisposition::Acted to prevent duplicate dispatch in key_down_event.
                let event = KeyEvent {
                    key: KeyCode::Char('\r'),
                    modifiers: Modifiers::NONE,
                    leds: KeyboardLedStatus::empty(),
                    repeat_count: 1,
                    key_is_down: true,
                    raw: None,
                };
                inner.ime_last_event = Some(event.clone());
                inner.ime_state = ImeDisposition::Acted;
                let events = inner.events.clone();
                drop(inner);
                events.dispatch(WindowEvent::KeyEvent(event));
                return;
            }

            inner.ime_state = ImeDisposition::Continue;
            inner.ime_last_event.take();
        }
    }

    extern "C" fn has_marked_text(this: &mut Object, _sel: Sel) -> BOOL {
        if let Some(myself) = Self::get_this(this) {
            let inner = myself.inner.borrow();
            if inner.ime_text.is_empty() {
                NO
            } else {
                YES
            }
        } else {
            NO
        }
    }

    extern "C" fn marked_range(this: &mut Object, _sel: Sel) -> NSRange {
        if let Some(myself) = Self::get_this(this) {
            let inner = myself.inner.borrow();
            log::trace!("marked_range {:?}", inner.ime_text);
            if inner.ime_text.is_empty() {
                NSRange::new(NSNotFound as _, 0)
            } else {
                NSRange::new(0, inner.ime_text.len() as u64)
            }
        } else {
            NSRange::new(NSNotFound as _, 0)
        }
    }

    extern "C" fn selected_range(_this: &mut Object, _sel: Sel) -> NSRange {
        // Return a valid cursor position instead of NSNotFound.
        // This enables macOS dictation/voice input which requires
        // a valid cursor position where dictated text can be inserted.
        NSRange::new(0, 0)
    }

    // Called by the IME when inserting composed text and/or emoji
    extern "C" fn insert_text_replacement_range(
        this: &mut Object,
        _sel: Sel,
        astring: id,
        replacement_range: NSRange,
    ) {
        let s = unsafe { nsstring_to_str(astring) };
        log::trace!(
            "insert_text_replacement_range {} {:?}",
            s,
            replacement_range
        );
        // Filter out dead key placeholder text that may be sent by some keyboard layouts
        if is_dead_key_placeholder_text(s) {
            return;
        }
        if let Some(myself) = Self::get_this(this) {
            let mut inner = myself.inner.borrow_mut();

            let key_is_down = inner.key_is_down.take().unwrap_or(true);

            let key = KeyCode::composed(s);

            let event = KeyEvent {
                key,
                modifiers: Modifiers::NONE,
                leds: KeyboardLedStatus::empty(),
                repeat_count: 1,
                key_is_down,
                raw: None,
            };

            inner.ime_text.clear();
            inner.ime_last_event.replace(event.clone());
            inner.ime_state = ImeDisposition::Acted;
            let events = inner.events.clone();
            drop(inner);
            events.dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
            events.dispatch(WindowEvent::KeyEvent(event));
        }
    }

    extern "C" fn set_marked_text_selected_range_replacement_range(
        this: &mut Object,
        _sel: Sel,
        astring: id,
        selected_range: NSRange,
        replacement_range: NSRange,
    ) {
        let s = unsafe { nsstring_to_str(astring) };
        log::trace!(
            "set_marked_text_selected_range_replacement_range {} {:?} {:?}",
            s,
            selected_range,
            replacement_range
        );
        // Filter out dead key placeholder text; use empty string so dead key composing works
        let s = if is_dead_key_placeholder_text(s) {
            ""
        } else {
            s
        };
        if let Some(myself) = Self::get_this(this) {
            let mut inner = myself.inner.borrow_mut();
            inner.ime_text = s.to_string();

            /*
            let key_is_down = inner.key_is_down.take().unwrap_or(true);

            let key = KeyCode::composed(s);

            let event = KeyEvent {
                key,
                modifiers: Modifiers::NONE,
                repeat_count: 1,
                key_is_down,
            }
            .normalize_shift();

            inner.ime_last_event.replace(event.clone());
            inner.events.dispatch(WindowEvent::KeyEvent(event));
            */
            inner.ime_last_event.take();
            inner.ime_state = ImeDisposition::Acted;

            // Dispatch preedit status so composition text is visible immediately,
            // even without a preceding keyDown event (e.g. voice input)
            let status = if inner.ime_text.is_empty() {
                DeadKeyStatus::None
            } else {
                DeadKeyStatus::Composing(inner.ime_text.clone())
            };
            let events = inner.events.clone();
            drop(inner);
            events.dispatch(WindowEvent::AdviseDeadKeyStatus(status));
        }
    }

    extern "C" fn unmark_text(this: &mut Object, _sel: Sel) {
        log::trace!("unmarkText");
        if let Some(myself) = Self::get_this(this) {
            let mut inner = myself.inner.borrow_mut();
            // FIXME: docs say to insert the text here,
            // but iterm doesn't... and we've never seen
            // this get called so far?
            inner.ime_text.clear();
            inner.ime_last_event.take();
            inner.ime_state = ImeDisposition::Acted;
        }
    }

    extern "C" fn valid_attributes_for_marked_text(_this: &mut Object, _sel: Sel) -> id {
        // FIXME: returns NSArray<NSAttributedStringKey> *
        // log::trace!("valid_attributes_for_marked_text");
        // nil
        unsafe { NSArray::arrayWithObjects(nil, &[]) }
    }

    extern "C" fn attributed_substring_for_proposed_range(
        _this: &mut Object,
        _sel: Sel,
        _proposed_range: NSRange,
        _actual_range: NSRangePointer,
    ) -> id {
        log::trace!(
            "attributedSubstringForProposedRange {:?} {:?}",
            _proposed_range,
            _actual_range
        );
        nil
    }

    extern "C" fn character_index_for_point(
        _this: &mut Object,
        _sel: Sel,
        _point: NSPoint,
    ) -> NSUInteger {
        NSNotFound as _
    }

    extern "C" fn first_rect_for_character_range(
        this: &mut Object,
        _sel: Sel,
        range: NSRange,
        actual: NSRangePointer,
    ) -> NSRect {
        // Returns a rect in screen coordinates; this is used to place
        // the input method editor
        log::trace!(
            "firstRectForCharacterRange: range:{:?} actual:{:?}",
            range,
            actual
        );
        let window: id = unsafe { msg_send![this, window] };
        let frame = unsafe { NSWindow::frame(window) };
        let content: NSRect = unsafe { msg_send![window, contentRectForFrameRect: frame] };
        let backing_frame: NSRect = unsafe { msg_send![this, convertRectToBacking: frame] };
        let scale = frame.size.width / backing_frame.size.width;

        if !actual.0.is_null() {
            unsafe {
                *actual.0 = range;
            }
        }

        if let Some(this) = Self::get_this(this) {
            let cursor_pos = this
                .inner
                .borrow()
                .text_cursor_position
                .to_f64()
                .scale(scale, scale);

            NSRect::new(
                NSPoint::new(
                    content.origin.x + cursor_pos.min_x(),
                    content.origin.y + content.size.height - cursor_pos.max_y(),
                ),
                NSSize::new(cursor_pos.size.width, cursor_pos.size.height),
            )
        } else {
            frame
        }
    }

    extern "C" fn accepts_first_mouse(_this: &mut Object, _sel: Sel, _nsevent: id) -> BOOL {
        YES
    }

    extern "C" fn accepts_first_responder(_this: &mut Object, _sel: Sel) -> BOOL {
        YES
    }

    extern "C" fn view_did_change_effective_appearance(this: &mut Object, _sel: Sel) {
        if let Some(this) = Self::get_this(this) {
            if let Some(conn) = Connection::get() {
                let appearance = conn.get_appearance();
                this.dispatch_event(WindowEvent::AppearanceChanged(appearance));
            }
        }
    }

    extern "C" fn update_tracking_areas(this: &mut Object, _sel: Sel) {
        let frame = unsafe { NSView::frame(this as *mut _) };

        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            let mut inner = match this.inner.try_borrow_mut() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if let Some(ref view) = inner.view_id {
                let view = view.load();
                if view.is_null() {
                    return;
                }

                let tag = inner.tracking_rect_tag;
                if tag != 0 {
                    unsafe {
                        let () = msg_send![*view, removeTrackingRect: tag];
                    }
                }

                let rect = NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(frame.size.width, frame.size.height),
                );
                inner.tracking_rect_tag = unsafe {
                    msg_send![*view, addTrackingRect: rect owner: *view userData: nil assumeInside: NO]
                };
            }
        }
    }

    extern "C" fn window_should_close(this: &mut Object, _sel: Sel, _id: id) -> BOOL {
        unsafe {
            let () = msg_send![this, setNeedsDisplay: YES];
        }

        if let Some(this) = Self::get_this(this) {
            this.dispatch_event(WindowEvent::CloseRequested);
            NO
        } else {
            YES
        }
    }

    /// Ensure that the menubar is shown when we transition from a fullscreen window
    /// to either a non-fullscreen window or no windows.
    /// Without this, we can end up in a state where the menu bar is invisible when
    /// it should otherwise be visible, and it is especially confusing when there
    /// are no windows.
    fn update_application_presentation(&self, is_key: bool) {
        let is_simple_full_screen;
        let native_full_screen;

        {
            let inner = self.inner.borrow();
            is_simple_full_screen = inner.fullscreen.is_some();
            native_full_screen = inner.window.as_ref().map_or(false, |window| {
                let window = window.load();
                let style_mask = unsafe { NSWindow::styleMask(*window) };
                style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask)
            });
        }

        if !native_full_screen {
            let current_app = unsafe { NSApplication::sharedApplication(nil) };
            let target_options = match (is_key, is_simple_full_screen) {
                (true, true) => {
                    NSApplicationPresentationOptions::NSApplicationPresentationAutoHideMenuBar
                        | NSApplicationPresentationOptions::NSApplicationPresentationAutoHideDock
                }
                (true, false) | (false, _) => {
                    NSApplicationPresentationOptions::NSApplicationPresentationDefault
                }
            };
            unsafe {
                let current_options: NSApplicationPresentationOptions =
                    msg_send![current_app, presentationOptions];
                if current_options != target_options {
                    current_app.setPresentationOptions_(target_options);
                }
            }
        }
    }

    extern "C" fn did_become_key(this: &mut Object, _sel: Sel, _id: id) {
        if let Some(this) = Self::get_this(this) {
            this.dispatch_event(WindowEvent::FocusChanged(true));
            this.update_application_presentation(true);
        }
    }

    extern "C" fn did_resign_key(this: &mut Object, _sel: Sel, _id: id) {
        if let Some(this) = Self::get_this(this) {
            this.dispatch_event(WindowEvent::FocusChanged(false));
            this.update_application_presentation(true);
        }
    }

    extern "C" fn did_change_occlusion_state(this: &mut Object, _sel: Sel, _id: id) {
        if let Some(this) = Self::get_this(this) {
            let visible = {
                let inner = this.inner.borrow();
                inner.window.as_ref().is_some_and(|window| {
                    let window = window.load();
                    unsafe {
                        window
                            .occlusionState()
                            .contains(appkit::NSWindowOcclusionState::NSWindowOcclusionStateVisible)
                    }
                })
            };

            this.dispatch_event(WindowEvent::VisibilityChanged(visible));
        }
    }

    // Switch the coordinate system to have 0,0 in the top left
    extern "C" fn is_flipped(_this: &Object, _sel: Sel) -> BOOL {
        YES
    }

    // Tell macOS the view is opaque when window_background_opacity is 1.0
    // so the compositor can skip blending it with content behind it.
    // Reads from a cached AtomicBool (updated on config change) to avoid
    // acquiring the config Mutex on every compositor call (60-120Hz).
    extern "C" fn is_opaque(_this: &Object, _sel: Sel) -> BOOL {
        if VIEW_IS_OPAQUE.load(Ordering::Relaxed) {
            YES
        } else {
            NO
        }
    }

    extern "C" fn mouse_down_can_move_window(_this: &Object, _sel: Sel) -> BOOL {
        NO
    }

    // Don't use Cocoa native window tabbing
    extern "C" fn allow_automatic_tabbing(_this: &Object, _sel: Sel) -> BOOL {
        NO
    }

    // Accessibility support: report as text area so voice input tools can detect us
    extern "C" fn accessibility_role(_this: &Object, _sel: Sel) -> id {
        // NSAccessibilityTextAreaRole
        unsafe { msg_send![class!(NSString), stringWithUTF8String: AX_ROLE_TEXT_AREA.as_ptr()] }
    }

    extern "C" fn is_accessibility_element(_this: &Object, _sel: Sel) -> BOOL {
        YES
    }

    extern "C" fn accessibility_role_description(_this: &Object, _sel: Sel) -> id {
        // Intentionally not localized. Voice input tools may key off this phrase.
        unsafe {
            msg_send![
                class!(NSString),
                stringWithUTF8String: AX_ROLE_DESCRIPTION_TERMINAL_TEXT_AREA.as_ptr()
            ]
        }
    }

    extern "C" fn kaku_perform_key_assignment(
        this: &mut Object,
        _sel: Sel,
        menu_item: *mut Object,
    ) {
        let menu_item = MenuItem::with_menu_item(menu_item);
        // Safe because kakuPerformKeyAssignment: is only used with KeyAssignment
        let action = menu_item.get_represented_item();
        log::debug!("kaku_perform_key_assignment {action:?}",);
        match action {
            Some(RepresentedItem::KeyAssignment(action)) => {
                if let Some(this) = Self::get_this(this) {
                    // Keep the RefCell borrow out of the synchronous dispatch path so
                    // menu actions that call back into AppKit can safely re-enter.
                    if let Ok(inner) = this.inner.try_borrow() {
                        let events = inner.events.clone();
                        drop(inner);
                        events.dispatch(WindowEvent::PerformKeyAssignment(action));
                    }
                }
            }
            None => {}
        }
    }

    extern "C" fn window_will_close(this: &mut Object, _sel: Sel, _id: id) {
        // Mark the window as closing BEFORE any cleanup so that fullscreen
        // transition handlers (will_exit_fullscreen, did_exit_fullscreen) that
        // AppKit may fire during [NSWindow _close] will bail out instead of
        // dispatching events into an already-destroyed TermWindow.
        if let Some(view) = Self::get_this(this) {
            view.is_closing.set(true);
        }
        Self::cancel_pending_perform_requests(this as *mut Object);
        Self::detach_backing_layer(this);
        if let Some(this) = Self::get_this(this) {
            let conn = Connection::get();
            if !APP_TERMINATING.load(Ordering::Relaxed) {
                // Extract the raw window pointer while holding the borrow, then drop the
                // borrow *before* calling into AppKit. persist_window_size_and_position
                // and remember_last_closed_window_position both call Cocoa APIs that can
                // spin the event loop and re-enter kaku_perform_key_assignment, which
                // would fail with a double-borrow panic if we held the borrow here.
                let raw_window = this.inner.borrow().window.as_ref().map(|w| w.load());
                if let Some(window) = raw_window {
                    if !window.is_null() {
                        remember_last_closed_window_position(*window);
                        let _ = persist_window_size_and_position(*window);
                    }
                }
            }
            // Advise the window of its impending death.
            // Use try_borrow to avoid a double-borrow panic when window_will_close
            // fires synchronously inside a key event handler that already holds the borrow.
            if let Ok(inner) = this.inner.try_borrow() {
                let events = inner.events.clone();
                drop(inner);
                events.dispatch(WindowEvent::Destroyed);
            } else {
                log::warn!("window_will_close: RefCell already borrowed, WindowEvent::Destroyed not dispatched for window {}", this.window_id.get());
            }
            this.update_application_presentation(false);
            // window_id is stored outside the RefCell so we can always remove the
            // window from the connection map, even when a borrow is held upstream.
            let window_id = this.window_id.get();
            if let Some(conn) = conn {
                conn.windows.borrow_mut().remove(&window_id);
            }
        }
    }

    extern "C" fn did_move(this: &mut Object, _sel: Sel, _notification: id) {
        unsafe {
            let _: () = msg_send![
                class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: this as *mut Object
                selector: sel!(kakuPersistWindowStateAfterMove:)
                object: nil
            ];
            let _: () = msg_send![
                this,
                performSelector: sel!(kakuPersistWindowStateAfterMove:)
                withObject: nil
                afterDelay: MOVE_PERSIST_DELAY_SECS
            ];
        }
    }

    extern "C" fn persist_window_state_after_move(this: &mut Object, _sel: Sel, _obj: id) {
        if APP_TERMINATING.load(Ordering::Relaxed) {
            return;
        }

        if let Some(this) = Self::get_this(this) {
            if let Some(window) = this.inner.borrow().window.as_ref() {
                let window = window.load();
                if !window.is_null() {
                    let _ = persist_window_size_and_position(*window);
                }
            }
        }
    }

    fn mouse_common(this: &mut Object, nsevent: id, kind: MouseEventKind) {
        let view = this as id;
        let coords;
        let mouse_buttons;
        let modifiers;
        let screen_coords;
        unsafe {
            let point = NSView::convertPoint_fromView_(view, nsevent.locationInWindow(), nil);
            let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(point.x, point.y));
            let backing_rect = NSView::convertRectToBacking(view, rect);
            // backing_rect computes abs() values, so we need to restore the sign
            // from the original point
            coords = NSPoint::new(
                f64::copysign(backing_rect.size.width, point.x),
                f64::copysign(backing_rect.size.height, point.y),
            );
            mouse_buttons = decode_mouse_buttons(NSEvent::pressedMouseButtons(nsevent));
            modifiers = key_modifiers(nsevent.modifierFlags());
            screen_coords = NSEvent::mouseLocation(nsevent);
        }
        let event = MouseEvent {
            kind,
            coords: Point::new(coords.x as isize, coords.y as isize),
            screen_coords: cartesian_to_screen_point(screen_coords),
            mouse_buttons,
            modifiers,
        };

        if let Some(myself) = Self::get_this(this) {
            myself.dispatch_event(WindowEvent::MouseEvent(event));
        }
    }

    extern "C" fn mouse_up(this: &mut Object, _sel: Sel, nsevent: id) {
        Self::mouse_common(this, nsevent, MouseEventKind::Release(MousePress::Left));
    }

    extern "C" fn mouse_down(this: &mut Object, _sel: Sel, nsevent: id) {
        // Check if we're in fullscreen mode - if so, disable dragging
        let in_fullscreen = if let Some(view) = Self::get_this(this) {
            let simple_fullscreen = view.inner.borrow().fullscreen.is_some();
            let native_fullscreen = unsafe {
                let window: id = msg_send![this as id, window];
                window != nil
                    && NSWindow::styleMask(window)
                        .contains(NSWindowStyleMask::NSFullScreenWindowMask)
            };
            simple_fullscreen || native_fullscreen
        } else {
            false
        };

        // Clear stale flag to prevent false drag triggers from last abnormal exit
        PENDING_DRAG_MOVE.with(|flag| flag.set(false));
        Self::mouse_common(this, nsevent, MouseEventKind::Press(MousePress::Left));

        // Execute drag synchronously: app layer may call request_drag_move() in mouse_common to set flag
        // But skip if in fullscreen mode or if the window is zoomed/maximized. On macOS,
        // performWindowDragWithEvent: can be interpreted as a snap/maximize gesture when the
        // frame already fills the visible screen even if isZoomed is stale. Use double-click
        // to un-zoom instead of letting a single title-area click expand the window.
        let pending_drag = PENDING_DRAG_MOVE.with(|flag| flag.replace(false));
        if pending_drag && !in_fullscreen {
            unsafe {
                let window: id = msg_send![this as id, window];
                if window != nil {
                    let is_zoomed: bool = msg_send![window, isZoomed];
                    let fills_visible_frame = window_fills_visible_frame(window);
                    if should_perform_native_window_drag(
                        in_fullscreen,
                        is_zoomed,
                        fills_visible_frame,
                    ) {
                        let () = msg_send![window, performWindowDragWithEvent: nsevent];
                    }
                }
            }
        }
    }
    extern "C" fn right_mouse_up(this: &mut Object, _sel: Sel, nsevent: id) {
        Self::mouse_common(this, nsevent, MouseEventKind::Release(MousePress::Right));
    }

    extern "C" fn other_mouse_up(this: &mut Object, _sel: Sel, nsevent: id) {
        // Safety: We know this is an button event
        unsafe {
            let button_number = NSEvent::buttonNumber(nsevent);
            // Button 2 is the middle mouse button (scroll wheel)
            // but is the dedicated middle mouse button on 4 button mouses
            if button_number == 2 {
                Self::mouse_common(this, nsevent, MouseEventKind::Release(MousePress::Middle));
            }
        }
    }

    extern "C" fn scroll_wheel(this: &mut Object, _sel: Sel, nsevent: id) {
        let precise = unsafe { nsevent.hasPreciseScrollingDeltas() } == YES;
        let raw_vert_delta = unsafe { nsevent.scrollingDeltaY() };
        let raw_horz_delta = unsafe { nsevent.scrollingDeltaX() };
        let scale = if precise {
            // Devices with precise deltas report number of pixels scrolled.
            // At this layer we don't know how many pixels comprise a cell
            // in the terminal widget, and our abstraction doesn't allow being
            // told what that amount should be, so we come up with a hard
            // coded factor based on the likely default font size and dpi
            // to make the scroll speed feel a bit better.
            15.0
        } else {
            // Whereas imprecise deltas report the number of lines scrolled,
            // so we want to report those lines here wholesale.
            1.0
        };
        let mut vert_delta = raw_vert_delta / scale;
        let mut horz_delta = raw_horz_delta / scale;
        let mut suppress_vert = false;
        let mut suppress_horz = false;
        if precise {
            // Trackpads often emit tiny cross-axis jitter.
            // When one axis is clearly dominant, suppress the other axis so
            // terminal apps don't receive accidental left/right wheel events.
            let raw_v = raw_vert_delta.abs();
            let raw_h = raw_horz_delta.abs();
            if raw_v > raw_h * 1.25 {
                suppress_horz = true;
            } else if raw_h > raw_v * 1.25 {
                suppress_vert = true;
            }
        }
        if let Some(myself) = Self::get_this(this) {
            let mut inner = match myself.inner.try_borrow_mut() {
                Ok(inner) => inner,
                Err(_) => return,
            };

            if suppress_horz {
                inner.hscroll_remainder = 0.;
                horz_delta = 0.0;
            }
            if suppress_vert {
                inner.vscroll_remainder = 0.;
                vert_delta = 0.0;
            }

            let elapsed = inner.last_wheel.elapsed();

            // If it's been a while since the last wheel movement,
            // we want to clear out any accumulated fractional amount
            // and round this event up to 1 line so that we get an
            // immediate scroll on the first move.
            let stale = std::time::Duration::from_millis(250);
            if elapsed >= stale {
                if vert_delta != 0.0 && vert_delta.abs() < 1.0 {
                    vert_delta = round_away_from_zerof(vert_delta);
                }
                if horz_delta != 0.0 && horz_delta.abs() < 1.0 {
                    horz_delta = round_away_from_zerof(horz_delta);
                }
                inner.vscroll_remainder = 0.;
                inner.hscroll_remainder = 0.;
            }

            inner.last_wheel = Instant::now();

            // Reset remainder only when an explicit non-zero delta changes direction.
            // Zero deltas are common on smooth trackpads and should not discard
            // accumulated fractional scroll.
            if vert_delta != 0.0 && vert_delta.signum() != inner.vscroll_remainder.signum() {
                inner.vscroll_remainder = 0.;
            }
            if horz_delta != 0.0 && horz_delta.signum() != inner.hscroll_remainder.signum() {
                inner.hscroll_remainder = 0.;
            }

            vert_delta += inner.vscroll_remainder;
            horz_delta += inner.hscroll_remainder;

            inner.vscroll_remainder = vert_delta.fract();
            inner.hscroll_remainder = horz_delta.fract();

            vert_delta = vert_delta.trunc();
            horz_delta = horz_delta.trunc();
        }

        if vert_delta.abs() < 1.0 && horz_delta.abs() < 1.0 {
            return;
        }

        let kind = if vert_delta.abs() > horz_delta.abs() {
            MouseEventKind::VertWheel(round_away_from_zero(vert_delta))
        } else {
            MouseEventKind::HorzWheel(round_away_from_zero(horz_delta))
        };
        Self::mouse_common(this, nsevent, kind);
    }

    extern "C" fn right_mouse_down(this: &mut Object, _sel: Sel, nsevent: id) {
        Self::mouse_common(this, nsevent, MouseEventKind::Press(MousePress::Right));
    }

    extern "C" fn other_mouse_down(this: &mut Object, _sel: Sel, nsevent: id) {
        // Safety: See `other_mouse_up`
        unsafe {
            let button_number = NSEvent::buttonNumber(nsevent);
            // See `other_mouse_up`
            if button_number == 2 {
                Self::mouse_common(this, nsevent, MouseEventKind::Press(MousePress::Middle));
            }
        }
    }

    extern "C" fn mouse_moved_or_dragged(this: &mut Object, _sel: Sel, nsevent: id) {
        Self::mouse_common(this, nsevent, MouseEventKind::Move);
    }

    extern "C" fn mouse_exited(this: &mut Object, _sel: Sel, _nsevent: id) {
        if let Some(myself) = Self::get_this(this) {
            myself.dispatch_event(WindowEvent::MouseLeave);
        }
    }

    fn key_common(this: &mut Object, nsevent: id, key_is_down: bool) {
        let is_a_repeat = unsafe { nsevent.isARepeat() == YES };
        let chars = unsafe { nsstring_to_str(nsevent.characters()) };
        let unmod = unsafe { nsstring_to_str(nsevent.charactersIgnoringModifiers()) };
        // Some macOS keyboard layouts surface dead-key presses as a single
        // Unicode noncharacter placeholder instead of an empty string. Treat it
        // the same as empty so the existing dead-key translation path handles it.
        let chars = if is_dead_key_placeholder_text(chars) {
            ""
        } else {
            chars
        };
        let unmod = if is_dead_key_placeholder_text(unmod) {
            ""
        } else {
            unmod
        };
        let modifier_flags = unsafe { nsevent.modifierFlags() };
        let modifiers = key_modifiers(modifier_flags);
        let leds = if modifier_flags.bits() & (1 << 16) != 0 {
            KeyboardLedStatus::CAPS_LOCK
        } else {
            KeyboardLedStatus::empty()
        };
        let virtual_key = unsafe { nsevent.keyCode() };

        log::trace!(
            "key_common: chars=`{}` unmod=`{}` modifiers=`{:?}` virtual_key={:?} key_is_down:{}",
            chars.escape_debug(),
            unmod.escape_debug(),
            modifiers,
            virtual_key,
            key_is_down
        );

        // `Delete` on macos is really Backspace and emits BS.
        // `Fn-Delete` emits DEL.
        // Alt-Delete is mapped by the IME to be equivalent to Fn-Delete.
        // We want to emit Alt-BS in that situation.
        let (prefer_vkey, unmod) =
            if virtual_key == kVK_Delete && modifiers.contains(Modifiers::ALT) {
                (true, "\x08")
            } else if virtual_key == kVK_Tab {
                (true, "\t")
            } else if virtual_key == kVK_Delete {
                (true, "\x08")
            } else if virtual_key == kVK_ANSI_KeypadEnter {
                // https://github.com/wezterm/wezterm/issues/739
                // Keypad enter sends ctrl-c for some reason; explicitly
                // treat that as enter here.
                (true, "\r")
            } else if is_non_menu_virtual_key(virtual_key) {
                // Navigation/function keys can surface here as composed strings
                // with modifier-dependent escape fragments. Prefer vkey mapping
                // so they normalize into stable key codes.
                (true, unmod)
            } else if modifiers == (Modifiers::SUPER | Modifiers::SHIFT)
                && is_symbol_virtual_key(virtual_key)
            {
                // For Cmd+Shift+symbol combinations, prefer virtual-key decoding so
                // bindings can match stable base keys like "," across layouts/IME.
                // Use exact match to avoid affecting Cmd+Ctrl+Shift+symbol etc.
                (true, unmod)
            } else if is_command_alnum_shortcut(unmod, modifiers, virtual_key) {
                // Latin layouts: trust the labeled letter (AZERTY "a" sits on
                // kVK_ANSI_Q, so the physical key would mis-fire Cmd+Q). Empty
                // or non-ASCII unmod (true non-Latin IME) keeps the vkey path.
                (!is_ascii_letter_text(unmod), unmod)
            } else {
                (false, unmod)
            };

        // Shift-Tab on macOS produces \x19 for some reason.
        // Rewrite it to something we understand.
        // <https://github.com/wezterm/wezterm/issues/1902>
        let chars = if virtual_key == kVK_Tab && modifiers.contains(Modifiers::SHIFT) {
            "\t"
        } else {
            chars
        };

        let phys_code = vkey_to_phys(virtual_key);
        let raw_key_handled = Handled::new();
        let raw_key_event = RawKeyEvent {
            key: if unmod.is_empty() {
                match phys_code {
                    Some(phys) => KeyCode::Physical(phys),
                    None => KeyCode::RawCode(virtual_key as _),
                }
            } else {
                KeyCode::composed(unmod)
            },
            phys_code,
            raw_code: virtual_key as _,
            leds,
            modifiers,
            repeat_count: 1,
            key_is_down,
            handled: raw_key_handled.clone(),
        };
        if let Some(myself) = Self::get_this(this) {
            myself.dispatch_event(WindowEvent::RawKeyEvent(raw_key_event.clone()));
        }

        if raw_key_handled.is_handled() {
            log::trace!("raw key was handled; not processing further");
            return;
        }

        let chars = if let Some(myself) = Self::get_this(this) {
            let mut inner = myself.inner.borrow_mut();

            if chars.is_empty() || inner.dead_pending.is_some() {
                // Dead key!
                if !key_is_down {
                    return;
                }

                match inner.translate_key_event(virtual_key, modifier_flags, chars.is_empty()) {
                    Ok(TranslateStatus::Composing(composing)) => {
                        // Next key press in dead key sequence is pending.
                        let events = inner.events.clone();
                        drop(inner);
                        events.dispatch(WindowEvent::AdviseDeadKeyStatus(
                            DeadKeyStatus::Composing(composing),
                        ));
                        return;
                    }
                    Ok(TranslateStatus::Composed(translated)) => {
                        let events = inner.events.clone();
                        drop(inner);
                        events.dispatch(WindowEvent::AdviseDeadKeyStatus(DeadKeyStatus::None));
                        let event = KeyEvent {
                            key: KeyCode::composed(&translated),
                            modifiers: Modifiers::NONE,
                            leds: KeyboardLedStatus::empty(),
                            repeat_count: 1,
                            key_is_down,
                            raw: None,
                        };
                        events.dispatch(WindowEvent::KeyEvent(event));
                        return;
                    }
                    Ok(TranslateStatus::NotDead) => {
                        // Turned out that while it would have been a dead
                        // key combo, our send_composed_key_when_XXX settings
                        // said otherwise. Let's continue as if it was not
                        // a dead key.
                        unmod
                    }
                    Err(e) => {
                        log::error!("Failed to translate dead key: {}", e);
                        return;
                    }
                }
            } else {
                chars
            }
        } else {
            return;
        };

        let config_handle = config::configuration();
        let use_ime = config_handle.use_ime;
        let send_composed_key_when_left_alt_is_pressed =
            config_handle.send_composed_key_when_left_alt_is_pressed;
        let send_composed_key_when_right_alt_is_pressed =
            config_handle.send_composed_key_when_right_alt_is_pressed;

        // If unmod is empty it most likely means that the user has selected
        // an alternate keymap that has a chorded representation of eg: an ASCII
        // character.  One example of this is selecting a Norwegian keymap on
        // a US keyboard.  The `~` symbol is produced by pressing CTRL-].
        // That shows up here as unmod=`` with modifiers=CTRL.  In this situation
        // we want to cancel the modifiers out so that we just focus on
        // `chars` instead.
        let modifiers = if should_clear_modifiers_for_empty_unmod(unmod, modifiers, virtual_key) {
            Modifiers::NONE
        } else {
            modifiers
        };

        let alt_mods = Modifiers::LEFT_ALT | Modifiers::RIGHT_ALT | Modifiers::ALT;
        let only_left_alt = (modifiers & alt_mods) == (Modifiers::LEFT_ALT | Modifiers::ALT);
        let only_right_alt = (modifiers & alt_mods) == (Modifiers::RIGHT_ALT | Modifiers::ALT);

        let has_active_ime_composition = Self::get_this(this)
            .map(|s| !s.inner.borrow().ime_text.is_empty())
            .unwrap_or(false);

        // Also respect `send_composed_key_when_(left|right)_alt_is_pressed` configs
        // when `use_ime` is true.
        let forward_to_ime = if has_active_ime_composition {
            true
        } else if only_left_alt && !send_composed_key_when_left_alt_is_pressed {
            false
        } else if only_right_alt && !send_composed_key_when_right_alt_is_pressed {
            false
        } else {
            modifiers.is_empty()
                || modifiers.intersects(config_handle.macos_forward_to_ime_modifier_mask)
        };

        if key_is_down && use_ime && forward_to_ime {
            if let Some(myself) = Self::get_this(this) {
                let mut inner = myself.inner.borrow_mut();
                inner.key_is_down.replace(key_is_down);
                inner.ime_state = ImeDisposition::None;
                inner.ime_text.clear();
            }

            unsafe {
                let array: id = msg_send![class!(NSArray), arrayWithObject: nsevent];
                let _: () = msg_send![this, interpretKeyEvents: array];

                if let Some(myself) = Self::get_this(this) {
                    let mut inner = myself.inner.borrow_mut();
                    log::trace!(
                        "IME state: {:?}, last_event: {:?}",
                        inner.ime_state,
                        inner.ime_last_event
                    );
                    match inner.ime_state {
                        ImeDisposition::Continue => {
                            // IME handled the event by generating NOOP;
                            // let's continue with our normal handling
                            // code below.
                            inner.ime_last_event.take();
                        }
                        ImeDisposition::Acted => {
                            // The key caused the IME to call one of our
                            // callbacks, which may have generated an event and
                            // stashed it into ime_last_event.
                            // If it didn't generate an event, then a composition
                            // is pending.
                            let status = if inner.ime_last_event.is_none() {
                                DeadKeyStatus::Composing(inner.ime_text.clone())
                            } else {
                                DeadKeyStatus::None
                            };
                            let events = inner.events.clone();
                            drop(inner);
                            events.dispatch(WindowEvent::AdviseDeadKeyStatus(status));
                            return;
                        }
                        ImeDisposition::None => {
                            // The IME clocked something in its state,
                            // but didn't call one of our callbacks.
                            // In theory, we should stop here, but the IME
                            // mysteriously swallows key repeats for certain
                            // keys (i.e. b, f, j, m, p, q, v, x) but not others.
                            // To compensate for that, if the current event
                            // is a repeat, and the IME previously generated
                            // `Acted`, we will assume that we're safe to replay
                            // that last action.
                            if is_a_repeat {
                                if let Some(event) =
                                    inner.ime_last_event.as_ref().map(|e| e.clone())
                                {
                                    let events = inner.events.clone();
                                    drop(inner);
                                    events.dispatch(WindowEvent::KeyEvent(event));
                                    return;
                                }
                            }
                            let status = if inner.ime_text.is_empty() {
                                DeadKeyStatus::None
                            } else {
                                DeadKeyStatus::Composing(inner.ime_text.clone())
                            };
                            let events = inner.events.clone();
                            drop(inner);
                            events.dispatch(WindowEvent::AdviseDeadKeyStatus(status));
                            return;
                        }
                    }
                }
            }
        }

        fn key_string_to_key_code(s: &str) -> Option<KeyCode> {
            let mut char_iter = s.chars();
            if let Some(first_char) = char_iter.next() {
                if char_iter.next().is_none() {
                    // A single unicode char
                    Some(function_key_to_keycode(first_char))
                } else {
                    Some(KeyCode::Composed(s.to_owned()))
                }
            } else {
                None
            }
        }

        // When both shift and alt are pressed, macos appears to swap `chars` with `unmod`,
        // which isn't particularly helpful. eg: ALT+SHIFT+` produces chars='`' and unmod='~'
        // In this case, we take the key from unmod.
        // We leave `raw` set to None as we want to preserve the value of modifiers.
        // <https://github.com/wezterm/wezterm/issues/1706>.
        // We can't do this for every ALT+SHIFT combo, as the weird behavior doesn't
        // apply to eg: ALT+SHIFT+789 for Norwegian layouts
        // <https://github.com/wezterm/wezterm/issues/760>
        let swap_unmod_and_chars = (modifiers.contains(Modifiers::SHIFT | Modifiers::ALT)
            && virtual_key == kVK_ANSI_Grave)
            ||
            // <https://github.com/wezterm/wezterm/issues/1907>
            (modifiers.contains(Modifiers::SHIFT | Modifiers::CTRL)
                && virtual_key == kVK_ANSI_Slash);

        if let Some(key) = key_string_to_key_code(chars).or_else(|| key_string_to_key_code(unmod)) {
            let (key, raw_key) = if prefer_vkey {
                match phys_code {
                    Some(phys) => (phys.to_key_code(), None),
                    None => {
                        log::error!(
                            "prefer_vkey=true, but phys_code is None. {:?}",
                            raw_key_event
                        );
                        return;
                    }
                }
            } else if (only_left_alt && !send_composed_key_when_left_alt_is_pressed)
                || (only_right_alt && !send_composed_key_when_right_alt_is_pressed)
            {
                // Usually we take the unmodified key when compose is disabled for this ALT side.
                // However, some layouts (eg: Turkish) produce ASCII symbols such as `~`
                // via Option chords. Preserve those produced symbols and clear ALT by
                // pairing them with a raw/unmodified key.
                let raw = key_string_to_key_code(unmod);
                match (&key, &raw) {
                    (KeyCode::Char(c), Some(_))
                        if c.is_ascii_punctuation() && !chars.is_empty() && chars != unmod =>
                    {
                        (key, raw)
                    }
                    _ => match raw {
                        Some(key) => (key, None),
                        None => return,
                    },
                }
            } else if chars.is_empty() || chars == unmod {
                (key, None)
            } else if swap_unmod_and_chars {
                match key_string_to_key_code(unmod) {
                    Some(key) => (key, None),
                    None => return,
                }
            } else {
                let raw = key_string_to_key_code(unmod);
                match (&key, &raw) {
                    // Avoid eg: \x01 when we can use CTRL-A.
                    // This also helps to keep the correct sequence for backspace/delete.
                    // But take care: on German layouts CTRL-Backslash has unmod="/"
                    // but chars="\x1c"; we only want to do this transformation when
                    // chars and unmod have that base ASCII relationship.
                    // <https://github.com/wezterm/wezterm/issues/1891>
                    (KeyCode::Char(c), Some(KeyCode::Char(raw)))
                        if is_ascii_control(*c) == Some(raw.to_ascii_lowercase()) =>
                    {
                        (KeyCode::Char(*raw), None)
                    }
                    _ => (key, raw),
                }
            };

            let modifiers = if raw_key.is_some() {
                Modifiers::NONE
            } else {
                modifiers
            };

            let event = KeyEvent {
                key,
                modifiers,
                leds,
                repeat_count: 1,
                key_is_down,
                raw: Some(raw_key_event),
            }
            .normalize_shift()
            .resurface_positional_modifier_key();

            log::trace!(
                "key_common {:?} (chars={:?} unmod={:?} modifiers={:?})",
                event,
                chars,
                unmod,
                modifiers
            );

            if let Some(myself) = Self::get_this(this) {
                let mut inner = myself.inner.borrow_mut();
                // Don't clear the last IME event when a key is up otherwise it
                // could mess up the succeeding key repeats.
                if key_is_down {
                    inner.ime_last_event.take();
                }
                let events = inner.events.clone();
                drop(inner);
                events.dispatch(WindowEvent::KeyEvent(event));
            }
        }
    }

    extern "C" fn perform_key_equivalent(this: &mut Object, _sel: Sel, nsevent: id) -> BOOL {
        let chars = unsafe { nsstring_to_str(nsevent.characters()) };
        let unmod = unsafe { nsstring_to_str(nsevent.charactersIgnoringModifiers()) };
        let modifier_flags = unsafe { nsevent.modifierFlags() };
        let modifiers = key_modifiers(modifier_flags);
        let virtual_key = unsafe { nsevent.keyCode() };

        log::trace!(
            "perform_key_equivalent: chars=`{}` unmod=`{}` modifiers=`{:?}` virtual_key={:?}",
            chars.escape_debug(),
            unmod.escape_debug(),
            modifiers,
            virtual_key,
        );

        if should_intercept_perform_key_equivalent(chars, unmod, modifiers, virtual_key) {
            // Synthesize a key down event for this, because macOS will
            // not do that, even though we tell it that we handled this event.
            // <https://github.com/wezterm/wezterm/issues/1867>
            // Command + non-menu virtual keys are routed here by macOS
            // and would otherwise be consumed before reaching keyDown:.
            Self::key_common(this, nsevent, true);

            // Prevent macOS from calling doCommandBySelector(cancel:)
            YES
        } else {
            // Allow macOS to process built-in shortcuts like CMD-`
            // to cycle though windows
            NO
        }
    }

    extern "C" fn flags_changed(this: &mut Object, _sel: Sel, nsevent: id) {
        let modifier_flags = unsafe { nsevent.modifierFlags() };
        let modifiers = key_modifiers(modifier_flags);
        let leds = if modifier_flags.bits() & (1 << 16) != 0 {
            KeyboardLedStatus::CAPS_LOCK
        } else {
            KeyboardLedStatus::empty()
        };

        if let Some(myself) = Self::get_this(this) {
            myself.dispatch_event(WindowEvent::AdviseModifiersLedStatus(modifiers, leds));
        }
    }

    extern "C" fn key_down(this: &mut Object, _sel: Sel, nsevent: id) {
        Self::key_common(this, nsevent, true);
    }

    extern "C" fn key_up(this: &mut Object, _sel: Sel, nsevent: id) {
        Self::key_common(this, nsevent, false);
    }

    extern "C" fn did_change_screen(this: &mut Object, _sel: Sel, _notification: id) {
        log::trace!("did_change_screen");
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            // Just set a flag; we don't want to react immediately
            // as this even fires as part of a live move and the
            // resize flow may try to re-position the window to
            // the wrong place.
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.screen_changed = true;
            }
        }
    }

    extern "C" fn will_start_live_resize(this: &mut Object, _sel: Sel, _notification: id) {
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.live_resizing = true;
            }
        }
    }

    extern "C" fn will_enter_fullscreen(this: &mut Object, _sel: Sel, _notification: id) {
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.native_fullscreen_transition_active.set(true);
            this.native_fullscreen_target.set(Some(true));
            this.native_fullscreen_transition_start
                .set(Some(Instant::now()));
            // Use try_borrow_mut: AppKit can fire this synchronously while
            // Connection::with_window_inner already holds inner.borrow_mut().
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.live_resizing = true;
            } else {
                log::warn!("will_enter_fullscreen: RefCell already borrowed");
            }
        }
    }

    extern "C" fn window_should_enter_fullscreen(
        this: &mut Object,
        _sel: Sel,
        _window: id,
    ) -> BOOL {
        if let Some(this) = Self::get_this(this) {
            let (window_id, use_native) = {
                let inner = this.inner.borrow();
                (inner.window_id, inner.config.native_macos_fullscreen_mode)
            };
            if use_native {
                YES
            } else {
                Connection::with_window_inner(window_id, move |inner| {
                    inner.toggle_fullscreen();
                    Ok(())
                });
                NO
            }
        } else {
            YES
        }
    }

    extern "C" fn did_enter_fullscreen(this: &mut Object, _sel: Sel, _notification: id) {
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.native_fullscreen_transition_active.set(false);
            this.native_fullscreen_target.set(None);
            // Transition is complete: mark as non-live so the landing resize
            // calls tab.resize() (not resize_visual) and properly commits the
            // terminal buffer to the final fullscreen dimensions.
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.live_resizing = false;
            }
        }
        Self::did_resize(this, _sel, _notification);
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.native_fullscreen_transition_start.set(None);
            {
                // Use try_borrow_mut: AppKit can fire this synchronously while
                // Connection::with_window_inner already holds inner.borrow_mut().
                if let Ok(mut inner) = this.inner.try_borrow_mut() {
                    inner.live_resizing = false;
                    inner.paint_throttled = false;
                    // Clear any screen_changed flag accumulated during the transition
                    // (e.g. from sketchybar firing NSApplicationDidChangeScreenParametersNotification)
                    // so the upcoming NeedRepaint paints immediately instead of doing another resize.
                    inner.screen_changed = false;
                    inner.invalidated = true;
                    let events = inner.events.clone();
                    drop(inner);
                    events.dispatch(WindowEvent::NeedRepaint);
                }
            }
        }
    }

    extern "C" fn will_exit_fullscreen(this: &mut Object, _sel: Sel, _notification: id) {
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            let now = Instant::now();
            this.native_fullscreen_transition_active.set(true);
            this.native_fullscreen_target.set(Some(false));
            this.native_fullscreen_transition_start.set(Some(now));
            this.transition_hide_until.set(None);
            // Use try_borrow_mut: AppKit fires this notification synchronously
            // from inside [NSWindow close], which we call while Connection
            // already holds inner.borrow_mut(). A direct borrow_mut here would
            // panic and abort. Matches the window_will_close pattern.
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.live_resizing = true;
                inner.paint_throttled = false;
                inner.invalidated = true;
                let events = inner.events.clone();
                drop(inner);
                events.dispatch(WindowEvent::NeedRepaint);
            } else {
                log::warn!(
                    "will_exit_fullscreen: RefCell already borrowed (window \
                     closing), skipping display update"
                );
            }
        }
    }

    extern "C" fn did_exit_fullscreen(this: &mut Object, _sel: Sel, _notification: id) {
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.native_fullscreen_transition_active.set(false);
            this.native_fullscreen_target.set(None);
            this.transition_hide_until.set(None);
            // Transition is complete: mark as non-live so the landing resize
            // calls tab.resize() (not resize_visual) and properly commits the
            // terminal buffer to the final windowed dimensions.
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.live_resizing = false;
            }
        }
        Self::did_resize(this, _sel, _notification);
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.native_fullscreen_transition_start.set(None);
            {
                if let Ok(mut inner) = this.inner.try_borrow_mut() {
                    inner.paint_throttled = false;
                    // Clear any screen_changed flag accumulated during the transition
                    // (e.g. from sketchybar firing NSApplicationDidChangeScreenParametersNotification)
                    // so the upcoming NeedRepaint paints immediately instead of doing another resize.
                    inner.screen_changed = false;
                    inner.invalidated = true;
                    inner.live_resizing = false;
                    let events = inner.events.clone();
                    drop(inner);
                    events.dispatch(WindowEvent::NeedRepaint);
                }
            }

            if this.order_out_on_fullscreen_exit.replace(false) {
                if let Ok(inner) = this.inner.try_borrow() {
                    if let Some(window) = inner.window.as_ref() {
                        let window = window.load();
                        if !window.is_null() {
                            unsafe {
                                let () = msg_send![*window, orderOut: nil];
                            }
                        }
                    }
                }
            }
        }
    }

    extern "C" fn did_end_live_resize(this: &mut Object, _sel: Sel, _notification: id) {
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            // Force a final non-live resize pass so TermWindow receives
            // WindowEvent::Resized with live_resizing=false and flushes the
            // deferred PTY size updates from resize_visual().
            if let Ok(mut inner) = this.inner.try_borrow_mut() {
                inner.live_resizing = false;
            }
        }
        Self::did_resize(this, _sel, _notification);
    }

    extern "C" fn did_resize(this: &mut Object, _sel: Sel, _notification: id) {
        // Matches the early-return guard used by did_enter_fullscreen /
        // did_exit_fullscreen. AppKit may fire windowDidResize: while the
        // window is being torn down; dispatching Resized into an
        // already-destroyed TermWindow is pointless and noisy.
        if let Some(view) = Self::get_this(this) {
            if view.is_closing.get() {
                return;
            }
        }
        let view_id = this as *mut Object;
        let frame = unsafe { NSView::frame(this as *mut _) };
        let backing_frame = unsafe { NSView::convertRectToBacking(this as *mut _, frame) };
        let width = backing_frame.size.width;
        let height = backing_frame.size.height;
        let mut window_to_persist = None;
        if let Some(this) = Self::get_this(this) {
            // Avoid recursive borrow panics during fullscreen transition resizes.
            let mut inner = match this.inner.try_borrow_mut() {
                Ok(inner) => inner,
                Err(_) => {
                    let already_scheduled = this.resize_retry_scheduled.replace(true);
                    if !already_scheduled {
                        unsafe {
                            let _: () = msg_send![
                                view_id,
                                performSelector: sel!(windowDidResize:)
                                withObject: nil
                                afterDelay: 0.0
                            ];
                        }
                    }
                    return;
                }
            };

            this.resize_retry_scheduled.set(false);

            if let Some(gl_context_pair) = inner.gl_context_pair.as_ref() {
                gl_context_pair.backend.update();
            }

            // This is a little gross; ideally we'd call
            // WindowInner:is_fullscreen to determine this, but
            // we can't get a mutable reference to it from here
            // as we can be called in a context where something
            // higher up the callstack already has a mutable
            // reference and we'd panic.
            let native_transition_active = this.native_fullscreen_transition_active.get();
            let is_full_screen = if native_transition_active {
                this.native_fullscreen_target.get().unwrap_or(false)
            } else {
                inner.fullscreen.is_some()
                    || inner.window.as_ref().map_or(false, |window| {
                        let window = window.load();
                        let style_mask = unsafe { NSWindow::styleMask(*window) };
                        style_mask.contains(NSWindowStyleMask::NSFullScreenWindowMask)
                    })
            };

            let live_resizing = inner.live_resizing;
            let screen_changed = std::mem::take(&mut inner.screen_changed);

            // Note: isZoomed can falsely return YES during screen changes.
            // When screen_changed is true, carry forward the prior MAXIMIZED
            // state instead of querying isZoomed (which may give stale results
            // for the old screen frame). This avoids both false positives and
            // the prior bug where a truly maximized window lost its state
            // permanently after moving to another screen.
            // <https://github.com/wezterm/wezterm/issues/3503>
            // <https://github.com/tw93/Kaku/issues/131>
            let is_zoomed = if screen_changed {
                inner
                    .last_reported_window_state
                    .contains(WindowState::MAXIMIZED)
            } else {
                !is_full_screen
                    && inner.window.as_ref().map_or(false, |window| {
                        let window = window.load();
                        unsafe { msg_send![*window, isZoomed] }
                    })
            };

            let window_level = inner
                .window
                .as_ref()
                .map(|window| {
                    let level = unsafe { window.load().level() };
                    nswindow_level_to_window_level(level)
                })
                .unwrap_or_default();

            let level_state = match window_level {
                WindowLevel::AlwaysOnBottom => WindowState::ALWAYS_ON_BOTTOM,
                WindowLevel::AlwaysOnTop => WindowState::ALWAYS_ON_TOP,
                WindowLevel::Normal => WindowState::default(),
            };

            let screen_state = match (is_full_screen, is_zoomed) {
                (true, _) => WindowState::FULL_SCREEN,
                (_, true) => WindowState::MAXIMIZED,
                _ => WindowState::default(),
            };

            let fallback_scale = inner
                .window
                .as_ref()
                .and_then(|window| {
                    let window = window.load();
                    if window.is_null() {
                        None
                    } else {
                        let scale: CGFloat = unsafe { msg_send![*window, backingScaleFactor] };
                        if scale > 0.0 {
                            Some(scale as f64)
                        } else {
                            None
                        }
                    }
                })
                .unwrap_or_else(|| {
                    if frame.size.width > 0.0 {
                        backing_frame.size.width / frame.size.width
                    } else {
                        1.0
                    }
                });
            let fallback_dpi = (crate::DEFAULT_DPI * fallback_scale) as usize;
            let screen_dpi = inner.window.as_ref().and_then(|window| {
                let window = window.load();
                dpi_for_window_screen(*window, &inner.config).map(|dpi| dpi as usize)
            });
            let simple_transition_active = this.simple_fullscreen_transition_active.get();
            let transition_active = native_transition_active || simple_transition_active;
            let dpi = if transition_active {
                inner
                    .last_reported_dpi
                    .or(screen_dpi)
                    .unwrap_or(fallback_dpi)
            } else if let Some(dpi) = screen_dpi {
                dpi
            } else {
                fallback_dpi
            };
            inner.last_reported_dpi = Some(dpi);

            let window_state = screen_state | level_state;
            let prior_window_state = inner.last_reported_window_state;
            let events = inner.events.clone();
            let mut pending_events = Vec::new();
            let maximized_toggled = prior_window_state.contains(WindowState::MAXIMIZED)
                != window_state.contains(WindowState::MAXIMIZED);
            let fullscreen_involved = prior_window_state.contains(WindowState::FULL_SCREEN)
                || window_state.contains(WindowState::FULL_SCREEN);
            if maximized_toggled && !fullscreen_involved {
                let hide_ms = ZOOM_HIDE_CONTENT_MS;
                this.transition_hide_until
                    .set(Some(Instant::now() + Duration::from_millis(hide_ms)));
                inner.paint_throttled = false;
                inner.invalidated = true;
                pending_events.push(WindowEvent::NeedRepaint);
            }
            inner.last_reported_window_state = window_state;

            let suppress_intermediate_resize = if native_transition_active {
                match this.native_fullscreen_target.get() {
                    // Enter: WebGpu keeps transition content hidden, so dispatching intermediate
                    // resize updates avoids stale dimensions causing stretched/ghosted edges.
                    // Keep legacy suppression for non-WebGpu backends.
                    Some(true) => inner.config.front_end != config::FrontEndSelection::WebGpu,
                    // Exit: suppress only while hide window is active; then release updates early.
                    Some(false) => this
                        .transition_hide_until
                        .get()
                        .map(|until| Instant::now() < until)
                        .unwrap_or(false),
                    None => true,
                }
            } else {
                false
            };

            if !suppress_intermediate_resize {
                if screen_changed {
                    // Defer the resize dispatch to the next run-loop turn when the resize
                    // was triggered by a display reconfiguration (lock-screen return,
                    // monitor connect/disconnect). Dispatching synchronously here means
                    // we are still inside _NSCGSDisplayConfigurationDidReconfigureNotificationHandler
                    // -> _setFrameCommon -> postNotification, and re-entering AppKit or
                    // doing heavy render work in that context causes a main-thread hang.
                    let dimensions = Dimensions {
                        pixel_width: width as usize,
                        pixel_height: height as usize,
                        dpi,
                    };
                    let window_id = inner.window_id;
                    Connection::with_window_inner(window_id, move |inner| {
                        if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                            window_view.dispatch_event(WindowEvent::Resized {
                                dimensions,
                                window_state,
                                live_resizing,
                                screen_changed: true,
                            });
                        }
                        Ok(())
                    });
                } else {
                    pending_events.push(WindowEvent::Resized {
                        dimensions: Dimensions {
                            pixel_width: width as usize,
                            pixel_height: height as usize,
                            dpi,
                        },
                        window_state,
                        live_resizing,
                        screen_changed,
                    });
                }
            }

            if simple_transition_active {
                this.simple_fullscreen_transition_active.set(false);
                inner.live_resizing = false;
            }

            if !live_resizing && !APP_TERMINATING.load(Ordering::Relaxed) {
                window_to_persist = inner.window.as_ref().map(|window| window.load());
            }
            drop(inner);
            for event in pending_events {
                events.dispatch(event);
            }
        }
        if let Some(window) = window_to_persist {
            if !window.is_null() {
                let _ = persist_window_size_and_position(*window);
            }
        }
    }

    /// Returns the frame to use when zooming (maximizing) the window.
    /// We return the screen's visible frame to ensure the window fills the entire
    /// available space, ignoring resize increments that would otherwise cause
    /// the window to not fill the screen completely.
    /// <https://github.com/tw93/Kaku/issues/131>
    extern "C" fn window_will_use_standard_frame(
        _this: &mut Object,
        _sel: Sel,
        window: id,
        default_frame: NSRect,
    ) -> NSRect {
        unsafe {
            let screen: id = msg_send![window, screen];
            if screen.is_null() {
                return default_frame;
            }
            msg_send![screen, visibleFrame]
        }
    }

    extern "C" fn update_layer(_view: &mut Object, _sel: Sel) {
        log::trace!("update_layer called");
    }

    extern "C" fn wants_update_layer(_view: &mut Object, _sel: Sel) -> BOOL {
        log::trace!("wants_update_layer called");
        YES
    }

    extern "C" fn display_layer(view: &mut Object, sel: Sel, _layer_id: id) {
        Self::draw_rect(
            view,
            sel,
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
        )
    }

    extern "C" fn draw_layer_in_context(
        _view: &mut Object,
        _sel: Sel,
        _layer_id: id,
        _context: id,
    ) {
    }

    extern "C" fn layer_should_inherit_contents_scale_from_window(
        _: &Object,
        _: Sel,
        layer: *mut Object,
        _: CGFloat,
        _: *mut Object,
    ) -> BOOL {
        log::trace!("layer_should_inherit_contents_scale_from_window");
        unsafe {
            let () = msg_send![layer, setContentsScale: 1.0];
        }
        YES
    }

    extern "C" fn make_backing_layer(view: &mut Object, _: Sel) -> id {
        log::trace!("make_backing_layer");
        let (use_metal_backing_layer, force_square) = Self::get_this(view)
            .map(|this| {
                let inner = this.inner.borrow();
                (
                    inner.config.front_end == config::FrontEndSelection::WebGpu,
                    inner
                        .config
                        .window_decorations
                        .contains(WindowDecorations::MACOS_FORCE_SQUARE_CORNERS),
                )
            })
            .unwrap_or((false, false));
        let class = if use_metal_backing_layer {
            class!(CAMetalLayer)
        } else {
            class!(CALayer)
        };
        unsafe {
            // Use type method to get a backing layer instance.
            // So that we don't have to worry about retaining/releasing it.
            let layer: id = msg_send![class, layer];
            let () = msg_send![layer, setDelegate: view];
            let () = msg_send![layer, setContentsScale: 1.0];
            let () = msg_send![layer, setOpaque: NO];

            // Apply corner radius so compositor-clipped window corners
            // don't leave transparent arcs.  The radius set here will be
            // refreshed later by update_window_shadow, but we need an
            // initial value because that function may have already run
            // before AppKit calls make_backing_layer. MACOS_FORCE_SQUARE_CORNERS
            // opts out so tiled neighbor apps on macOS 26 can't poke into
            // the clipped arcs.
            if macos_version_major() >= 26 && !force_square {
                let corner_radius: CGFloat = 10.0;
                let () = msg_send![layer, setCornerRadius: corner_radius];
                let () = msg_send![layer, setMasksToBounds: YES];
            }

            layer
        }
    }

    extern "C" fn draw_rect(view: &mut Object, sel: Sel, _dirty_rect: NSRect) {
        let view_id = view as id;
        if let Some(this) = Self::get_this(view) {
            // Use try_borrow_mut to avoid panic if already borrowed (e.g., during zoom animation)
            let mut inner = match this.inner.try_borrow_mut() {
                Ok(inner) => inner,
                Err(_) => {
                    unsafe {
                        let _: () = msg_send![view_id, setNeedsDisplay: YES];
                    }
                    return;
                }
            };

            if inner.screen_changed {
                // If the screen resolution changed (which can also
                // happen if the window was dragged to another monitor
                // with different dpi), then we treat this as a resize
                // event that will in turn trigger an invalidation
                // and a repaint.
                drop(inner);
                Self::did_resize(view, sel, nil);
                return;
            }

            if inner.paint_throttled {
                inner.invalidated = true;
            } else {
                // Arm throttling before repaint so any re-entrant invalidate()
                // during NeedRepaint is preserved for the next frame.
                inner.invalidated = false;
                inner.paint_throttled = true;
                let window_id = inner.window_id;
                let max_fps = inner.config.max_fps;
                let events = inner.events.clone();
                drop(inner);
                events.dispatch(WindowEvent::NeedRepaint);
                promise::spawn::spawn(async move {
                    async_io::Timer::after(std::time::Duration::from_millis(1000 / max_fps as u64))
                        .await;
                    Connection::with_window_inner(window_id, move |inner| {
                        if let Some(window_view) = WindowView::get_this(unsafe { &**inner.view }) {
                            let mut state = window_view.inner.borrow_mut();
                            state.paint_throttled = false;
                            if state.invalidated {
                                unsafe {
                                    let () = msg_send![*inner.view, setNeedsDisplay: YES];
                                }
                            }
                        }
                        Ok(())
                    });
                })
                .detach();
            }
        }
    }

    extern "C" fn dragging_entered(this: &mut Object, _: Sel, sender: id) -> BOOL {
        if let Some(this) = Self::get_this(this) {
            let pb: id = unsafe { msg_send![sender, draggingPasteboard] };
            if pb.is_null() {
                return NO;
            }

            let filenames =
                unsafe { NSPasteboard::propertyListForType(pb, appkit::NSFilenamesPboardType) };
            if filenames.is_null() {
                return NO;
            }

            let paths = unsafe { filenames.iter() }
                .map(|file| unsafe {
                    let path = nsstring_to_str(file);
                    PathBuf::from(path)
                })
                .collect::<Vec<_>>();
            this.dispatch_event(WindowEvent::DraggedFile(paths));
        }
        YES
    }

    extern "C" fn perform_drag_operation(this: &mut Object, _: Sel, sender: id) -> BOOL {
        if let Some(this) = Self::get_this(this) {
            let pb: id = unsafe { msg_send![sender, draggingPasteboard] };
            if pb.is_null() {
                return NO;
            }

            let filenames =
                unsafe { NSPasteboard::propertyListForType(pb, appkit::NSFilenamesPboardType) };
            if filenames.is_null() {
                return NO;
            }

            let paths = unsafe { filenames.iter() }
                .map(|file| unsafe {
                    let path = nsstring_to_str(file);
                    PathBuf::from(path)
                })
                .collect::<Vec<_>>();
            this.dispatch_event(WindowEvent::DroppedFile(paths));
        }
        YES
    }

    fn get_this(this: &Object) -> Option<&mut Self> {
        unsafe {
            let myself: *mut c_void = *this.get_ivar(VIEW_CLS_NAME);
            if myself.is_null() {
                None
            } else {
                Some(&mut *(myself as *mut Self))
            }
        }
    }

    fn init_with_frame(inner: &Rc<RefCell<Inner>>, rect: NSRect) -> anyhow::Result<StrongPtr> {
        let cls = Self::get_class();

        let view_id: id = unsafe { msg_send![cls, alloc] };
        let view_id: StrongPtr = unsafe { StrongPtr::new(msg_send![view_id, initWithFrame:rect]) };
        inner.borrow_mut().view_id.replace(view_id.weak());

        let view = Box::into_raw(Box::new(Self {
            inner: Rc::clone(&inner),
            window_id: Cell::new(inner.borrow().window_id),
            simple_fullscreen_active: Cell::new(false),
            simple_fullscreen_transition_active: Cell::new(false),
            transition_hide_until: Cell::new(None),
            display_change_opengl_present_until: Cell::new(None),
            native_fullscreen_transition_active: Cell::new(false),
            native_fullscreen_target: Cell::new(None),
            native_fullscreen_transition_start: Cell::new(None),
            resize_retry_scheduled: Cell::new(false),
            is_closing: Cell::new(false),
            order_out_on_fullscreen_exit: Cell::new(false),
        }));

        unsafe {
            (**view_id).set_ivar(VIEW_CLS_NAME, view as *mut c_void);
        }

        Ok(view_id)
    }

    fn get_class() -> &'static Class {
        Class::get(VIEW_CLS_NAME).unwrap_or_else(Self::define_class)
    }

    fn define_class() -> &'static Class {
        let mut cls = ClassDecl::new(VIEW_CLS_NAME, class!(NSView))
            .expect("Unable to register WindowView class");

        cls.add_ivar::<*mut c_void>(VIEW_CLS_NAME);
        cls.add_protocol(
            Protocol::get("NSTextInputClient").expect("failed to get NSTextInputClient protocol"),
        );

        cls.add_protocol(Protocol::get("CALayerDelegate").expect("CALayerDelegate not defined"));

        unsafe {
            cls.add_method(
                sel!(dealloc),
                WindowView::dealloc as extern "C" fn(&mut Object, Sel),
            );

            cls.add_method(
                sel!(kakuPerformKeyAssignment:),
                Self::kaku_perform_key_assignment as extern "C" fn(&mut Object, Sel, *mut Object),
            );

            cls.add_method(
                sel!(windowWillClose:),
                Self::window_will_close as extern "C" fn(&mut Object, Sel, id),
            );

            cls.add_method(
                sel!(windowShouldClose:),
                Self::window_should_close as extern "C" fn(&mut Object, Sel, id) -> BOOL,
            );

            cls.add_method(
                sel!(makeBackingLayer),
                Self::make_backing_layer as extern "C" fn(&mut Object, Sel) -> id,
            );

            cls.add_method(
                sel!(layer:shouldInheritContentsScale:fromWindow:),
                Self::layer_should_inherit_contents_scale_from_window
                    as extern "C" fn(&Object, Sel, *mut Object, CGFloat, *mut Object) -> BOOL,
            );

            cls.add_method(
                sel!(drawRect:),
                Self::draw_rect as extern "C" fn(&mut Object, Sel, NSRect),
            );

            cls.add_method(
                sel!(updateLayer),
                Self::update_layer as extern "C" fn(&mut Object, Sel),
            );

            cls.add_method(
                sel!(wantsUpdateLayer),
                Self::wants_update_layer as extern "C" fn(&mut Object, Sel) -> BOOL,
            );

            cls.add_method(
                sel!(displayLayer:),
                Self::display_layer as extern "C" fn(&mut Object, Sel, id),
            );

            cls.add_method(
                sel!(drawLayer:inContext:),
                Self::draw_layer_in_context as extern "C" fn(&mut Object, Sel, id, id),
            );

            cls.add_method(
                sel!(isFlipped),
                Self::is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
            );

            cls.add_method(
                sel!(isOpaque),
                Self::is_opaque as extern "C" fn(&Object, Sel) -> BOOL,
            );

            cls.add_method(
                sel!(mouseDownCanMoveWindow),
                Self::mouse_down_can_move_window as extern "C" fn(&Object, Sel) -> BOOL,
            );

            cls.add_method(
                sel!(allowsAutomaticWindowTabbing),
                Self::allow_automatic_tabbing as extern "C" fn(&Object, Sel) -> BOOL,
            );

            cls.add_method(
                sel!(windowWillStartLiveResize:),
                Self::will_start_live_resize as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowDidEndLiveResize:),
                Self::did_end_live_resize as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowWillEnterFullScreen:),
                Self::will_enter_fullscreen as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowShouldEnterFullScreen:),
                Self::window_should_enter_fullscreen as extern "C" fn(&mut Object, Sel, id) -> BOOL,
            );
            cls.add_method(
                sel!(windowDidEnterFullScreen:),
                Self::did_enter_fullscreen as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowWillExitFullScreen:),
                Self::will_exit_fullscreen as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowDidExitFullScreen:),
                Self::did_exit_fullscreen as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowDidResize:),
                Self::did_resize as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowWillUseStandardFrame:defaultFrame:),
                Self::window_will_use_standard_frame
                    as extern "C" fn(&mut Object, Sel, id, NSRect) -> NSRect,
            );
            cls.add_method(
                sel!(windowDidMove:),
                Self::did_move as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowDidChangeScreen:),
                Self::did_change_screen as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(kakuPersistWindowStateAfterMove:),
                Self::persist_window_state_after_move as extern "C" fn(&mut Object, Sel, id),
            );

            cls.add_method(
                sel!(windowDidBecomeKey:),
                Self::did_become_key as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowDidResignKey:),
                Self::did_resign_key as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(windowDidChangeOcclusionState:),
                Self::did_change_occlusion_state as extern "C" fn(&mut Object, Sel, id),
            );

            cls.add_method(
                sel!(mouseMoved:),
                Self::mouse_moved_or_dragged as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(mouseDragged:),
                Self::mouse_moved_or_dragged as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(rightMouseDragged:),
                Self::mouse_moved_or_dragged as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(mouseDown:),
                Self::mouse_down as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(mouseUp:),
                Self::mouse_up as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(rightMouseDown:),
                Self::right_mouse_down as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(rightMouseUp:),
                Self::right_mouse_up as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(otherMouseDragged:),
                Self::mouse_moved_or_dragged as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(otherMouseDown:),
                Self::other_mouse_down as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(otherMouseUp:),
                Self::other_mouse_up as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(scrollWheel:),
                Self::scroll_wheel as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(mouseExited:),
                Self::mouse_exited as extern "C" fn(&mut Object, Sel, id),
            );

            cls.add_method(
                sel!(keyDown:),
                Self::key_down as extern "C" fn(&mut Object, Sel, id),
            );
            cls.add_method(
                sel!(keyUp:),
                Self::key_up as extern "C" fn(&mut Object, Sel, id),
            );

            cls.add_method(
                sel!(performKeyEquivalent:),
                Self::perform_key_equivalent as extern "C" fn(&mut Object, Sel, id) -> BOOL,
            );

            cls.add_method(
                sel!(acceptsFirstResponder),
                Self::accepts_first_responder as extern "C" fn(&mut Object, Sel) -> BOOL,
            );

            cls.add_method(
                sel!(acceptsFirstMouse:),
                Self::accepts_first_mouse as extern "C" fn(&mut Object, Sel, id) -> BOOL,
            );

            cls.add_method(
                sel!(viewDidChangeEffectiveAppearance),
                Self::view_did_change_effective_appearance as extern "C" fn(&mut Object, Sel),
            );

            cls.add_method(
                sel!(updateTrackingAreas),
                Self::update_tracking_areas as extern "C" fn(&mut Object, Sel),
            );

            cls.add_method(
                sel!(flagsChanged:),
                Self::flags_changed as extern "C" fn(&mut Object, Sel, id),
            );

            // NSTextInputClient

            cls.add_method(
                sel!(hasMarkedText),
                Self::has_marked_text as extern "C" fn(&mut Object, Sel) -> BOOL,
            );
            cls.add_method(
                sel!(markedRange),
                Self::marked_range as extern "C" fn(&mut Object, Sel) -> NSRange,
            );
            cls.add_method(
                sel!(selectedRange),
                Self::selected_range as extern "C" fn(&mut Object, Sel) -> NSRange,
            );
            cls.add_method(
                sel!(setMarkedText:selectedRange:replacementRange:),
                Self::set_marked_text_selected_range_replacement_range
                    as extern "C" fn(&mut Object, Sel, id, NSRange, NSRange),
            );
            cls.add_method(
                sel!(unmarkText),
                Self::unmark_text as extern "C" fn(&mut Object, Sel),
            );
            cls.add_method(
                sel!(validAttributesForMarkedText),
                Self::valid_attributes_for_marked_text as extern "C" fn(&mut Object, Sel) -> id,
            );
            cls.add_method(
                sel!(doCommandBySelector:),
                Self::do_command_by_selector as extern "C" fn(&mut Object, Sel, Sel),
            );

            cls.add_method(
                sel!( attributedSubstringForProposedRange:actualRange:),
                Self::attributed_substring_for_proposed_range
                    as extern "C" fn(&mut Object, Sel, NSRange, NSRangePointer) -> id,
            );
            cls.add_method(
                sel!(insertText:replacementRange:),
                Self::insert_text_replacement_range as extern "C" fn(&mut Object, Sel, id, NSRange),
            );

            cls.add_method(
                sel!(characterIndexForPoint:),
                Self::character_index_for_point
                    as extern "C" fn(&mut Object, Sel, NSPoint) -> NSUInteger,
            );
            cls.add_method(
                sel!(firstRectForCharacterRange:actualRange:),
                Self::first_rect_for_character_range
                    as extern "C" fn(&mut Object, Sel, NSRange, NSRangePointer) -> NSRect,
            );
            cls.add_method(
                sel!(draggingEntered:),
                Self::dragging_entered as extern "C" fn(&mut Object, Sel, id) -> BOOL,
            );
            cls.add_method(
                sel!(performDragOperation:),
                Self::perform_drag_operation as extern "C" fn(&mut Object, Sel, id) -> BOOL,
            );

            // Accessibility support for voice input tools like Typeless
            cls.add_method(
                sel!(accessibilityRole),
                Self::accessibility_role as extern "C" fn(&Object, Sel) -> id,
            );
            cls.add_method(
                sel!(isAccessibilityElement),
                Self::is_accessibility_element as extern "C" fn(&Object, Sel) -> BOOL,
            );
            cls.add_method(
                sel!(accessibilityRoleDescription),
                Self::accessibility_role_description as extern "C" fn(&Object, Sel) -> id,
            );
        }

        cls.register()
    }
}
