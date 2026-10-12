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
    self, CGFloat, NSApp, NSApplication, NSApplicationActivateIgnoringOtherApps,
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
use std::collections::BTreeMap;
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

/// True once `on_app_terminating` has been entered. AppKit sets this before
/// it sends `windowShouldClose:` to individual NSWindows during quit, so the
/// GUI layer can distinguish "user pressed Cmd+Q" from "user closed a single
/// window" inside its close-requested callback.
pub fn is_app_terminating() -> bool {
    APP_TERMINATING.load(Ordering::Relaxed)
}

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
#[cfg(test)]
const HOTKEY_SLIDE_DURATION: Duration = Duration::from_millis(200);
// cocoa 0.25 does not expose these newer AppKit collection-behavior bits.
// Keep the raw values local so Kaku can opt into native macOS window tiling
// without broadening the dependency surface.
const NS_WINDOW_COLLECTION_BEHAVIOR_FULLSCREEN_ALLOWS_TILING_BITS: NSUInteger = 1 << 11;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULLSCREEN_DISALLOWS_TILING_BITS: NSUInteger = 1 << 12;
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

fn gl_layer_is_opaque(window_opacity: f32) -> BOOL {
    if window_opacity >= 1.0 {
        YES
    } else {
        NO
    }
}

impl GlContextPair {
    /// on macOS we first try to initialize EGL by dynamically loading it.
    /// The system doesn't provide an EGL implementation, but the ANGLE
    /// project (and MetalANGLE) both provide implementations.
    /// The ANGLE EGL implementation wants a CALayer descendant passed
    /// as the EGLNativeWindowType.
    pub fn create(view: id, window_opacity: f32) -> anyhow::Result<Self> {
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
            let layer_opaque = gl_layer_is_opaque(window_opacity);
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
    // Park the native host while the terminal view is in a panel. A dedicated
    // drawer's host is newly created and never shown as an ordinary window.
    ordinary_window: Option<StrongPtr>,
    ordinary_window_screens: Vec<NSRect>,
    dedicated_hotkey: bool,
    config: ConfigHandle,
}

#[derive(Clone, Copy)]
struct HotkeySlide {
    resting_frame: NSRect,
    from: NSPoint,
    to: NSPoint,
    started: Instant,
    duration: Duration,
    showing: bool,
    hide_on_focus_loss: bool,
}

fn hotkey_offscreen_origin(frame: NSRect, screen: NSRect) -> NSPoint {
    // AppKit points have a bottom-left origin, including on secondary screens.
    NSPoint::new(frame.origin.x, screen.origin.y + screen.size.height)
}

impl HotkeySlide {
    fn should_hide_when_cancelled(self, is_key: bool) -> bool {
        !(self.showing || self.hide_on_focus_loss && is_key)
    }

    fn reverse_on_focus_gain(self, is_key: bool, current_origin: NSPoint) -> Option<Self> {
        if !self.hide_on_focus_loss || self.showing || !is_key {
            return None;
        }
        Some(Self {
            from: current_origin,
            to: self.resting_frame.origin,
            started: Instant::now(),
            showing: true,
            hide_on_focus_loss: false,
            ..self
        })
    }

    fn reverse_when_auto_hide_disabled(
        self,
        hide_on_focus_loss: bool,
        current_origin: NSPoint,
    ) -> Option<Self> {
        // Pinning cancels only an in-flight automatic hide, never a hotkey hide.
        self.reverse_on_focus_gain(!hide_on_focus_loss, current_origin)
    }

    fn origin_at(self, elapsed: Duration) -> NSPoint {
        if self.duration.is_zero() {
            return self.to;
        }
        let t = (elapsed.as_secs_f64() / self.duration.as_secs_f64()).min(1.);
        let eased = t * t * (3. - 2. * t);
        NSPoint::new(
            self.from.x + (self.to.x - self.from.x) * eased,
            self.from.y + (self.to.y - self.from.y) * eased,
        )
    }
}

fn hotkey_should_hide(showing: Option<bool>, is_key: bool, is_visible: bool) -> bool {
    showing.unwrap_or(is_key && is_visible)
}

fn drawer_hotkey_should_hide(
    showing: Option<bool>,
    is_key: bool,
    is_visible: bool,
    hide_on_focus_loss: bool,
) -> bool {
    showing.unwrap_or(is_visible && (is_key || !hide_on_focus_loss))
}

fn hotkey_animation_duration(duration_ms: u64, reduce_motion: bool) -> Duration {
    if reduce_motion {
        Duration::ZERO
    } else {
        Duration::from_millis(duration_ms)
    }
}

fn hotkey_extend_behind_notch(
    is_panel: bool,
    override_value: Option<bool>,
    inherited: bool,
) -> bool {
    if is_panel {
        override_value.unwrap_or(inherited)
    } else {
        inherited
    }
}

fn should_hide_hotkey_on_focus_loss(
    is_panel: bool,
    is_key: bool,
    is_visible: bool,
    showing: Option<bool>,
    enabled: bool,
) -> bool {
    enabled && is_panel && !is_key && is_visible && showing != Some(false)
}

fn defer_hotkey_request(pending: &Cell<Option<bool>>, transitioning: bool, showing: bool) -> bool {
    pending.set(if transitioning { Some(showing) } else { None });
    transitioning
}

fn cancel_hotkey_visibility_intent(pending: &Cell<Option<bool>>, exit_hide: &Cell<bool>) {
    // A stable fullscreen hide can set the exit marker without queuing a hotkey intent.
    pending.set(None);
    exit_hide.set(false);
}

#[derive(Clone, Copy)]
struct ScreenResize {
    dimensions: Dimensions,
    window_state: WindowState,
    live_resizing: bool,
}

#[derive(Default)]
struct DeferredScreenResize {
    pending: Cell<Option<ScreenResize>>,
}

impl DeferredScreenResize {
    fn is_pending(&self) -> bool {
        self.pending.get().is_some()
    }

    // Only the first snapshot needs a callback; later resizes replace its payload.
    fn update(&self, resize: ScreenResize) -> bool {
        self.pending.replace(Some(resize)).is_none()
    }

    fn take(&self) -> Option<ScreenResize> {
        self.pending.take()
    }
}

fn content_size_changed(current: NSSize, requested: NSSize) -> bool {
    (requested.width - current.width).abs() > 0.01
        || (requested.height - current.height).abs() > 0.01
}

fn should_start_native_fullscreen_exit(transitioning: bool, target: Option<bool>) -> bool {
    !(transitioning && target == Some(false))
}

fn use_appkit_fullscreen_toggle(use_native: bool, hotkey_panel: bool) -> bool {
    // A panel must return its terminal view to the ordinary window first.
    use_native && !hotkey_panel
}

fn set_titlebar_button_hidden(button: id, hidden: BOOL) {
    // Panels and borderless/fullscreen windows may not have every button.
    if button.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![button, setHidden: hidden];
    }
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

fn hotkey_panel_style_mask(mask: NSWindowStyleMask) -> NSWindowStyleMask {
    // A borderless host avoids native rounded chrome. The terminal view opts
    // into nonactivating panel keyboard focus via needsPanelToBecomeKey.
    (mask & NSWindowStyleMask::NSResizableWindowMask)
        | nonactivating_panel_mask()
        | NSWindowStyleMask::NSFullSizeContentViewWindowMask
}

fn hotkey_window_level() -> NSWindowLevel {
    use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
    // Cover both the menu bar and status items, but leave system popups above us.
    i64::from(
        CGWindowLevelForKey(CGWindowLevelKey::MainMenuWindowLevelKey)
            .max(CGWindowLevelForKey(CGWindowLevelKey::StatusWindowLevelKey)),
    ) + 1
}

fn nonactivating_panel_mask() -> NSWindowStyleMask {
    // NSWindowStyleMaskNonactivatingPanel is missing from cocoa's bindings.
    unsafe { NSWindowStyleMask::from_bits_unchecked(1 << 7) }
}

fn is_hotkey_panel(window: id) -> bool {
    unsafe {
        let panel: BOOL = msg_send![window, isKindOfClass: get_hotkey_window_class()];
        panel == YES
    }
}

fn hotkey_collection_behavior(
    mut behavior: appkit::NSWindowCollectionBehavior,
    macos_major: u32,
) -> appkit::NSWindowCollectionBehavior {
    behavior.remove(
        appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
            | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
            | fullscreen_allows_tiling_behavior(),
    );
    behavior.insert(
        appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
            | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
    );
    if macos_major >= 13 {
        // Newer Stage Manager / fullscreen roles are absent from cocoa.
        unsafe {
            behavior.remove(appkit::NSWindowCollectionBehavior::from_bits_unchecked(
                (1 << 16) | (1 << 17),
            ));
            behavior.insert(appkit::NSWindowCollectionBehavior::from_bits_unchecked(
                1 << 18,
            ));
        }
    }
    behavior
}

fn set_window_position(window: *mut Object, coords: ScreenPoint) {
    unsafe {
        let cartesian = screen_point_to_cartesian(coords);
        let frame = NSWindow::frame(window);
        let content_frame = NSWindow::contentRectForFrameRect_(window, frame);
        let delta_x = content_frame.origin.x - frame.origin.x;
        let delta_y = content_frame.origin.y - frame.origin.y;
        let mut point = NSPoint::new(
            cartesian.x as f64 - delta_x,
            cartesian.y as f64 - delta_y - content_frame.size.height,
        );

        // Manual drags and programmatic moves can otherwise place the title
        // bar behind the macOS menu bar, where the cursor can no longer
        // reach it (#508). Native drags never allow this: AppKit pins the
        // frame's top edge to the visible frame. Mirror that constraint for
        // the top edge only; the other edges may go off screen, matching
        // native drag behavior.
        let new_frame = NSRect::new(point, frame.size);
        if let Some(visible) = visible_frame_for_target_frame(window, new_frame) {
            if let Some(clamped_y) = clamp_frame_top_below_menu_bar(new_frame, visible) {
                point.y = clamped_y;
            }
        }

        NSWindow::setFrameOrigin_(window, point);
    }
}

const MIN_RESTORE_WIDTH: usize = 200;
const MIN_RESTORE_HEIGHT: usize = 120;

thread_local! {
    static LAST_CLOSED_WINDOW_POSITION: RefCell<Option<ScreenPoint>> = RefCell::new(None);
    // The view survives the NSWindow -> NSPanel host swap; never retain a session here.
    static OS_PARAMETER_VIEWS: RefCell<BTreeMap<usize, WeakPtr>> = RefCell::new(BTreeMap::new());
    // Sync drag flag: set by request_drag_move(), checked by mouse_down to execute performWindowDragWithEvent:
    static PENDING_DRAG_MOVE: Cell<bool> = Cell::new(false);
    static PENDING_DRAG_MOVE_FROM_MAXIMIZED: Cell<bool> = Cell::new(false);
    // Set by mouse_down when a maximized/zoomed window wants a native drag, then
    // consumed by the first mouseDragged: so a bare single click never reaches
    // performWindowDragWithEvent: (which macOS would treat as a snap/maximize). #414
    static ARMED_MAXIMIZED_NATIVE_DRAG: Cell<bool> = Cell::new(false);
}

fn os_parameters_native_view(
    window_id: usize,
    borrowed_view: impl FnOnce() -> Option<id>,
) -> Option<StrongPtr> {
    let registered = OS_PARAMETER_VIEWS.with(|views| views.borrow().get(&window_id).cloned());
    let view = match registered {
        Some(view) => view.load(),
        None => unsafe { StrongPtr::retain(borrowed_view()?) },
    };
    if view.is_null() {
        None
    } else {
        Some(view)
    }
}

fn os_parameters_screen(window: Option<id>, main_screen: impl FnOnce() -> id) -> id {
    let screen = window.map_or(nil, |window| unsafe { msg_send![window, screen] });
    if screen.is_null() {
        main_screen()
    } else {
        screen
    }
}

fn os_parameters_native_window(view: &StrongPtr) -> Option<StrongPtr> {
    let window: id = unsafe { msg_send![**view, window] };
    if window.is_null() {
        None
    } else {
        Some(unsafe { StrongPtr::retain(window) })
    }
}

fn os_parameters_native_state(window: id, window_state: WindowState) -> (bool, bool) {
    let native_full_screen =
        unsafe { NSWindow::styleMask(window) }.contains(NSWindowStyleMask::NSFullScreenWindowMask);
    let hotkey_panel = window_state.contains(WindowState::HOTKEY_WINDOW) || is_hotkey_panel(window);
    (native_full_screen, hotkey_panel)
}

fn screen_safe_area_top(screen: id) -> Option<usize> {
    if screen.is_null() {
        return None;
    }
    get_screen_safe_area_insets(screen).map(|insets| unsafe {
        let frame = NSScreen::frame(screen);
        let backing_frame = NSScreen::convertRectToBacking_(screen, frame);
        let scale = backing_frame.size.height / frame.size.height;
        (insets.top.ceil() * scale) as usize
    })
}

fn make_terminal_key_window(window: &StrongPtr, view: &StrongPtr) {
    unsafe {
        window.makeKeyAndOrderFront_(nil);
        // A key NSWindow can still be its own first responder instead of the terminal.
        window.makeFirstResponder_(**view);
    }
}

fn set_window_style_mask_preserving_responder(window: &StrongPtr, mask: NSWindowStyleMask) {
    unsafe {
        // AppKit can reset firstResponder while rebuilding the window's frame view.
        let responder: id = msg_send![**window, firstResponder];
        let responder = if responder.is_null() {
            None
        } else {
            Some(StrongPtr::retain(responder))
        };
        window.setStyleMask_(mask);
        if let Some(responder) = responder {
            window.makeFirstResponder_(*responder);
        }
    }
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
    // Never serialize an absent version as `null`. A missing version marks an
    // incomplete initialization that bundled kaku.lua will retry while
    // preserving the window fields written here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    managed_shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_geometry: Option<PersistedWindowSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    window_position: Option<PersistedWindowPosition>,
    #[serde(default, flatten)]
    extra: BTreeMap<String, serde_json::Value>,
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
    if window.is_null() || is_hotkey_panel(window) {
        return;
    }

    let content_view: id = unsafe { msg_send![window, contentView] };
    if !content_view.is_null() {
        if let Some(view) = unsafe { WindowView::get_this(&*content_view) } {
            if view.hotkey_slide.get().is_some() {
                return;
            }
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestedDragAction {
    /// Do not start a native window drag for this press.
    None,
    /// Arm a native drag and start it only once the pointer actually moves.
    DeferUntilDrag,
    /// Start the native drag synchronously from the mouse-down event.
    PerformNow,
}

/// Decide how a title-area press that requested a native window drag should be
/// handled.
///
/// A maximized/zoomed window looks identical on a plain single click and on the
/// start of a drag, so firing `performWindowDragWithEvent:` directly from
/// mouse_down lets macOS interpret a bare title-area click as a snap/maximize
/// gesture (#414). For that case we defer: arm the drag and only begin it when a
/// `mouseDragged:` arrives, so a click is a no-op while a real drag still pulls
/// the window off the top (#428). Non-maximized native drags keep firing
/// immediately, matching the prior behavior.
fn requested_window_drag_action(
    in_fullscreen: bool,
    is_zoomed: bool,
    fills_visible_frame: bool,
    from_maximized: bool,
) -> RequestedDragAction {
    if in_fullscreen {
        return RequestedDragAction::None;
    }
    // GUI state may lag AppKit while crossing screens, and a window can fill
    // the desktop without being zoomed. Neither state should disable dragging.
    // Treat either signal as maximized for click protection, then let AppKit
    // handle the actual drag once the pointer moves.
    if from_maximized || is_zoomed || fills_visible_frame {
        RequestedDragAction::DeferUntilDrag
    } else {
        RequestedDragAction::PerformNow
    }
}

/// Frame origin that centers a window of `size` inside `visible`, both in the
/// same screen's point coordinates. A window larger than the visible frame is
/// pinned to the visible origin instead of drifting onto a neighbor screen.
fn centered_frame_origin(visible: NSRect, size: NSSize) -> NSPoint {
    NSPoint::new(
        visible.origin.x + ((visible.size.width - size.width) / 2.).max(0.),
        visible.origin.y + ((visible.size.height - size.height) / 2.).max(0.),
    )
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

fn frame_after_display_change(
    frame: NSRect,
    simple_fullscreen: bool,
    fills_visible_frame: bool,
    screen: ScreenGeometry,
) -> NSRect {
    if simple_fullscreen {
        screen.frame
    } else if fills_visible_frame {
        screen.visible_frame
    } else {
        fit_frame_to_visible_frame(frame, screen.visible_frame).unwrap_or(frame)
    }
}

/// Returns the y for `setFrameOrigin` that keeps the frame's top edge at or
/// below the visible frame's top (i.e. below the menu bar), or None when the
/// frame is already compliant. Only the top edge is constrained: windows may
/// legitimately hang off the left, right, or bottom of the screen, exactly
/// like a native title-bar drag allows.
fn clamp_frame_top_below_menu_bar(frame: NSRect, visible_frame: NSRect) -> Option<f64> {
    if visible_frame.size.width <= 0.0 || visible_frame.size.height <= 0.0 {
        return None;
    }
    let top = frame.origin.y + frame.size.height;
    let visible_top = visible_frame.origin.y + visible_frame.size.height;
    if top > visible_top + 0.5 {
        Some(visible_top - frame.size.height)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct ScreenGeometry {
    frame: NSRect,
    visible_frame: NSRect,
}

#[derive(Clone, Copy, Default)]
struct WindowedScreenState {
    screen_id: Option<u32>,
    screen: Option<ScreenGeometry>,
    frame: Option<NSRect>,
    fills_visible_frame: bool,
    max_invalidated_by_move: bool,
}

impl WindowedScreenState {
    fn settled(self, frame: NSRect, screen_id: Option<u32>, screen: ScreenGeometry) -> Self {
        let fills_visible_frame = nsrect_approx_eq(frame, screen.visible_frame, 1.0);
        Self {
            screen_id,
            screen: Some(screen),
            frame: Some(frame),
            fills_visible_frame,
            max_invalidated_by_move: self.max_invalidated_by_move && !fills_visible_frame,
        }
    }

    fn after_frame_change(
        self,
        frame: NSRect,
        screen_id: Option<u32>,
        screen: ScreenGeometry,
        previous_screen: Option<ScreenGeometry>,
    ) -> Self {
        if let Some(previous) = self.screen {
            // Topology changes can move a window before the observer runs.
            // Only a move on unchanged live screens updates its saved intent.
            if !previous_screen.is_some_and(|live| {
                nsrect_approx_eq(previous.frame, live.frame, 0.5)
                    && nsrect_approx_eq(previous.visible_frame, live.visible_frame, 0.5)
            }) {
                return self;
            }
        }
        let mut next = self.settled(frame, screen_id, screen);
        if let Some(previous) = self.frame {
            let resized = (previous.size.width - frame.size.width).abs() > 0.5
                || (previous.size.height - frame.size.height).abs() > 0.5;
            let moved = self.screen_id != screen_id
                || (previous.origin.x - frame.origin.x).abs() > 0.5
                || (previous.origin.y - frame.origin.y).abs() > 0.5;
            if resized || next.fills_visible_frame {
                next.max_invalidated_by_move = false;
            } else if moved {
                next.max_invalidated_by_move = true;
            }
        }
        next
    }

    fn fills_after_topology_change(self, last_maximized: bool) -> bool {
        self.fills_visible_frame || (last_maximized && !self.max_invalidated_by_move)
    }
}

fn hotkey_panel_frame(screen: ScreenGeometry, height_percent: f64) -> NSRect {
    // AppKit points, not backing pixels. The drawer covers the menu bar area.
    let height = screen.frame.size.height * height_percent / 100.;
    NSRect::new(
        NSPoint::new(
            screen.frame.origin.x,
            screen.frame.origin.y + screen.frame.size.height - height,
        ),
        NSSize::new(screen.frame.size.width, height),
    )
}

fn frame_after_display_topology_change(
    frame: NSRect,
    simple_fullscreen: bool,
    fills_visible_frame: bool,
    previous_screens: &[NSRect],
    current_screens: &[NSRect],
    screen: ScreenGeometry,
) -> NSRect {
    // Added/reordered displays need no correction. Any removed or resized
    // display can strand a window spanning that display, even if its own stays.
    let prior_displays_unchanged = !previous_screens.is_empty()
        && previous_screens.iter().all(|previous| {
            current_screens
                .iter()
                .any(|current| nsrect_approx_eq(*previous, *current, 0.5))
        });
    if prior_displays_unchanged {
        frame
    } else {
        frame_after_display_change(frame, simple_fullscreen, fills_visible_frame, screen)
    }
}

fn screen_frames() -> Vec<NSRect> {
    unsafe {
        let screens = NSScreen::screens(nil);
        (0..NSArray::count(screens))
            .map(|index| {
                let screen = NSArray::objectAtIndex(screens, index);
                msg_send![screen, frame]
            })
            .collect()
    }
}

fn select_hotkey_screen_geometry(
    mouse: Option<NSPoint>,
    screens: &[ScreenGeometry],
    fallback: Option<ScreenGeometry>,
) -> Option<ScreenGeometry> {
    mouse
        .and_then(|point| {
            screens.iter().find(|screen| {
                let frame = screen.frame;
                // Half-open edges select exactly one of two adjacent displays.
                point.x >= frame.origin.x
                    && point.x < frame.origin.x + frame.size.width
                    && point.y >= frame.origin.y
                    && point.y < frame.origin.y + frame.size.height
            })
        })
        .copied()
        .or(fallback)
}

fn hotkey_screen_geometry(follow_mouse: bool) -> Option<ScreenGeometry> {
    unsafe {
        let main = NSScreen::mainScreen(nil);
        let fallback = if main.is_null() {
            None
        } else {
            Some(ScreenGeometry {
                frame: msg_send![main, frame],
                visible_frame: msg_send![main, visibleFrame],
            })
        };
        if !follow_mouse {
            return fallback;
        }

        // mouseLocation and NSScreen frames are both global AppKit points.
        let mouse = NSEvent::mouseLocation(nil);
        let screens = NSScreen::screens(nil);
        let count = NSArray::count(screens);
        let mut geometries = Vec::with_capacity(count as usize);
        for i in 0..count {
            let screen = NSArray::objectAtIndex(screens, i);
            geometries.push(ScreenGeometry {
                frame: msg_send![screen, frame],
                visible_frame: msg_send![screen, visibleFrame],
            });
        }
        select_hotkey_screen_geometry(Some(mouse), &geometries, fallback)
    }
}

fn rect_top(rect: NSRect) -> f64 {
    rect.origin.y + rect.size.height
}

/// True when the two rects share more than a touching edge horizontally.
/// Adjacent displays in a macOS arrangement abut exactly, so the comparison
/// must be strict: treating `maxX == minX` as overlap would make every
/// side-by-side pair look stacked.
fn rects_overlap_horizontally(a: NSRect, b: NSRect) -> bool {
    a.origin.x < b.origin.x + b.size.width && b.origin.x < a.origin.x + a.size.width
}

/// Visible frame whose top edge bounds the title bar of `target`.
///
/// A display bounds the title bar when the window spans it horizontally, and
/// the most restrictive such display wins: while any part of the title bar sits
/// over a display, it stays below that display's menu bar. Sampling one point
/// at the window's horizontal center instead is what #519 reported. A window
/// straddling a menu-bar display and a taller neighbour had its center on the
/// neighbour, so the clamp used the neighbour's inset-free `visibleFrame`, and
/// the part of the title bar over the first display rose past its menu bar into
/// space the cursor cannot reach.
///
/// A display stops bounding the title bar once the title bar has risen clear of
/// that display's frame into a display stacked above it, so a window crossing
/// upward is not pinned to the monitor it is leaving (#508). The clearance test
/// matters: excluding a display merely because a taller neighbour exists spans
/// the whole arrangement horizontally when the displays are stacked, which
/// silently disables the menu bar clamp for every window sitting on the lower
/// display and lets its title bar rest behind the menu bar, unreachable.
///
/// The tallest spanned display has no taller neighbour, so at least one display
/// always survives the filter.
fn select_visible_frame_for_target(
    target: NSRect,
    screens: &[ScreenGeometry],
    current: Option<NSRect>,
) -> Option<NSRect> {
    let target_top = rect_top(target);
    let spanned: Vec<&ScreenGeometry> = screens
        .iter()
        .filter(|screen| rects_overlap_horizontally(target, screen.frame))
        .collect();

    spanned
        .iter()
        .filter(|screen| {
            let cleared = target_top > rect_top(screen.frame);
            let stacked_under_taller = spanned.iter().any(|other| {
                rect_top(other.frame) > rect_top(screen.frame)
                    && rects_overlap_horizontally(other.frame, screen.frame)
            });
            !(cleared && stacked_under_taller)
        })
        .map(|screen| screen.visible_frame)
        .min_by(|left, right| rect_top(*left).total_cmp(&rect_top(*right)))
        .or(current)
}

/// Visible frame bounding the title bar of the position `frame` is headed to,
/// falling back to the window's own screen when it spans no display at all.
fn visible_frame_for_target_frame(window: id, frame: NSRect) -> Option<NSRect> {
    let current = visible_frame_for_window(window);
    unsafe {
        let screens: id = msg_send![class!(NSScreen), screens];
        let count: usize = msg_send![screens, count];
        let mut geometries = Vec::with_capacity(count);
        for i in 0..count {
            let screen: id = msg_send![screens, objectAtIndex: i];
            let sframe: NSRect = msg_send![screen, frame];
            let visible_frame: NSRect = msg_send![screen, visibleFrame];
            geometries.push(ScreenGeometry {
                frame: sframe,
                visible_frame,
            });
        }
        select_visible_frame_for_target(frame, &geometries, current)
    }
}

fn visible_frame_for_window(window: id) -> Option<NSRect> {
    screen_geometry_for_window(window).map(|screen| screen.visible_frame)
}

fn screen_geometry_for_window(window: id) -> Option<ScreenGeometry> {
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
            Some(ScreenGeometry {
                frame: msg_send![screen, frame],
                visible_frame: msg_send![screen, visibleFrame],
            })
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
    screen_geometry_for_identifier(screen_id).is_some()
}

fn screen_geometry_for_identifier(screen_id: u32) -> Option<ScreenGeometry> {
    unsafe {
        let screens = NSScreen::screens(nil);
        let count = screens.count();
        for idx in 0..count {
            let screen = screens.objectAtIndex(idx);
            if screen_identifier_for_screen(screen) == Some(screen_id) {
                return Some(ScreenGeometry {
                    frame: msg_send![screen, frame],
                    visible_frame: msg_send![screen, visibleFrame],
                });
            }
        }
    }

    None
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
        managed_shell: None,
        window_geometry: Some(size),
        window_position: None,
        extra: BTreeMap::new(),
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
    if window.is_null() || is_hotkey_panel(window) {
        return false;
    }

    let content_view: id = unsafe { msg_send![window, contentView] };
    if !content_view.is_null() {
        if let Some(window_view) = unsafe { WindowView::get_this(&*content_view) } {
            if window_view.simple_fullscreen_active.get()
                || window_view.simple_fullscreen_transition_active.get()
                || window_view.native_fullscreen_transition_active.get()
                || window_view.hotkey_slide.get().is_some()
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
        let tracked_windows: Vec<_> = conn.windows.borrow().values().cloned().collect();
        for window_inner in tracked_windows {
            let mut inner = window_inner.borrow_mut();
            if inner.dedicated_hotkey {
                continue;
            }
            inner.restore_ordinary_window();
            let window = *inner.window;
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
    pub async fn discard_hotkey_window() {
        if let Some(id) = super::app::take_hotkey_window() {
            let _ = Connection::with_window_inner(id, |inner| {
                inner.close();
                Ok(())
            })
            .await;
        }
    }

    /// Called on the GUI thread before the newly created window is ever shown.
    pub fn initialize_hotkey_window(&self, visible: bool) -> anyhow::Result<()> {
        let conn =
            Connection::get().ok_or_else(|| anyhow!("window connection is not initialized"))?;
        let handle = conn
            .window_by_id(self.id)
            .ok_or_else(|| anyhow!("invalid window id {}", self.id))?;
        let mut inner = handle.borrow_mut();
        inner.dedicated_hotkey = true;
        inner.prepare_global_hotkey_panel();
        super::app::register_hotkey_window(self.id);
        if visible {
            inner.show_global_hotkey_window();
        } else {
            inner.dispatch_resize_event();
        }
        Ok(())
    }

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
        // x/y are canonical ScreenPoint pixels; set_window_position performs
        // the conversion to AppKit points, so no scale division here.

        let explicit_initial_pos = match (x, y) {
            (Some(x), Some(y)) => Some(ScreenPoint::new(x as isize, y as isize)),
            _ => None,
        };
        let is_first_window = !conn
            .windows
            .borrow()
            .values()
            .any(|window| !window.borrow().is_dedicated_hotkey());
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
                fullscreen_screens: Vec::new(),
                display_screen_frames: screen_frames(),
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
                ordinary_window: None,
                ordinary_window_screens: Vec::new(),
                dedicated_hotkey: false,
                view,
                config: config.clone(),
            }));
            inner.borrow_mut().window.replace(weak_window);
            OS_PARAMETER_VIEWS.with(|views| {
                views
                    .borrow_mut()
                    .insert(window_id, window_inner.borrow().view.weak());
            });
            conn.windows
                .borrow_mut()
                .insert(window_id, Rc::clone(&window_inner));
            // The startup pipeline has now created its window, so a later
            // dock-icon reopen with no windows is free to spawn again.
            crate::connection::clear_startup_pending_first_window();

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

pub fn nswindow_level_to_window_level(
    nswindow_level: NSWindowLevel,
    hotkey_panel: bool,
) -> WindowLevel {
    if hotkey_panel {
        return WindowLevel::AlwaysOnTop;
    }
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
    fn show_context_menu(&self, menu: crate::os::macos::menu::Menu, point: ScreenPoint) {
        if self.ns_window().is_some() {
            unsafe {
                // ScreenPoint uses Kaku's global top-left coordinate space,
                // which is normalized against the primary display in
                // cartesian_to_screen_point. Invert that same transform;
                // using the secondary display frame here introduces an
                // offset on external monitors.
                let primary = NSScreen::screens(nil).objectAtIndex(0);
                let primary_frame: cocoa::foundation::NSRect = msg_send![primary, frame];
                let backing = NSScreen::convertRectToBacking_(primary, primary_frame);
                let scale = backing.size.height / primary_frame.size.height;
                let cocoa_y = primary_frame.size.height - point.y as f64 / scale;
                menu.pop_up_at_screen_point((point.x as f64 / scale) as isize, cocoa_y as isize);
            }
        }
    }

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
            inner.clear_pending_hotkey_visibility();
            inner.order_out_after_fullscreen_exit();
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
        PENDING_DRAG_MOVE_FROM_MAXIMIZED.with(|flag| flag.set(false));
    }

    fn request_drag_move_from_maximized(&self) {
        PENDING_DRAG_MOVE.with(|flag| flag.set(true));
        PENDING_DRAG_MOVE_FROM_MAXIMIZED.with(|flag| flag.set(true));
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

    fn center(&self) {
        Connection::with_window_inner(self.id, move |inner| {
            inner.center();
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
        window_state: WindowState,
    ) -> anyhow::Result<Option<Parameters>> {
        // We implement this method primarily to provide Notch-avoidance for
        // systems with a notch.
        // Non-native fullscreen and top-edge drawers can extend behind the notch.

        // Resize dispatch can hold WindowInner's mutable borrow. Use the weak
        // view to query its current host rather than falling back to mainScreen.
        let Some(native_view) = os_parameters_native_view(self.id, || self.ns_view()) else {
            return Ok(None);
        };
        let window_view = unsafe { WindowView::get_this(&**native_view) };
        if window_view
            .as_ref()
            .is_some_and(|view| view.is_closing.get())
        {
            return Ok(None);
        }
        let Some(native_window) = os_parameters_native_window(&native_view) else {
            return Ok(None);
        };
        let (native_full_screen, hotkey_panel) =
            os_parameters_native_state(*native_window, window_state);

        // For simple fullscreen, window_state may lag one frame behind the style/frame update.
        // Track the state in WindowView::simple_fullscreen_active (Cell<bool>) so we can
        // read it without borrowing the RefCell and avoid borrow conflicts during resize.
        let simple_full_screen = !hotkey_panel
            && window_view
                .map(|view| view.simple_fullscreen_active.get())
                .unwrap_or(false);

        let should_apply_notch_padding = (simple_full_screen || hotkey_panel)
            && !native_full_screen
            && !hotkey_extend_behind_notch(
                hotkey_panel,
                config.macos_global_hotkey_extend_behind_notch,
                config.macos_fullscreen_extend_behind_notch,
            );

        let border_dimensions = if should_apply_notch_padding {
            let screen = os_parameters_screen(Some(*native_window), || unsafe {
                NSScreen::mainScreen(nil)
            });
            if let Some(top) = screen_safe_area_top(screen) {
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
pub(crate) fn cartesian_to_screen_point(cartesian: NSPoint) -> ScreenPoint {
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
        if self.ordinary_window.is_some() {
            false
        } else if self.is_native_fullscreen() {
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
        self.cancel_hotkey_slide();
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
                    current_app.setPresentationOptions_(
                        NSApplicationPresentationOptions::NSApplicationPresentationDefault,
                    );
                    let (fills_visible_frame, previous_screens) = {
                        let inner = window_view.inner.borrow();
                        (
                            window_view.windowed_screen.get().fills_visible_frame,
                            inner.fullscreen_screens.clone(),
                        )
                    };
                    let restored_rect = screen_geometry_for_window(*self.window)
                        .map(|screen| {
                            frame_after_display_topology_change(
                                saved_rect,
                                false,
                                fills_visible_frame,
                                &previous_screens,
                                &screen_frames(),
                                screen,
                            )
                        })
                        .unwrap_or(saved_rect);
                    self.window.setFrame_display_(restored_rect, YES);
                    let clear: id = msg_send![class!(NSColor), clearColor];
                    let opaque = if self.config.window_background_opacity >= 1.0 {
                        YES
                    } else {
                        NO
                    };
                    let _: () = msg_send![*self.window, setOpaque: opaque];
                    let _: () = msg_send![*self.window, setBackgroundColor: clear];
                    window_view.inner.borrow_mut().fullscreen.take();
                    window_view.inner.borrow_mut().fullscreen_screens.clear();
                    window_view.simple_fullscreen_active.set(false);
                    window_view.settle_windowed_screen_state(*self.window);
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
                    {
                        let mut inner = window_view.inner.borrow_mut();
                        inner.fullscreen.replace(saved_rect);
                        inner.fullscreen_screens = screen_frames();
                    }
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
                    let mask = NSWindowStyleMask::NSBorderlessWindowMask;
                    if is_hotkey_panel(*self.window) {
                        apply_decorations_to_window(
                            &self.window,
                            WindowDecorations::NONE,
                            self.config.integrated_title_button_style,
                            false,
                        );
                    } else {
                        set_window_style_mask_preserving_responder(&self.window, mask);
                    }
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
        let is_opaque = if opaque { YES } else { NO };
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            window_view.opaque.set(is_opaque);
        }
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
                // Keep the NSWindow itself clear so the body stays see-through,
                // but tint the layer backing with the theme background at the
                // window opacity. macOS 26 exposes a sliver of layer backing at
                // the rounded top corners that the GPU surface does not cover
                // (visible on the side without the traffic-light buttons); a
                // fully clear backing renders that sliver black, so fill it with
                // the same semi-transparent background the body uses.
                let bg = self
                    .config
                    .resolved_palette
                    .background
                    .unwrap_or(RgbaColor::from(SrgbaTuple(0., 0., 0., 1.0)));
                let a = self.config.window_background_opacity as f64;
                let clear: id = msg_send![class!(NSColor), clearColor];
                let _: () = msg_send![*self.window, setBackgroundColor: clear];
                objc2_core_graphics::CGColor::new_srgb(bg.0.into(), bg.1.into(), bg.2.into(), a)
            };

            // Match our Metal layer's corner radius to the window frame's corner
            // radius so compositor-clipped corners don't leave transparent arcs.
            // On macOS 26+ NSThemeFrame.layer.cornerRadius returns 0 because
            // rounding is handled by the compositor, so fall back to 10pt.
            // MACOS_FORCE_SQUARE_CORNERS opts out entirely: required on macOS 26
            // where the compositor leaves the NSWindow occupancy rectangular, so
            // tiled neighbor apps would otherwise poke into the clipped arcs.
            // The full-width drawer must not add rounding at the screen edges.
            let force_square = is_hotkey_panel(*self.window)
                || self
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
                    // macOS 26 clips window corners at the compositor level and
                    // reports cornerRadius == 0 on the frame layer. Rounding the
                    // Metal layer ourselves with a guessed radius that does not
                    // match the compositor leaves a crescent at each corner that
                    // exposes the window backing -- a black block under a
                    // transparent window. Only round the layer when opaque (the
                    // gap is filled with the theme color there anyway); for
                    // transparent windows keep radius 0 so the layer fills the
                    // full frame and the compositor owns the rounding.
                    if !force_square
                        && corner_radius == 0.0
                        && macos_version_major() >= 26
                        && is_opaque == YES
                    {
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
                    return false;
                }
                let previous_screens =
                    std::mem::replace(&mut inner.display_screen_frames, screen_frames());
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
                let fills_visible_frame = window_view
                    .windowed_screen
                    .get()
                    .fills_after_topology_change(
                        inner
                            .last_reported_window_state
                            .contains(WindowState::MAXIMIZED),
                    );
                Connection::with_window_inner(window_id, move |inner| {
                    inner.cancel_hotkey_slide();
                    // Read the live mode: the window may have entered fullscreen
                    // while this refresh was queued. AppKit owns native fullscreen.
                    if !inner.is_native_fullscreen() {
                        // Reapply window decorations so the title bar is
                        // restored after a monitor disconnection.
                        inner.apply_decorations();

                        // Display topology changes can leave a window with a
                        // frame from the old screen. Clamp both origin and size
                        // to the new visible frame so the title bar and content
                        // stay reachable.
                        unsafe {
                            let frame = NSWindow::frame(*inner.window);
                            if let Some(screen) = screen_geometry_for_window(*inner.window) {
                                let simple_fullscreen = inner.is_fullscreen();
                                let hotkey_panel = is_hotkey_panel(*inner.window);
                                let adjusted = if hotkey_panel {
                                    hotkey_panel_frame(
                                        screen,
                                        inner.config.macos_global_hotkey_height_percent,
                                    )
                                } else {
                                    frame_after_display_topology_change(
                                        frame,
                                        simple_fullscreen,
                                        fills_visible_frame,
                                        &previous_screens,
                                        &screen_frames(),
                                        screen,
                                    )
                                };
                                if !nsrect_approx_eq(frame, adjusted, 0.5) {
                                    inner.window.setFrame_display_(adjusted, YES);
                                }
                                if !simple_fullscreen && !hotkey_panel {
                                    if let Some(view) = WindowView::get_this(&**inner.view) {
                                        view.windowed_screen.set(
                                            view.windowed_screen.get().settled(
                                                NSWindow::frame(*inner.window),
                                                screen_identifier_for_window(*inner.window),
                                                screen,
                                            ),
                                        );
                                    }
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
        if self.dedicated_hotkey {
            self.show_global_hotkey_window();
            return;
        }
        self.restore_ordinary_window();
        unsafe {
            let current_app = NSRunningApplication::currentApplication(nil);
            current_app.activateWithOptions_(NSApplicationActivateIgnoringOtherApps);

            // Stupid hack: adjust the window style mask and set it back
            // to what it was.
            // Without this, the CAMetalLayer used by webgpu seems to get
            // stuck with a scale factor of 2 despite us having configured 1.
            set_window_style_mask_preserving_responder(
                &self.window,
                NSWindowStyleMask::NSBorderlessWindowMask,
            );

            apply_decorations_to_window(
                &self.window,
                self.config.window_decorations,
                self.config.integrated_title_button_style,
                self.config.native_macos_fullscreen_mode,
            );

            self.update_titlebar_background();
            apply_window_appearance(&self.window, &self.config);

            make_terminal_key_window(&self.window, &self.view);
        }
        self.dispatch_resize_event();
    }

    fn close(&mut self) {
        self.clear_pending_hotkey_visibility();
        self.cancel_hotkey_slide();
        unsafe {
            if let Some(window) = self.ordinary_window.take() {
                window.close();
            }
            self.window.close();
        }
    }

    pub(crate) fn focus(&mut self) {
        if self.dedicated_hotkey {
            self.show_global_hotkey_window();
            return;
        }
        self.restore_ordinary_window();
        make_terminal_key_window(&self.window, &self.view);
    }

    pub(crate) fn is_dedicated_hotkey(&self) -> bool {
        self.dedicated_hotkey
    }

    pub(crate) fn ordinary_global_hotkey_should_hide(&self) -> bool {
        let pending = WindowView::get_this(unsafe { &**self.view }).and_then(|view| {
            view.pending_hotkey_visibility
                .get()
                .or_else(|| view.order_out_on_fullscreen_exit.get().then_some(false))
        });
        hotkey_should_hide(pending, self.is_key_window(), self.is_visible())
    }

    pub(crate) fn show_ordinary_global_hotkey_window(&mut self) {
        if self.dedicated_hotkey {
            return;
        }
        if self.defer_hotkey_visibility(true) {
            return;
        }
        // Cancel a pending fullscreen-exit hide without installing a panel or
        // changing the ordinary window's frame, style, or terminal session.
        if let Some(view) = WindowView::get_this(unsafe { &**self.view }) {
            view.order_out_on_fullscreen_exit.set(false);
        }
        unsafe {
            let hidden: BOOL = msg_send![NSApp(), isHidden];
            if hidden == YES {
                let _: () = msg_send![NSApp(), unhide: NSApp()];
            }
            NSRunningApplication::currentApplication(nil)
                .activateWithOptions_(NSApplicationActivateIgnoringOtherApps);
        }
        self.focus();
    }

    pub(crate) fn hide_ordinary_global_hotkey_window(&mut self) {
        if !self.dedicated_hotkey && !self.defer_hotkey_visibility(false) {
            self.order_out_after_fullscreen_exit();
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

    /// Dispatch a KeyAssignment to this window's event stream as if the user
    /// had pressed its key binding. Used by AppKit-level paths (Dock quit,
    /// applicationShouldTerminate:) that need to defer to the GUI layer's
    /// mux-aware handling.
    pub(crate) fn dispatch_key_assignment(
        &self,
        action: config::keyassignment::KeyAssignment,
    ) -> bool {
        let view = unsafe { &**self.view };
        let Some(window_view) = WindowView::get_this(view) else {
            return false;
        };
        let Ok(inner) = window_view.inner.try_borrow() else {
            return false;
        };
        let events = inner.events.clone();
        drop(inner);
        events.dispatch(WindowEvent::PerformKeyAssignment(action));
        true
    }

    fn prepare_global_hotkey_panel(&mut self) {
        unsafe {
            if self.ordinary_window.is_none() {
                self.ordinary_window_screens = screen_frames();
                let frame = NSWindow::frame(*self.window);
                let style = hotkey_panel_style_mask(NSWindow::styleMask(*self.window));
                let panel: id = msg_send![get_hotkey_window_class(), alloc];
                let panel = StrongPtr::new(NSWindow::initWithContentRect_styleMask_backing_defer_(
                    panel,
                    frame,
                    style,
                    NSBackingStoreBuffered,
                    NO,
                ));
                panel.setReleasedWhenClosed_(NO);
                let _: () = msg_send![*panel, setExcludedFromWindowsMenu: YES];
                let _: () = msg_send![*panel, setRestorable: NO];
                let _: () = msg_send![*panel, setTabbingMode: 2i64];
                let _: () = msg_send![*panel, setFloatingPanel: YES];
                let _: () = msg_send![*panel, setHidesOnDeactivate: NO];
                panel.setAcceptsMouseMovedEvents_(YES);
                panel.setTitle_(self.window.title());
                panel.setBackgroundColor_(self.window.backgroundColor());
                panel.setColorSpace_(NSWindow::colorSpace(*self.window));
                let opaque: BOOL = msg_send![*self.window, isOpaque];
                panel.setOpaque_(opaque);
                panel.setHasShadow_(self.window.hasShadow());
                let min_size: NSSize = msg_send![*self.window, contentMinSize];
                let increments: NSSize = msg_send![*self.window, resizeIncrements];
                let _: () = msg_send![*panel, setContentMinSize: min_size];
                panel.setResizeIncrements_(increments);
                let _: () = msg_send![*panel, registerForDraggedTypes:
                    NSArray::arrayWithObject(nil, appkit::NSFilenamesPboardType)];
                panel.setLevel_(hotkey_window_level());
                panel.setCollectionBehavior_(hotkey_collection_behavior(
                    self.window.collectionBehavior(),
                    macos_version_major(),
                ));
                self.window.setDelegate_(nil);
                self.window.setContentView_(nil);
                let _: () = msg_send![*self.window, orderOut: nil];
                let ordinary = std::mem::replace(&mut self.window, panel);
                self.ordinary_window = Some(ordinary);
                if let Some(view) = WindowView::get_this(&**self.view) {
                    view.inner.borrow_mut().window = Some(self.window.weak());
                }
                self.window.setContentView_(*self.view);
                self.window.setDelegate_(*self.view);
                if self.is_fullscreen() {
                    apply_decorations_to_window(
                        &self.window,
                        WindowDecorations::NONE,
                        self.config.integrated_title_button_style,
                        false,
                    );
                } else {
                    self.apply_decorations();
                }
                self.update_window_shadow();
                self.update_titlebar_background();
                self.update_window_background_blur();
                apply_window_appearance(&self.window, &self.config);
                self.window.setFrame_display_(frame, NO);
            }
        }
    }

    /// A nonactivating panel can take keyboard focus without activating Kaku
    /// or leaving the foreground app's fullscreen Space. Keep the same NSView
    /// (and Metal surface) when installing the terminal's native panel host.
    pub(crate) fn show_global_hotkey_window(&mut self) {
        if self.defer_hotkey_visibility(true) {
            return;
        }
        self.prepare_global_hotkey_panel();
        unsafe {
            if let Some(geometry) =
                hotkey_screen_geometry(self.config.macos_global_hotkey_follow_mouse)
            {
                let frame =
                    hotkey_panel_frame(geometry, self.config.macos_global_hotkey_height_percent);
                self.animate_hotkey_slide(frame, geometry.frame, true);
            }
            // Cmd+H may have hidden the app independently of the hotkey.
            let hidden: BOOL = msg_send![NSApp(), isHidden];
            if hidden == YES {
                let _: () = msg_send![NSApp(), unhideWithoutActivation];
            }
            // Only the panel takes keyboard focus; do not activate the app.
            self.window.makeKeyAndOrderFront_(nil);
            self.window.makeFirstResponder_(*self.view);
        }
        self.dispatch_resize_event();
    }

    fn restore_ordinary_window(&mut self) {
        if self.dedicated_hotkey {
            return;
        }
        self.clear_pending_hotkey_visibility();
        self.cancel_hotkey_slide();
        let Some(ordinary) = self.ordinary_window.take() else {
            return;
        };
        let previous_screens = std::mem::take(&mut self.ordinary_window_screens);
        unsafe {
            ordinary.setTitle_(self.window.title());
            self.window.setDelegate_(nil);
            self.window.setContentView_(nil);
            self.window.close();
            self.window = ordinary;
            if let Some(view) = WindowView::get_this(&**self.view) {
                view.inner.borrow_mut().window = Some(self.window.weak());
            }
            self.window.setContentView_(*self.view);
            self.window.setDelegate_(*self.view);
            self.apply_decorations();
            self.update_window_shadow();
            self.update_window_background_blur();
            self.update_titlebar_background();
            apply_window_appearance(&self.window, &self.config);
            if !self.is_native_fullscreen() {
                // The drawer's geometry must not replace the parked ordinary window.
                let ordinary_frame = NSWindow::frame(*self.window);
                if let Some(screen) = screen_geometry_for_window(*self.window) {
                    let fills_visible_frame = WindowView::get_this(&**self.view)
                        .map(|view| view.windowed_screen.get().fills_visible_frame)
                        .unwrap_or(false);
                    let adjusted = frame_after_display_topology_change(
                        ordinary_frame,
                        self.is_fullscreen(),
                        fills_visible_frame,
                        &previous_screens,
                        &screen_frames(),
                        screen,
                    );
                    self.window.setFrame_display_(adjusted, NO);
                }
            }
        }
        self.dispatch_resize_event();
    }

    pub(crate) fn is_visible(&self) -> bool {
        unsafe {
            let visible: BOOL = msg_send![*self.window, isVisible];
            visible == YES
        }
    }

    pub(crate) fn global_hotkey_should_hide(&self) -> bool {
        let showing = WindowView::get_this(unsafe { &**self.view }).and_then(|view| {
            view.pending_hotkey_visibility
                .get()
                .or_else(|| view.hotkey_slide.get().map(|slide| slide.showing))
        });
        drawer_hotkey_should_hide(
            showing,
            self.is_key_window(),
            self.is_visible(),
            self.config.macos_global_hotkey_hide_on_focus_loss,
        )
    }

    pub(crate) fn hide_global_hotkey_window(&mut self) {
        if self.defer_hotkey_visibility(false) {
            return;
        }
        if self.ordinary_window.is_some() {
            if let Some(screen) = screen_geometry_for_window(*self.window) {
                let resting_frame = WindowView::get_this(unsafe { &**self.view })
                    .and_then(|view| view.hotkey_slide.get().map(|slide| slide.resting_frame))
                    .unwrap_or_else(|| unsafe { NSWindow::frame(*self.window) });
                self.animate_hotkey_slide(resting_frame, screen.frame, false);
                return;
            }
        }
        if self.is_native_fullscreen() {
            // Keep the desired state while AppKit exits, so a second hotkey
            // reverses this hide rather than asking to hide again.
            if let Some(view) = WindowView::get_this(unsafe { &**self.view }) {
                view.pending_hotkey_visibility.set(Some(false));
            }
        }
        self.order_out_after_fullscreen_exit();
    }

    fn defer_hotkey_visibility(&self, showing: bool) -> bool {
        let Some(view) = WindowView::get_this(unsafe { &**self.view }) else {
            return false;
        };
        if defer_hotkey_request(
            &view.pending_hotkey_visibility,
            view.native_fullscreen_transition_active.get(),
            showing,
        ) {
            // A reveal cancels the hide attached to the in-flight exit.
            if showing {
                view.order_out_on_fullscreen_exit.set(false);
            }
            log::debug!(
                "deferring hotkey visibility={} until native fullscreen completes",
                showing
            );
            true
        } else {
            false
        }
    }

    fn clear_pending_hotkey_visibility(&self) {
        if let Some(view) = WindowView::get_this(unsafe { &**self.view }) {
            cancel_hotkey_visibility_intent(
                &view.pending_hotkey_visibility,
                &view.order_out_on_fullscreen_exit,
            );
        }
    }

    fn order_out_after_fullscreen_exit(&mut self) {
        // Never strand a black native-fullscreen Space with a direct orderOut.
        if self.is_native_fullscreen() {
            if let Some(view) = WindowView::get_this(unsafe { &**self.view }) {
                view.order_out_on_fullscreen_exit.set(true);
                if !should_start_native_fullscreen_exit(
                    view.native_fullscreen_transition_active.get(),
                    view.native_fullscreen_target.get(),
                ) {
                    return;
                }
            }
            self.exit_native_fullscreen();
        } else {
            self.order_out();
        }
    }

    fn animate_hotkey_slide(&mut self, resting_frame: NSRect, screen: NSRect, showing: bool) {
        let Some(view) = WindowView::get_this(unsafe { &**self.view }) else {
            return;
        };
        let generation = view.hotkey_slide_generation.get().wrapping_add(1);
        view.hotkey_slide_generation.set(generation);
        let offscreen = hotkey_offscreen_origin(resting_frame, screen);
        let from = if showing && !self.is_visible() {
            offscreen
        } else {
            unsafe { NSWindow::frame(*self.window).origin }
        };
        let reduce_motion: BOOL = unsafe {
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            msg_send![workspace, accessibilityDisplayShouldReduceMotion]
        };
        let slide = HotkeySlide {
            resting_frame,
            from,
            to: if showing {
                resting_frame.origin
            } else {
                offscreen
            },
            started: Instant::now(),
            duration: hotkey_animation_duration(
                self.config.macos_global_hotkey_animation_duration_ms,
                reduce_motion == YES,
            ),
            showing,
            hide_on_focus_loss: false,
        };
        view.hotkey_slide.set(Some(slide));
        unsafe {
            if slide.duration.is_zero() {
                self.cancel_hotkey_slide();
                return;
            }
            self.window
                .setFrame_display_(NSRect::new(from, resting_frame.size), NO);
        }
        let window_id = view.window_id.get();
        // Timed main-thread steps allow a new hotkey to reverse immediately;
        // AppKit's synchronous setFrame:display:animate: blocks hotkey handling.
        promise::spawn::spawn(async move {
            loop {
                async_io::Timer::after(Duration::from_millis(8)).await;
                let running = Connection::with_window_inner(window_id, move |inner| {
                    Ok(inner.step_hotkey_slide(generation))
                })
                .await;
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn step_hotkey_slide(&mut self, generation: u64) -> bool {
        let Some(view) = WindowView::get_this(unsafe { &**self.view }) else {
            return false;
        };
        if view.hotkey_slide_generation.get() != generation || view.is_closing.get() {
            return false;
        }
        let Some(mut slide) = view.hotkey_slide.get() else {
            return false;
        };
        if slide.hide_on_focus_loss {
            let current_origin = unsafe { NSWindow::frame(*self.window).origin };
            if let Some(reverse) = slide.reverse_on_focus_gain(self.is_key_window(), current_origin)
            {
                log::debug!("global hotkey panel regained keyboard focus; sliding back in");
                slide = reverse;
                view.hotkey_slide.set(Some(slide));
            }
        }
        let elapsed = slide.started.elapsed();
        if elapsed >= slide.duration {
            self.cancel_hotkey_slide();
            return false;
        }
        unsafe {
            let _: () = msg_send![*self.window, setFrameOrigin: slide.origin_at(elapsed)];
        }
        true
    }

    fn cancel_hotkey_slide(&mut self) {
        if let Some(view) = WindowView::get_this(unsafe { &**self.view }) {
            view.cancel_hotkey_slide(*self.window);
        }
    }

    fn hide(&mut self) {
        self.clear_pending_hotkey_visibility();
        self.cancel_hotkey_slide();
        unsafe {
            NSWindow::miniaturize_(*self.window, *self.window);
            // We could literally set it invisible like this, but
            // then there is no UI to make it visible again later.
            //let () = msg_send![*self.window, setIsVisible: NO];
        }
    }

    /// Remove the window from screen without changing fullscreen state.
    /// Native fullscreen callers must use order_out_after_fullscreen_exit()
    /// instead, so AppKit can tear down the Space before hiding the window.
    pub(crate) fn order_out(&mut self) {
        self.cancel_hotkey_slide();
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
            let level = if self.dedicated_hotkey {
                hotkey_window_level()
            } else {
                window_level_to_nswindow_level(level)
            };
            NSWindow::setLevel_(*self.window, level);
        }
        // Dispatch a resize event with the updated window state
        self.dispatch_resize_event();
    }

    fn set_inner_size(&mut self, width: usize, height: usize) {
        unsafe {
            let frame = NSView::frame(*self.view as *mut _);
            let backing_frame = NSView::convertRectToBacking(*self.view as *mut _, frame);
            let scale = backing_frame.size.width / frame.size.width;
            let size = NSSize::new(width as f64 / scale, height as f64 / scale);
            // Same-size render housekeeping must not interrupt the reveal.
            if content_size_changed(frame.size, size) {
                self.cancel_hotkey_slide();
            }

            NSWindow::setContentSize_(*self.window, size);

            // setContentSize_ doesn't explicitly invalidate,
            // so we need to do it ourselves
            self.invalidate();
        }
    }

    fn set_window_position(&mut self, coords: ScreenPoint) {
        self.cancel_hotkey_slide();
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
        self.cancel_hotkey_slide();
        // Always call zoom when explicitly requested by the user.
        // NSWindow::isZoomed can falsely return YES after screen changes,
        // which would cause the zoom call to be skipped even though the
        // window is not actually maximized.
        // <https://github.com/tw93/Kaku/issues/131>
        self.arm_transition_content_hide(ZOOM_HIDE_CONTENT_MS, "zoom_maximize", false);
        unsafe {
            NSWindow::zoom_(*self.window, nil);
        }
        self.revalidate_windowed_zoom_intent();
    }

    fn restore(&mut self) {
        self.cancel_hotkey_slide();
        if self.is_zoomed() {
            self.arm_transition_content_hide(ZOOM_HIDE_CONTENT_MS, "zoom_restore", false);
            unsafe {
                NSWindow::zoom_(*self.window, nil);
            }
            self.revalidate_windowed_zoom_intent();
        }
    }

    fn revalidate_windowed_zoom_intent(&self) {
        if let Some(view) = WindowView::get_this(unsafe { &**self.view }) {
            let mut state = view.windowed_screen.get();
            state.max_invalidated_by_move = false;
            view.windowed_screen.set(state);
        }
    }

    /// Center on the window's own screen, working entirely in that screen's
    /// point coordinates. Keep this native rather than routing through
    /// `Connection::screens()` plus `set_window_position`: the single-screen
    /// point math needs no cross-space conversion at all.
    fn center(&mut self) {
        self.cancel_hotkey_slide();
        unsafe {
            let screen: id = msg_send![*self.window, screen];
            let screen = if screen.is_null() {
                NSScreen::mainScreen(nil)
            } else {
                screen
            };
            if screen.is_null() {
                return;
            }
            let visible: NSRect = msg_send![screen, visibleFrame];
            let frame = NSWindow::frame(*self.window);
            let origin = centered_frame_origin(visible, frame.size);
            NSWindow::setFrameOrigin_(*self.window, origin);
        }
    }

    fn toggle_fullscreen(&mut self) {
        if self.dedicated_hotkey {
            return;
        }
        if self.ordinary_window.is_some() {
            self.show();
        }
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
        let panel_layout_changed = is_hotkey_panel(*self.window)
            && (self.config.macos_global_hotkey_height_percent
                != config.macos_global_hotkey_height_percent
                || hotkey_extend_behind_notch(
                    true,
                    self.config.macos_global_hotkey_extend_behind_notch,
                    self.config.macos_fullscreen_extend_behind_notch,
                ) != hotkey_extend_behind_notch(
                    true,
                    config.macos_global_hotkey_extend_behind_notch,
                    config.macos_fullscreen_extend_behind_notch,
                ));

        self.config = config.clone();
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            {
                let mut inner = window_view.inner.borrow_mut();
                inner.config = config.clone();
                if dpi_changed || panel_layout_changed {
                    inner.screen_changed = true;
                }
            }
            if let Some(reverse) = window_view.hotkey_slide.get().and_then(|slide| {
                slide.reverse_when_auto_hide_disabled(
                    config.macos_global_hotkey_hide_on_focus_loss,
                    unsafe { NSWindow::frame(*self.window).origin },
                )
            }) {
                window_view.hotkey_slide.set(Some(reverse));
            }
        }
        self.update_window_shadow();
        self.update_window_background_blur();
        self.update_titlebar_background();
        apply_window_appearance(&self.window, &self.config);
        self.apply_decorations();
        if panel_layout_changed {
            self.cancel_hotkey_slide();
            if let Some(screen) = screen_geometry_for_window(*self.window) {
                let frame = hotkey_panel_frame(screen, config.macos_global_hotkey_height_percent);
                unsafe {
                    self.window.setFrame_display_(frame, NO);
                }
            }
            self.dispatch_resize_event();
        }
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

fn fullscreen_allows_tiling_behavior() -> appkit::NSWindowCollectionBehavior {
    unsafe {
        appkit::NSWindowCollectionBehavior::from_bits_unchecked(
            NS_WINDOW_COLLECTION_BEHAVIOR_FULLSCREEN_ALLOWS_TILING_BITS,
        )
    }
}

fn fullscreen_disallows_tiling_behavior() -> appkit::NSWindowCollectionBehavior {
    unsafe {
        appkit::NSWindowCollectionBehavior::from_bits_unchecked(
            NS_WINDOW_COLLECTION_BEHAVIOR_FULLSCREEN_DISALLOWS_TILING_BITS,
        )
    }
}

fn native_macos_fullscreen_collection_behavior(
    mut behavior: appkit::NSWindowCollectionBehavior,
    native_macos_fullscreen_mode: bool,
) -> appkit::NSWindowCollectionBehavior {
    let primary = appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary;
    let auxiliary =
        appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary;
    let allows_tiling = fullscreen_allows_tiling_behavior();
    let disallows_tiling = fullscreen_disallows_tiling_behavior();

    if native_macos_fullscreen_mode {
        behavior.remove(auxiliary | disallows_tiling);
        behavior.insert(primary | allows_tiling);
    } else {
        behavior.remove(primary | auxiliary | allows_tiling | disallows_tiling);
    }

    behavior
}

fn apply_decorations_to_window(
    window: &StrongPtr,
    decorations: WindowDecorations,
    integrated_title_button_style: IntegratedTitleButtonStyle,
    native_macos_fullscreen_mode: bool,
) {
    let mask = decoration_to_mask(decorations, integrated_title_button_style);
    let decorations = if is_hotkey_panel(**window) {
        decorations & WindowDecorations::RESIZE
    } else {
        effective_decorations(decorations, integrated_title_button_style)
    };
    unsafe {
        if is_hotkey_panel(**window) {
            window.setStyleMask_(hotkey_panel_style_mask(mask));
            window.setCollectionBehavior_(hotkey_collection_behavior(
                window.collectionBehavior(),
                macos_version_major(),
            ));
        } else {
            set_window_style_mask_preserving_responder(window, mask);
            window.setCollectionBehavior_(native_macos_fullscreen_collection_behavior(
                window.collectionBehavior(),
                native_macos_fullscreen_mode,
            ));
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
            set_titlebar_button_hidden(button, hidden);
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
    /// Preserve cross-display window geometry unless a saved display disappears.
    fullscreen_screens: Vec<NSRect>,
    display_screen_frames: Vec<NSRect>,

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
        let glium_context = GlContextPair::create(*view, self.config.window_background_opacity)?;

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
    // Per-view cache avoids reborrowing Inner from AppKit's isOpaque callback.
    opaque: Cell<BOOL>,
    /// Window ID stored outside RefCell so window_will_close can access it even
    /// when a borrow is already held by a caller higher on the stack.
    window_id: Cell<usize>,
    /// Move delegates can run while Inner is borrowed by set_window_position.
    windowed_screen: Cell<WindowedScreenState>,
    hotkey_slide: Cell<Option<HotkeySlide>>,
    hotkey_slide_generation: Cell<u64>,
    pending_hotkey_visibility: Cell<Option<bool>>,
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
    deferred_screen_resize: DeferredScreenResize,
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

    fn screen_resize(width: usize, height: usize, dpi: usize, state: WindowState) -> ScreenResize {
        ScreenResize {
            dimensions: Dimensions {
                pixel_width: width,
                pixel_height: height,
                dpi,
            },
            window_state: state,
            live_resizing: false,
        }
    }

    #[test]
    fn deferred_screen_resize_keeps_recovered_dimensions_and_state() {
        let pending = DeferredScreenResize::default();
        assert!(pending.update(screen_resize(1538, 1148, 144, WindowState::empty())));
        assert!(!pending.update(screen_resize(3456, 2104, 144, WindowState::MAXIMIZED)));
        let latest = pending.take().unwrap();
        assert_eq!(latest.dimensions.pixel_width, 3456);
        assert_eq!(latest.dimensions.pixel_height, 2104);
        assert_eq!(latest.window_state, WindowState::MAXIMIZED);
        assert!(!pending.is_pending());
        assert!(pending.take().is_none());
    }

    #[test]
    fn deferred_screen_resize_keeps_latest_dpi_and_live_resize_end() {
        let pending = DeferredScreenResize::default();
        let mut initial = screen_resize(3840, 720, 144, WindowState::HOTKEY_WINDOW);
        initial.live_resizing = true;
        assert!(pending.update(initial));
        assert!(!pending.update(screen_resize(3456, 746, 192, WindowState::HOTKEY_WINDOW)));
        let latest = pending.take().unwrap();
        assert_eq!(latest.dimensions.pixel_width, 3456);
        assert_eq!(latest.dimensions.pixel_height, 746);
        assert_eq!(latest.dimensions.dpi, 192);
        assert_eq!(latest.window_state, WindowState::HOTKEY_WINDOW);
        assert!(!latest.live_resizing);
    }

    #[test]
    fn deferred_screen_resize_is_one_shot_and_per_window() {
        let ordinary = DeferredScreenResize::default();
        let hotkey = DeferredScreenResize::default();
        assert!(!ordinary.is_pending());
        assert!(ordinary.update(screen_resize(100, 200, 96, WindowState::empty())));
        assert!(hotkey.update(screen_resize(300, 400, 144, WindowState::HOTKEY_WINDOW)));
        assert_eq!(ordinary.take().unwrap().dimensions.pixel_width, 100);
        assert!(ordinary.take().is_none());
        assert!(hotkey.is_pending());
        assert_eq!(hotkey.take().unwrap().dimensions.pixel_width, 300);
        assert!(ordinary.update(screen_resize(500, 600, 192, WindowState::FULL_SCREEN)));
        assert_eq!(
            ordinary.take().unwrap().window_state,
            WindowState::FULL_SCREEN
        );
    }

    #[test]
    fn deferred_screen_resize_same_size_keeps_latest_state() {
        let pending = DeferredScreenResize::default();
        assert!(pending.update(screen_resize(800, 600, 96, WindowState::empty())));
        assert!(!pending.update(screen_resize(800, 600, 96, WindowState::FULL_SCREEN)));
        assert_eq!(
            pending.take().unwrap().window_state,
            WindowState::FULL_SCREEN
        );
    }

    #[test]
    fn deferred_screen_resize_discarded_transition_can_reschedule() {
        let pending = DeferredScreenResize::default();
        assert!(pending.update(screen_resize(800, 600, 96, WindowState::empty())));
        assert!(pending.take().is_some());
        assert!(pending.take().is_none());
        assert!(pending.update(screen_resize(1600, 900, 144, WindowState::FULL_SCREEN)));
        assert_eq!(pending.take().unwrap().dimensions.pixel_width, 1600);
        assert!(pending.take().is_none());
    }

    #[test]
    fn fullscreen_intent_stable_hide_is_cancelled_by_new_ordinary_focus() {
        let pending = Cell::new(None);
        let exit_hide = Cell::new(false);
        assert!(!defer_hotkey_request(&pending, false, false));
        exit_hide.set(true);
        assert_eq!(pending.get(), None);
        assert!(exit_hide.get());
        cancel_hotkey_visibility_intent(&pending, &exit_hide);
        assert_eq!(pending.get(), None);
        assert!(!exit_hide.get());
    }

    #[test]
    fn fullscreen_intent_cancel_clears_both_cells_and_is_idempotent() {
        for requested in [None, Some(false), Some(true)] {
            for hide in [false, true] {
                let pending = Cell::new(requested);
                let exit_hide = Cell::new(hide);
                for _ in 0..2 {
                    cancel_hotkey_visibility_intent(&pending, &exit_hide);
                    assert_eq!(pending.get(), None);
                    assert!(
                        !exit_hide.get(),
                        "pending={:?}, exit_hide={}",
                        requested,
                        hide
                    );
                }
            }
        }
    }

    #[test]
    fn fullscreen_intent_cancel_is_local_to_the_focused_window() {
        let first_pending = Cell::new(None);
        let first_hide = Cell::new(true);
        let second_pending = Cell::new(Some(false));
        let second_hide = Cell::new(true);
        cancel_hotkey_visibility_intent(&first_pending, &first_hide);
        assert_eq!(first_pending.get(), None);
        assert!(!first_hide.get());
        assert_eq!(second_pending.get(), Some(false));
        assert!(second_hide.get());
        cancel_hotkey_visibility_intent(&second_pending, &second_hide);
        assert_eq!(second_pending.get(), None);
        assert!(!second_hide.get());
    }

    #[test]
    fn fullscreen_intent_exit_consumes_only_an_uncancelled_or_newer_hide() {
        let pending = Cell::new(None);
        let exit_hide = Cell::new(true);
        // Match did_exit_fullscreen's one-shot consumption, with a positive hide case.
        assert!(exit_hide.replace(false));
        assert!(!exit_hide.replace(false));
        exit_hide.set(true);
        cancel_hotkey_visibility_intent(&pending, &exit_hide);
        assert!(!exit_hide.replace(false));
        // A later explicit hide must still be able to re-arm the exit marker.
        assert!(!defer_hotkey_request(&pending, false, false));
        exit_hide.set(true);
        assert!(exit_hide.replace(false));
        assert!(!exit_hide.replace(false));
        assert_eq!(pending.get(), None);
    }

    #[test]
    fn fullscreen_intent_cancellation_and_native_exit_wiring() {
        let source = include_str!("window.rs");
        let ordinary = &source[source.find("impl WindowInner {").unwrap()..];
        for signature in ["fn show(&mut self)", "pub(crate) fn focus(&mut self)"] {
            let method = production_method(ordinary, signature);
            assert!(
                method.find("if self.dedicated_hotkey").unwrap()
                    < method.find("self.show_global_hotkey_window()").unwrap()
            );
            assert!(
                method.find("return;").unwrap()
                    < method.find("self.restore_ordinary_window()").unwrap()
            );
            assert!(
                method.find("self.restore_ordinary_window()").unwrap()
                    < method.find("make_terminal_key_window").unwrap()
            );
        }
        let restore = production_method(source, "fn restore_ordinary_window(");
        assert!(
            restore
                .find("self.clear_pending_hotkey_visibility()")
                .unwrap()
                < restore.find("self.ordinary_window.take()").unwrap()
        );
        let clear = production_method(source, "fn clear_pending_hotkey_visibility(");
        assert!(clear.contains("cancel_hotkey_visibility_intent("));
        assert!(clear.contains("&view.pending_hotkey_visibility"));
        assert!(clear.contains("&view.order_out_on_fullscreen_exit"));
        let mark = production_method(source, "fn order_out_after_fullscreen_exit(");
        assert!(
            mark.find("view.order_out_on_fullscreen_exit.set(true)")
                .unwrap()
                < mark.find("self.exit_native_fullscreen()").unwrap()
        );
        let consume = production_method(source, "extern \"C\" fn did_exit_fullscreen(");
        assert!(consume.contains("if this.order_out_on_fullscreen_exit.replace(false)"));
        assert!(
            consume
                .find("order_out_on_fullscreen_exit.replace(false)")
                .unwrap()
                < consume.find("orderOut: nil").unwrap()
        );
        assert!(
            consume.find("orderOut: nil").unwrap()
                < consume
                    .find("this.dispatch_pending_hotkey_visibility()")
                    .unwrap()
        );
    }

    fn ordinary_input_host_fixture() -> StrongPtr {
        static CLASS: std::sync::OnceLock<&'static Class> = std::sync::OnceLock::new();
        let cls = CLASS.get_or_init(|| {
            let mut cls = ClassDecl::new("KakuOrdinaryInputFixture", class!(NSObject)).unwrap();
            cls.add_ivar::<NSUInteger>("focusStep");
            cls.add_ivar::<NSUInteger>("responderStep");
            cls.add_ivar::<id>("responder");
            cls.add_ivar::<NSUInteger>("styleMask");
            extern "C" fn first_responder(this: &Object, _: Sel) -> id {
                unsafe { *this.get_ivar("responder") }
            }
            extern "C" fn set_style(this: &mut Object, _: Sel, mask: NSUInteger) {
                let host = this as *mut Object;
                unsafe {
                    this.set_ivar("styleMask", mask);
                    // AppKit replaces the responder with the host when changing style.
                    this.set_ivar("responder", host);
                }
            }
            extern "C" fn order_front(this: &mut Object, _: Sel, _: id) {
                unsafe {
                    let step: NSUInteger = *this.get_ivar("focusStep");
                    this.set_ivar("focusStep", step + 1);
                }
            }
            extern "C" fn make_responder(this: &mut Object, _: Sel, view: id) -> BOOL {
                unsafe {
                    let step: NSUInteger = *this.get_ivar("focusStep");
                    this.set_ivar("responderStep", step);
                    this.set_ivar("focusStep", step + 1);
                    this.set_ivar("responder", view);
                }
                YES
            }
            unsafe {
                cls.add_method(
                    sel!(firstResponder),
                    first_responder as extern "C" fn(&Object, Sel) -> id,
                );
                cls.add_method(
                    sel!(setStyleMask:),
                    set_style as extern "C" fn(&mut Object, Sel, NSUInteger),
                );
                cls.add_method(
                    sel!(makeKeyAndOrderFront:),
                    order_front as extern "C" fn(&mut Object, Sel, id),
                );
                cls.add_method(
                    sel!(makeFirstResponder:),
                    make_responder as extern "C" fn(&mut Object, Sel, id) -> BOOL,
                );
            }
            cls.register()
        });
        unsafe {
            let host: id = msg_send![*cls, new];
            (*host).set_ivar("focusStep", 0 as NSUInteger);
            (*host).set_ivar("responderStep", 0 as NSUInteger);
            // Reproduce the native probe: the ordinary host starts as its own responder.
            (*host).set_ivar("responder", host);
            StrongPtr::new(host)
        }
    }

    #[test]
    fn ordinary_style_change_preserves_input_on_fullscreen_enter_and_exit() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let host = ordinary_input_host_fixture();
        let view = unsafe { StrongPtr::new(msg_send![class!(NSObject), new]) };
        unsafe { (**host).set_ivar("responder", *view) };
        for mask in [
            NSWindowStyleMask::NSBorderlessWindowMask,
            NSWindowStyleMask::NSTitledWindowMask,
        ] {
            set_window_style_mask_preserving_responder(&host, mask);
            assert_eq!(unsafe { *(**host).get_ivar::<id>("responder") }, *view);
            assert_eq!(
                unsafe { *(**host).get_ivar::<NSUInteger>("styleMask") },
                mask.bits() as NSUInteger
            );
        }
        assert_eq!(unsafe { *(**host).get_ivar::<NSUInteger>("focusStep") }, 2);
    }

    #[test]
    fn ordinary_style_change_does_not_assign_an_absent_responder() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let host = ordinary_input_host_fixture();
        unsafe { (**host).set_ivar("responder", nil) };
        set_window_style_mask_preserving_responder(
            &host,
            NSWindowStyleMask::NSBorderlessWindowMask,
        );
        assert_eq!(unsafe { *(**host).get_ivar::<NSUInteger>("focusStep") }, 0);
    }

    #[test]
    fn ordinary_style_change_does_not_replace_another_windows_responder() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let first = ordinary_input_host_fixture();
        let second = ordinary_input_host_fixture();
        let first_view = unsafe { StrongPtr::new(msg_send![class!(NSObject), new]) };
        let second_view = unsafe { StrongPtr::new(msg_send![class!(NSObject), new]) };
        unsafe {
            (**first).set_ivar("responder", *first_view);
            (**second).set_ivar("responder", *second_view);
        }
        set_window_style_mask_preserving_responder(
            &first,
            NSWindowStyleMask::NSBorderlessWindowMask,
        );
        assert_eq!(
            unsafe { *(**first).get_ivar::<id>("responder") },
            *first_view
        );
        assert_eq!(
            unsafe { *(**second).get_ivar::<id>("responder") },
            *second_view
        );
        assert_eq!(
            unsafe { *(**second).get_ivar::<NSUInteger>("focusStep") },
            0
        );
    }

    #[test]
    fn ordinary_style_change_wiring_leaves_the_hotkey_panel_route_unchanged() {
        let source = include_str!("window.rs");
        let helper = production_method(source, "fn set_window_style_mask_preserving_responder(");
        assert!(
            helper.find("firstResponder").unwrap()
                < helper.find("window.setStyleMask_(mask)").unwrap()
        );
        assert!(
            helper.find("StrongPtr::retain").unwrap()
                < helper.find("window.setStyleMask_(mask)").unwrap()
        );
        assert!(
            helper.find("window.setStyleMask_(mask)").unwrap()
                < helper.find("window.makeFirstResponder_").unwrap()
        );
        assert!(!helper.contains("makeKeyAndOrderFront"));
        assert!(!helper.contains("activateWithOptions"));
        let decorations = production_method(source, "fn apply_decorations_to_window(");
        assert!(decorations.contains("window.setStyleMask_(hotkey_panel_style_mask(mask))"));
        assert!(decorations.contains("set_window_style_mask_preserving_responder(window, mask)"));
        let fullscreen = production_method(source, "fn toggle_simple_fullscreen(");
        assert!(
            fullscreen.contains("set_window_style_mask_preserving_responder(&self.window, mask)")
        );
        assert!(!fullscreen.contains("self.window.setStyleMask_"));
    }

    #[test]
    fn ordinary_input_focus_orders_front_then_selects_the_terminal_view() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let host = ordinary_input_host_fixture();
        let view = unsafe { StrongPtr::new(msg_send![class!(NSObject), new]) };
        assert_eq!(unsafe { *(**host).get_ivar::<id>("responder") }, *host);
        make_terminal_key_window(&host, &view);
        let responder = unsafe { *(**host).get_ivar::<id>("responder") };
        let responder_step = unsafe { *(**host).get_ivar::<NSUInteger>("responderStep") };
        let steps = unsafe { *(**host).get_ivar::<NSUInteger>("focusStep") };
        assert_eq!(responder, *view);
        assert_eq!(responder_step, 1);
        assert_eq!(steps, 2);
    }

    #[test]
    fn ordinary_input_focus_uses_each_hosts_view_and_reselects_on_reopen() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let first = ordinary_input_host_fixture();
        let second = ordinary_input_host_fixture();
        let first_view = unsafe { StrongPtr::new(msg_send![class!(NSObject), new]) };
        let second_view = unsafe { StrongPtr::new(msg_send![class!(NSObject), new]) };
        make_terminal_key_window(&first, &first_view);
        make_terminal_key_window(&second, &second_view);
        assert_eq!(
            unsafe { *(**first).get_ivar::<id>("responder") },
            *first_view
        );
        assert_eq!(
            unsafe { *(**second).get_ivar::<id>("responder") },
            *second_view
        );
        // A native reopen may have restored the host itself as first responder.
        unsafe { (**first).set_ivar("responder", *first) };
        make_terminal_key_window(&first, &first_view);
        assert_eq!(
            unsafe { *(**first).get_ivar::<id>("responder") },
            *first_view
        );
        assert_eq!(
            unsafe { *(**first).get_ivar::<NSUInteger>("responderStep") },
            3
        );
        assert_eq!(unsafe { *(**first).get_ivar::<NSUInteger>("focusStep") }, 4);
        assert_eq!(
            unsafe { *(**second).get_ivar::<NSUInteger>("focusStep") },
            2
        );
    }

    #[test]
    fn ordinary_input_focus_wiring_keeps_the_dedicated_panel_route() {
        let source = include_str!("window.rs");
        let ordinary = &source[source.find("impl WindowInner {").unwrap()..];
        for signature in ["fn show(&mut self)", "pub(crate) fn focus(&mut self)"] {
            let method = production_method(ordinary, signature);
            assert!(method.contains("make_terminal_key_window(&self.window, &self.view)"));
            assert!(
                method.find("if self.dedicated_hotkey").unwrap()
                    < method.find("self.show_global_hotkey_window()").unwrap()
            );
            assert!(
                method.find("return;").unwrap() < method.find("make_terminal_key_window").unwrap()
            );
            assert!(
                method.find("self.restore_ordinary_window()").unwrap()
                    < method.find("make_terminal_key_window").unwrap()
            );
        }
        let panel = production_method(source, "pub(crate) fn show_global_hotkey_window(");
        assert!(!panel.contains("make_terminal_key_window"));
        assert!(panel.contains("makeFirstResponder_(*self.view)"));
        assert!(!panel.contains("activateWithOptions_"));
    }

    #[repr(C)]
    struct FixtureInsets {
        top: CGFloat,
        left: CGFloat,
        bottom: CGFloat,
        right: CGFloat,
    }

    unsafe impl objc::Encode for FixtureInsets {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{NSEdgeInsets=dddd}") }
        }
    }

    // Foundation objects supply native selectors without creating windows or an NSApp.
    fn native_screen_fixture(link: id, top: CGFloat, scale: CGFloat) -> StrongPtr {
        static CLASS: std::sync::OnceLock<&'static Class> = std::sync::OnceLock::new();
        let cls = CLASS.get_or_init(|| {
            let mut cls = ClassDecl::new("KakuOsParameterFixture", class!(NSObject)).unwrap();
            cls.add_ivar::<id>("fixtureLink");
            cls.add_ivar::<CGFloat>("fixtureTop");
            cls.add_ivar::<CGFloat>("fixtureScale");
            cls.add_ivar::<NSUInteger>("fixtureStyle");
            cls.add_ivar::<*mut c_void>(VIEW_CLS_NAME);
            extern "C" fn link(this: &Object, _: Sel) -> id {
                unsafe { *this.get_ivar("fixtureLink") }
            }
            extern "C" fn style(this: &Object, _: Sel) -> NSUInteger {
                unsafe { *this.get_ivar("fixtureStyle") }
            }
            extern "C" fn insets(this: &Object, _: Sel) -> FixtureInsets {
                FixtureInsets {
                    top: unsafe { *this.get_ivar("fixtureTop") },
                    left: 0.,
                    bottom: 0.,
                    right: 0.,
                }
            }
            extern "C" fn frame(_: &Object, _: Sel) -> NSRect {
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(1920., 1080.))
            }
            extern "C" fn backing(this: &Object, _: Sel, rect: NSRect) -> NSRect {
                let scale: CGFloat = unsafe { *this.get_ivar("fixtureScale") };
                NSRect::new(
                    rect.origin,
                    NSSize::new(rect.size.width * scale, rect.size.height * scale),
                )
            }
            unsafe {
                cls.add_method(sel!(window), link as extern "C" fn(&Object, Sel) -> id);
                cls.add_method(sel!(screen), link as extern "C" fn(&Object, Sel) -> id);
                cls.add_method(
                    sel!(styleMask),
                    style as extern "C" fn(&Object, Sel) -> NSUInteger,
                );
                cls.add_method(
                    sel!(safeAreaInsets),
                    insets as extern "C" fn(&Object, Sel) -> FixtureInsets,
                );
                cls.add_method(sel!(frame), frame as extern "C" fn(&Object, Sel) -> NSRect);
                cls.add_method(
                    sel!(convertRectToBacking:),
                    backing as extern "C" fn(&Object, Sel, NSRect) -> NSRect,
                );
            }
            cls.register()
        });
        unsafe {
            let obj: id = msg_send![*cls, new];
            (*obj).set_ivar("fixtureLink", link);
            (*obj).set_ivar("fixtureTop", top);
            (*obj).set_ivar("fixtureScale", scale);
            (*obj).set_ivar("fixtureStyle", 0 as NSUInteger);
            (*obj).set_ivar(VIEW_CLS_NAME, std::ptr::null_mut::<c_void>());
            StrongPtr::new(obj)
        }
    }

    fn fixture_os_parameter_top(
        window_id: usize,
        borrowed_view: &RefCell<id>,
        main_screen: id,
        fallbacks: &Cell<usize>,
    ) -> Option<usize> {
        let view = os_parameters_native_view(window_id, || {
            borrowed_view.try_borrow().ok().map(|view| *view)
        });
        let window = view.as_ref().and_then(os_parameters_native_window);
        let screen = os_parameters_screen(window.as_ref().map(|window| **window), || {
            fallbacks.set(fallbacks.get() + 1);
            main_screen
        });
        screen_safe_area_top(screen)
    }

    #[test]
    fn native_screen_pinned_drawer_keeps_external_inset_under_outer_borrow() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let external = native_screen_fixture(nil, 0., 1.);
        let builtin = native_screen_fixture(nil, 37.5, 2.);
        let panel = native_screen_fixture(*external, 0., 1.);
        let view = native_screen_fixture(*panel, 0., 1.);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().insert(91, view.weak()));
        let outer = RefCell::new(*view);
        let _held = outer.borrow_mut();
        assert!(outer.try_borrow().is_err());
        let fallbacks = Cell::new(0);
        let top = fixture_os_parameter_top(91, &outer, *builtin, &fallbacks);
        assert_eq!(top, Some(0));
        assert_eq!(fallbacks.get(), 0);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().remove(&91));
    }

    #[test]
    fn native_screen_simple_fullscreen_keeps_current_screen_scale_under_outer_borrow() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let builtin = native_screen_fixture(nil, 37.5, 2.);
        let external = native_screen_fixture(nil, 0., 1.);
        let window = native_screen_fixture(*builtin, 0., 1.);
        let view = native_screen_fixture(*window, 0., 1.);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().insert(92, view.weak()));
        let outer = RefCell::new(*view);
        let _held = outer.borrow_mut();
        let fallbacks = Cell::new(0);
        let top = fixture_os_parameter_top(92, &outer, *external, &fallbacks);
        assert_eq!(top, Some(76));
        assert_eq!(fallbacks.get(), 0);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().remove(&92));
    }

    #[test]
    fn native_screen_view_ref_tracks_panel_swap_and_screen_change() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let external = native_screen_fixture(nil, 0., 1.);
        let builtin = native_screen_fixture(nil, 37.5, 2.);
        let ordinary = native_screen_fixture(*external, 0., 1.);
        let panel = native_screen_fixture(*builtin, 0., 1.);
        let view = native_screen_fixture(*ordinary, 0., 1.);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().insert(93, view.weak()));
        let outer = RefCell::new(*view);
        let fallbacks = Cell::new(0);
        assert_eq!(
            fixture_os_parameter_top(93, &outer, *builtin, &fallbacks),
            Some(0)
        );
        // The same view now belongs to the panel; no cached host/screen may survive.
        unsafe { (**view).set_ivar("fixtureLink", *panel) };
        let _held = outer.borrow_mut();
        assert_eq!(
            fixture_os_parameter_top(93, &outer, *external, &fallbacks),
            Some(76)
        );
        unsafe { (**panel).set_ivar("fixtureLink", *external) };
        assert_eq!(
            fixture_os_parameter_top(93, &outer, *builtin, &fallbacks),
            Some(0)
        );
        assert_eq!(fallbacks.get(), 0);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().remove(&93));
    }

    #[test]
    fn native_screen_current_fullscreen_flags_under_outer_borrow() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let ordinary = native_screen_fixture(nil, 0., 1.);
        let replacement = native_screen_fixture(nil, 0., 1.);
        let view = native_screen_fixture(*ordinary, 0., 1.);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().insert(95, view.weak()));
        let outer = RefCell::new(*view);
        let _held = outer.borrow_mut();
        let native_view =
            os_parameters_native_view(95, || outer.try_borrow().ok().map(|view| *view))
                .expect("current view must remain queryable under an outer mutable borrow");
        let window = os_parameters_native_window(&native_view).unwrap();
        assert_eq!(*window, *ordinary);
        assert_eq!(
            os_parameters_native_state(*window, WindowState::default()),
            (false, false)
        );
        unsafe {
            (**replacement).set_ivar(
                "fixtureStyle",
                NSWindowStyleMask::NSFullScreenWindowMask.bits(),
            );
            (**view).set_ivar("fixtureLink", *replacement);
        }
        let window = os_parameters_native_window(&native_view).unwrap();
        assert_eq!(*window, *replacement);
        assert_eq!(
            os_parameters_native_state(*window, WindowState::default()),
            (true, false)
        );
        unsafe { (**replacement).set_ivar("fixtureStyle", 0 as NSUInteger) };
        assert_eq!(
            os_parameters_native_state(*window, WindowState::HOTKEY_WINDOW),
            (false, true)
        );
        unsafe { (**view).set_ivar("fixtureLink", nil) };
        assert!(os_parameters_native_window(&native_view).is_none());
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().remove(&95));
    }

    #[test]
    fn native_screen_weak_view_expires_without_using_a_stale_borrowed_pointer() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let view = native_screen_fixture(nil, 0., 1.);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().insert(94, view.weak()));
        drop(view);
        let borrowed = Cell::new(false);
        assert!(os_parameters_native_view(94, || {
            borrowed.set(true);
            None
        })
        .is_none());
        assert!(!borrowed.get());
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().remove(&94));
        let view = native_screen_fixture(nil, 0., 1.);
        assert_eq!(
            *os_parameters_native_view(94, || Some(*view)).unwrap(),
            *view
        );
        assert_eq!(screen_safe_area_top(nil), None);
    }

    #[test]
    fn native_screen_parameters_use_live_screen_and_exclude_native_fullscreen() {
        let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
        let external = native_screen_fixture(nil, 0., 1.);
        let builtin = native_screen_fixture(nil, 37.5, 2.);
        let host = native_screen_fixture(*external, 0., 1.);
        let view = native_screen_fixture(*host, 0., 1.);
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().insert(96, view.weak()));
        let window = Window::for_test(96);
        let config = ConfigHandle::default_config();
        // No Connection or borrowable WindowInner exists in this fixture.
        assert!(window.ns_window().is_none());
        let parameters = window
            .get_os_parameters(&config, WindowState::HOTKEY_WINDOW)
            .unwrap()
            .unwrap();
        assert_eq!(parameters.border_dimensions.unwrap().top.get(), 0);
        unsafe { (**host).set_ivar("fixtureLink", *builtin) };
        let parameters = window
            .get_os_parameters(&config, WindowState::HOTKEY_WINDOW)
            .unwrap()
            .unwrap();
        assert_eq!(parameters.border_dimensions.unwrap().top.get(), 76);
        let ordinary = window
            .get_os_parameters(&config, WindowState::default())
            .unwrap()
            .unwrap();
        assert!(ordinary.border_dimensions.is_none());
        unsafe {
            (**host).set_ivar(
                "fixtureStyle",
                NSWindowStyleMask::NSFullScreenWindowMask.bits(),
            );
        }
        let fullscreen = window
            .get_os_parameters(&config, WindowState::HOTKEY_WINDOW)
            .unwrap()
            .unwrap();
        assert!(fullscreen.border_dimensions.is_none());
        OS_PARAMETER_VIEWS.with(|views| views.borrow_mut().remove(&96));
    }

    #[test]
    fn native_screen_registry_lifetime_and_parameter_wiring() {
        let source = include_str!("window.rs");
        let create = production_method(source, "pub async fn new_window<");
        assert!(create.contains(".insert(window_id, window_inner.borrow().view.weak())"));
        assert!(
            create.find("OS_PARAMETER_VIEWS.with").unwrap()
                < create.find("events.assign_window").unwrap()
        );
        let close = production_method(source, "extern \"C\" fn window_will_close(");
        assert!(
            close.find("remove(&view.window_id.get())").unwrap()
                < close.find("WindowEvent::Destroyed").unwrap()
        );
        let parameters = production_method(source, "fn get_os_parameters(");
        assert!(parameters.contains("os_parameters_native_view(self.id"));
        assert!(parameters.contains("os_parameters_native_window(&native_view)"));
        assert!(parameters.contains("os_parameters_native_state(*native_window, window_state)"));
        assert!(parameters.contains("os_parameters_screen(Some(*native_window)"));
        assert!(parameters.contains("view.simple_fullscreen_active.get()"));
        assert!(
            parameters.find("view.is_closing.get()").unwrap()
                < parameters.find("os_parameters_native_window").unwrap()
        );
        assert!(!parameters.contains("self.ns_window()"));
    }

    #[test]
    fn hotkey_options_egl_layer_opacity_uses_effective_window_config() {
        for (global_opacity, window_opacity, expected) in [(1., 0.5, NO), (0.5, 1., YES)] {
            assert_ne!(gl_layer_is_opaque(global_opacity), expected);
            assert_eq!(gl_layer_is_opaque(window_opacity), expected);
        }
        assert_eq!(gl_layer_is_opaque(0.), NO);
        assert_eq!(gl_layer_is_opaque(0.999), NO);
    }

    #[test]
    fn hotkey_options_egl_layer_opacity_wiring() {
        let source = include_str!("window.rs");
        let create = production_method(source, "pub fn create(view:");
        assert!(!create.contains("config::configuration().window_background_opacity"));
        assert!(create.contains("config::configuration().prefer_egl"));
        assert!(create.contains("let layer_opaque = gl_layer_is_opaque(window_opacity)"));
        assert!(create.contains("setOpaque: layer_opaque"));
        let enable = source
            .split("impl Inner {\n")
            .nth(1)
            .unwrap()
            .split("fn translate_key_event(")
            .next()
            .unwrap();
        assert!(
            enable.contains("GlContextPair::create(*view, self.config.window_background_opacity)")
        );
        assert!(!create.contains("inner.borrow"));
    }

    #[test]
    fn hotkey_options_view_opacity_cache_is_per_window_wiring() {
        let source = include_str!("window.rs");
        let opaque = production_method(source, "extern \"C\" fn is_opaque(");
        assert!(!opaque.contains("VIEW_IS_OPAQUE"));
        assert!(opaque.contains("this.opaque.get()"));
        assert!(!opaque.contains("inner.borrow"));
        let update = production_method(source, "fn update_window_shadow(");
        assert!(update.contains("window_view.opaque.set(is_opaque)"));
        let init = production_method(source, "fn init_with_frame(");
        let init: String = init.split_whitespace().collect();
        assert!(init.contains(
            "opaque:Cell::new(gl_layer_is_opaque(inner.borrow().config.window_background_opacity"
        ));
    }

    #[test]
    fn hotkey_options_view_opacity_caches_do_not_overwrite_each_other() {
        let ordinary = Cell::new(gl_layer_is_opaque(0.5));
        let drawer = Cell::new(gl_layer_is_opaque(1.));
        assert_eq!(ordinary.get(), NO);
        assert_eq!(drawer.get(), YES);
        drawer.set(gl_layer_is_opaque(0.3));
        assert_eq!(ordinary.get(), NO);
        ordinary.set(gl_layer_is_opaque(1.));
        assert_eq!(ordinary.get(), YES);
        assert_eq!(drawer.get(), NO);
    }

    #[test]
    fn hotkey_options_height_percent_controls_drawer_frame() {
        let screen = ScreenGeometry {
            frame: NSRect::new(NSPoint::new(-1200., 900.), NSSize::new(1200., 900.)),
            visible_frame: NSRect::new(NSPoint::new(-1200., 900.), NSSize::new(1200., 850.)),
        };
        for percent in [10., 50., 100.] {
            let frame = hotkey_panel_frame(screen, percent);
            assert_eq!(frame.size.height, 900. * percent / 100.);
            assert_eq!(frame.size.width, 1200.);
            assert_eq!(frame.origin.x, -1200.);
            assert_eq!(frame.origin.y + frame.size.height, 1800.);
        }
    }

    #[test]
    fn hotkey_options_animation_duration_respects_zero_and_reduce_motion() {
        assert_eq!(hotkey_animation_duration(0, false), Duration::ZERO);
        assert_eq!(
            hotkey_animation_duration(500, false),
            Duration::from_millis(500)
        );
        assert_eq!(hotkey_animation_duration(500, true), Duration::ZERO);
    }

    #[test]
    fn hotkey_options_slide_captures_duration_and_handles_zero_without_division() {
        let slide = HotkeySlide {
            resting_frame: NSRect::new(NSPoint::new(0., 600.), NSSize::new(1000., 300.)),
            from: NSPoint::new(0., 900.),
            to: NSPoint::new(0., 600.),
            started: Instant::now(),
            duration: hotkey_animation_duration(500, false),
            showing: true,
            hide_on_focus_loss: false,
        };
        assert_eq!(slide.origin_at(Duration::from_millis(250)).y, 750.);
        // A new setting does not change the duration of an existing slide.
        let new_duration = hotkey_animation_duration(0, false);
        assert_eq!(slide.duration, Duration::from_millis(500));
        let immediate = HotkeySlide {
            duration: new_duration,
            ..slide
        };
        assert_eq!(immediate.origin_at(Duration::ZERO).y, immediate.to.y);
        let hide = HotkeySlide {
            from: slide.to,
            to: slide.from,
            showing: false,
            ..immediate
        };
        assert_eq!(hide.origin_at(Duration::ZERO).y, hide.to.y);
    }

    #[test]
    fn hotkey_options_pinning_reverses_only_inflight_auto_hide() {
        let hide = HotkeySlide {
            resting_frame: NSRect::new(NSPoint::new(0., 600.), NSSize::new(1000., 300.)),
            from: NSPoint::new(0., 600.),
            to: NSPoint::new(0., 900.),
            started: Instant::now(),
            duration: Duration::from_millis(500),
            showing: false,
            hide_on_focus_loss: true,
        };
        let current = NSPoint::new(0., 750.);
        assert!(hide
            .reverse_when_auto_hide_disabled(true, current)
            .is_none());
        let reverse = hide
            .reverse_when_auto_hide_disabled(false, current)
            .unwrap();
        assert!(reverse.showing);
        assert!(!reverse.hide_on_focus_loss);
        assert_eq!(reverse.duration, hide.duration);
        assert_eq!(reverse.from.y, current.y);
        assert_eq!(reverse.to.y, hide.resting_frame.origin.y);
        assert!(HotkeySlide {
            hide_on_focus_loss: false,
            ..hide
        }
        .reverse_when_auto_hide_disabled(false, current)
        .is_none());
    }

    #[test]
    fn hotkey_options_hide_on_focus_loss_can_be_disabled() {
        assert!(!should_hide_hotkey_on_focus_loss(
            true, false, true, None, false
        ));
        assert!(should_hide_hotkey_on_focus_loss(
            true, false, true, None, true
        ));
        assert!(hotkey_should_hide(None, true, true));
    }

    #[test]
    fn hotkey_options_explicit_toggle_hides_a_pinned_unfocused_drawer() {
        assert!(drawer_hotkey_should_hide(None, false, true, false));
        assert!(!drawer_hotkey_should_hide(None, false, false, false));
        assert!(!drawer_hotkey_should_hide(None, false, true, true));
        assert!(drawer_hotkey_should_hide(Some(true), false, false, false));
        assert!(!drawer_hotkey_should_hide(Some(false), true, true, false));
    }

    #[test]
    fn hotkey_options_notch_override_inherits_only_when_unset() {
        assert!(!hotkey_extend_behind_notch(true, Some(false), true));
        assert!(hotkey_extend_behind_notch(true, Some(true), false));
        for inherited in [false, true] {
            assert_eq!(hotkey_extend_behind_notch(true, None, inherited), inherited);
            assert_eq!(
                hotkey_extend_behind_notch(false, Some(!inherited), inherited),
                inherited
            );
        }
    }

    #[test]
    fn hotkey_options_native_consumer_wiring() {
        let source = include_str!("window.rs");
        let reveal = production_method(source, "pub(crate) fn show_global_hotkey_window(");
        let refresh = production_method(source, "pub(crate) fn refresh_after_display_change(");
        let animate = production_method(source, "fn animate_hotkey_slide(");
        let reload = production_method(source, "fn config_did_change(&mut self,");
        let resign = production_method(source, "extern \"C\" fn did_resign_key(");
        let parameters = production_method(source, "fn get_os_parameters(");
        assert!(reveal.contains("self.config.macos_global_hotkey_height_percent"));
        assert!(refresh.contains("inner.config.macos_global_hotkey_height_percent"));
        assert!(animate.contains("self.config.macos_global_hotkey_animation_duration_ms"));
        assert!(resign.contains("inner.config.macos_global_hotkey_hide_on_focus_loss"));
        assert!(parameters.contains("config.macos_global_hotkey_extend_behind_notch"));
        let toggle = production_method(source, "pub(crate) fn global_hotkey_should_hide(");
        assert!(toggle.contains("drawer_hotkey_should_hide("));
        assert!(toggle.contains("self.config.macos_global_hotkey_hide_on_focus_loss"));
        assert!(reload.contains("hotkey_panel_frame("));
        assert!(reload.contains("reverse_when_auto_hide_disabled("));
        assert!(reload.contains("dpi_changed || panel_layout_changed"));
        assert!(reload.contains("screen_geometry_for_window(*self.window)"));
        assert!(!reload.contains("hotkey_screen_geometry("));
        let step = production_method(source, "fn step_hotkey_slide(");
        assert!(step.contains("elapsed >= slide.duration"));
        assert!(!step.contains("self.config.macos_global_hotkey_animation_duration_ms"));
        assert!(animate.contains("if slide.duration.is_zero()"));
        let origin = production_method(source, "fn origin_at(");
        assert!(
            origin.find("self.duration.is_zero()").unwrap()
                < origin.find("self.duration.as_secs_f64()").unwrap()
        );
    }

    #[test]
    fn boundary_cancelling_auto_hide_keeps_regained_focus() {
        let hide = HotkeySlide {
            resting_frame: NSRect::new(NSPoint::new(0., 600.), NSSize::new(1000., 300.)),
            from: NSPoint::new(0., 700.),
            to: NSPoint::new(0., 1000.),
            started: Instant::now(),
            duration: HOTKEY_SLIDE_DURATION,
            showing: false,
            hide_on_focus_loss: true,
        };
        assert!(!hide.should_hide_when_cancelled(true));
        assert!(hide.should_hide_when_cancelled(false));
        assert!(HotkeySlide {
            hide_on_focus_loss: false,
            ..hide
        }
        .should_hide_when_cancelled(true));
        assert!(!HotkeySlide {
            showing: true,
            ..hide
        }
        .should_hide_when_cancelled(false));
        let source = production_method(include_str!("window.rs"), "fn cancel_hotkey_slide(&self,");
        assert!(source.contains("slide.should_hide_when_cancelled("));
    }

    #[test]
    fn boundary_ordinary_move_updates_fill_cache_before_topology_observer() {
        let source = production_method(include_str!("window.rs"), "extern \"C\" fn did_move(");
        assert!(source.contains("refresh_windowed_screen_state("));
        let refresh = production_method(
            include_str!("window.rs"),
            "pub(crate) fn refresh_after_display_change(",
        );
        assert!(refresh.contains("fills_after_topology_change("));
        let resize = production_method(include_str!("window.rs"), "extern \"C\" fn did_resize(");
        assert!(resize.contains("!this.windowed_screen.get().max_invalidated_by_move"));
    }

    #[test]
    fn boundary_live_screen_move_invalidates_old_fill_and_maximized_intent() {
        let (screens, filled_a, _) = side_by_side_screens();
        let initial = WindowedScreenState::default().settled(filled_a, Some(1), screens[0]);
        let moved_frame = NSRect::new(NSPoint::new(1550., 80.), filled_a.size);
        let moved = initial.after_frame_change(moved_frame, Some(2), screens[1], Some(screens[0]));
        assert_eq!(moved.screen_id, Some(2));
        assert!(!moved.fills_visible_frame);
        assert!(moved.max_invalidated_by_move);
        assert!(!moved.fills_after_topology_change(true));
        // A subsequent resize must not revive stale isZoomed/last MAX.
        let unchanged =
            moved.after_frame_change(moved_frame, Some(2), screens[1], Some(screens[1]));
        assert!(unchanged.max_invalidated_by_move);
        assert!(!unchanged.fills_after_topology_change(true));
        let forced = moved.after_frame_change(filled_a, Some(1), screens[0], None);
        assert!(!forced.fills_after_topology_change(true));
        let resized = moved.after_frame_change(
            NSRect::new(moved_frame.origin, NSSize::new(900., 700.)),
            Some(2),
            screens[1],
            Some(screens[1]),
        );
        assert!(!resized.max_invalidated_by_move);
        let maximized = moved.after_frame_change(
            screens[1].visible_frame,
            Some(2),
            screens[1],
            Some(screens[1]),
        );
        assert!(maximized.fills_visible_frame);
        assert!(!maximized.max_invalidated_by_move);
    }

    #[test]
    fn boundary_topology_moves_preserve_prior_fill_even_when_screen_id_survives() {
        let (screens, filled_a, _) = side_by_side_screens();
        let initial = WindowedScreenState::default().settled(filled_a, Some(1), screens[0]);
        for previous in [
            None,
            Some(ScreenGeometry {
                frame: screens[1].frame,
                ..screens[0]
            }),
            Some(ScreenGeometry {
                visible_frame: screens[1].visible_frame,
                ..screens[0]
            }),
        ] {
            let migrated =
                initial.after_frame_change(screens[1].visible_frame, Some(2), screens[1], previous);
            assert_eq!(migrated.screen_id, Some(1));
            assert!(nsrect_approx_eq(migrated.frame.unwrap(), filled_a, 0.));
            assert!(migrated.fills_after_topology_change(true));
        }
        let settled = initial.settled(screens[1].visible_frame, Some(2), screens[1]);
        assert_eq!(settled.screen_id, Some(2));
        assert!(settled.fills_visible_frame);
    }

    #[test]
    fn boundary_fullscreen_exit_rebases_windowed_screen_cache() {
        let source = include_str!("window.rs");
        let simple = production_method(source, "fn toggle_simple_fullscreen(&mut self)");
        assert!(simple.contains("settle_windowed_screen_state(*self.window)"));
        let native = production_method(source, "extern \"C\" fn did_exit_fullscreen(");
        assert!(native.contains("settle_windowed_screen_state(window)"));

        let (screens, filled_a, _) = side_by_side_screens();
        let initial = WindowedScreenState::default().settled(filled_a, Some(1), screens[0]);
        // The display changed during fullscreen while the windowed cache froze.
        let landed = initial.settled(screens[1].visible_frame, Some(1), screens[1]);
        let smaller = NSRect::new(NSPoint::new(1500., 80.), NSSize::new(800., 600.));
        let resized = landed.after_frame_change(smaller, Some(1), screens[1], Some(screens[1]));
        assert!(!resized.fills_after_topology_change(false));
        assert!(nsrect_approx_eq(resized.frame.unwrap(), smaller, 0.));
    }

    #[test]
    fn boundary_screen_change_dispatches_deferred_resize_without_dpi_change() {
        let source = production_method(
            include_str!("window.rs"),
            "extern \"C\" fn did_change_screen(",
        );
        assert!(source.contains("Connection::with_window_inner("));
        assert!(source.contains("inner.dispatch_resize_event()"));
    }

    #[test]
    fn centered_frame_origin_centers_inside_visible_frame() {
        // A secondary display sitting left of the primary, in its own
        // point coordinates: origin can be negative.
        let visible = NSRect::new(NSPoint::new(-1920., 100.), NSSize::new(1920., 1055.));
        let origin = centered_frame_origin(visible, NSSize::new(800., 600.));
        assert_eq!(origin.x, -1360.);
        assert_eq!(origin.y, 327.5);
    }

    #[test]
    fn centered_frame_origin_pins_oversized_window_to_visible_origin() {
        let visible = NSRect::new(NSPoint::new(-1920., 100.), NSSize::new(1920., 1055.));
        let origin = centered_frame_origin(visible, NSSize::new(2400., 1600.));
        assert_eq!(origin.x, -1920.);
        assert_eq!(origin.y, 100.);
    }

    #[test]
    fn persisted_window_state_keeps_managed_shell() {
        let mut state: PersistedState = serde_json::from_str(
            r#"{"config_version":22,"managed_shell":"fish","window_geometry":{"width":120,"height":40},"future_setting":{"enabled":true}}"#,
        )
        .unwrap();
        state.window_geometry = Some(PersistedWindowSize {
            width: 140,
            height: 50,
        });

        let saved = serde_json::to_value(state).unwrap();
        assert_eq!(saved["managed_shell"], "fish");
        assert_eq!(saved["window_geometry"]["width"], 140);
        assert_eq!(saved["future_setting"]["enabled"], true);
    }

    fn side_by_side_screens() -> ([ScreenGeometry; 2], NSRect, NSRect) {
        let left_visible = NSRect::new(NSPoint::new(0., 25.), NSSize::new(1440., 875.));
        let right_visible = NSRect::new(NSPoint::new(1440., 0.), NSSize::new(1920., 1080.));
        (
            [
                ScreenGeometry {
                    frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1440., 900.)),
                    visible_frame: left_visible,
                },
                ScreenGeometry {
                    frame: NSRect::new(NSPoint::new(1440., 0.), NSSize::new(1920., 1080.)),
                    visible_frame: right_visible,
                },
            ],
            left_visible,
            right_visible,
        )
    }

    #[test]
    fn hotkey_follow_mouse_selects_each_display_and_shared_boundary() {
        let (screens, _, _) = side_by_side_screens();
        for (mouse, expected) in [
            (NSPoint::new(100., 100.), screens[0]),
            (NSPoint::new(1440., 100.), screens[1]),
            (NSPoint::new(3000., 1000.), screens[1]),
        ] {
            let selected =
                select_hotkey_screen_geometry(Some(mouse), &screens, Some(screens[0])).unwrap();
            assert!(nsrect_approx_eq(selected.frame, expected.frame, 0.0));
            assert!(nsrect_approx_eq(
                selected.visible_frame,
                expected.visible_frame,
                0.0
            ));
        }
    }

    #[test]
    fn hotkey_follow_mouse_handles_negative_and_stacked_display_origins() {
        let screens = [
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(-1920., -200.), NSSize::new(1920., 1080.)),
                visible_frame: NSRect::new(NSPoint::new(-1920., -200.), NSSize::new(1920., 1055.)),
            },
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(0., 900.), NSSize::new(1440., 900.)),
                visible_frame: NSRect::new(NSPoint::new(0., 900.), NSSize::new(1440., 875.)),
            },
        ];
        for (mouse, expected) in [
            (NSPoint::new(-100., -100.), screens[0]),
            (NSPoint::new(100., 1000.), screens[1]),
        ] {
            let selected = select_hotkey_screen_geometry(Some(mouse), &screens, None).unwrap();
            assert!(nsrect_approx_eq(selected.frame, expected.frame, 0.0));
        }
    }

    #[test]
    fn hotkey_follow_mouse_falls_back_when_disabled_or_no_display_matches() {
        let (screens, _, _) = side_by_side_screens();
        for mouse in [
            None,
            Some(NSPoint::new(-500., -500.)),
            Some(NSPoint::new(3360., 100.)),
        ] {
            let selected =
                select_hotkey_screen_geometry(mouse, &screens, Some(screens[0])).unwrap();
            assert!(nsrect_approx_eq(selected.frame, screens[0].frame, 0.0));
        }
        assert!(select_hotkey_screen_geometry(Some(NSPoint::new(1., 1.)), &[], None).is_none());
        let selected =
            select_hotkey_screen_geometry(Some(NSPoint::new(1., 1.)), &[], Some(screens[0]))
                .unwrap();
        assert!(nsrect_approx_eq(selected.frame, screens[0].frame, 0.0));
    }

    #[test]
    fn target_screen_follows_title_bar_across_displays() {
        let (screens, _left_visible, right_visible) = side_by_side_screens();
        // Clear of the left display entirely, so only the right one bounds it.
        let target = NSRect::new(NSPoint::new(1500., 100.), NSSize::new(500., 500.));

        let selected = select_visible_frame_for_target(target, &screens, None).unwrap();
        assert!(nsrect_approx_eq(selected, right_visible, 0.0));
    }

    #[test]
    fn straddling_windows_obey_the_shorter_displays_menu_bar() {
        let (screens, left_visible, _right_visible) = side_by_side_screens();
        // Spans both displays. The window center sits on the right display,
        // which has no menu bar inset, so center-point selection used to hand
        // back the right visible frame and let the left half of the title bar
        // rise past the left display's menu bar (#519).
        let target = NSRect::new(NSPoint::new(1300., 100.), NSSize::new(500., 500.));

        let selected = select_visible_frame_for_target(target, &screens, None).unwrap();
        assert!(nsrect_approx_eq(selected, left_visible, 0.0));
    }

    #[test]
    fn dragging_up_while_straddling_clamps_to_the_menu_bar_display() {
        // Built-in laptop display with a menu bar, taller external to its
        // right with none: the arrangement from #519.
        let builtin_visible = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1512., 957.));
        let external_visible = NSRect::new(NSPoint::new(1512., 0.), NSSize::new(2560., 1440.));
        let screens = [
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1512., 982.)),
                visible_frame: builtin_visible,
            },
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(1512., 0.), NSSize::new(2560., 1440.)),
                visible_frame: external_visible,
            },
        ];
        // Straddles the seam, dragged so its top edge is at 1100 -- above the
        // built-in display's frame entirely.
        let target = NSRect::new(NSPoint::new(1200., 600.), NSSize::new(800., 500.));

        let selected = select_visible_frame_for_target(target, &screens, None).unwrap();
        assert!(nsrect_approx_eq(selected, builtin_visible, 0.0));
        // Top pinned to 957, so origin.y = 957 - 500.
        assert_eq!(clamp_frame_top_below_menu_bar(target, selected), Some(457.));
        // The pre-fix selection left the title bar unclamped at y = 1100.
        assert_eq!(
            clamp_frame_top_below_menu_bar(target, external_visible),
            None
        );
    }

    #[test]
    fn target_screen_falls_back_to_the_current_display() {
        let (screens, left_visible, _right_visible) = side_by_side_screens();
        // Entirely to the left of every display: nothing bounds the title bar,
        // so ownership stays with the display the window is on.
        let target = NSRect::new(NSPoint::new(-900., 100.), NSSize::new(500., 500.));

        assert!(select_visible_frame_for_target(target, &screens, None).is_none());
        let selected =
            select_visible_frame_for_target(target, &screens, Some(left_visible)).unwrap();
        assert!(nsrect_approx_eq(selected, left_visible, 0.0));
    }

    /// Built-in laptop below carrying the menu bar, taller external stacked
    /// above it.
    fn stacked_screens() -> ([ScreenGeometry; 2], NSRect, NSRect) {
        let builtin_visible = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1512., 957.));
        let external_visible = NSRect::new(NSPoint::new(0., 982.), NSSize::new(2560., 1440.));
        (
            [
                ScreenGeometry {
                    frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1512., 982.)),
                    visible_frame: builtin_visible,
                },
                ScreenGeometry {
                    frame: NSRect::new(NSPoint::new(0., 982.), NSSize::new(2560., 1440.)),
                    visible_frame: external_visible,
                },
            ],
            builtin_visible,
            external_visible,
        )
    }

    #[test]
    fn lower_display_menu_bar_clamps_until_the_title_bar_clears_it() {
        let (screens, builtin_visible, _external_visible) = stacked_screens();
        // Wholly inside the built-in display, dragged up so the title bar would
        // enter the menu bar strip (957..982). Excluding the built-in display
        // just because a taller one is stacked above dropped the clamp here and
        // left the title bar behind the menu bar with no way to grab it back.
        let target = NSRect::new(NSPoint::new(200., 500.), NSSize::new(800., 460.));

        let selected = select_visible_frame_for_target(target, &screens, None).unwrap();
        assert!(nsrect_approx_eq(selected, builtin_visible, 0.0));
        assert_eq!(clamp_frame_top_below_menu_bar(target, selected), Some(497.));
    }

    #[test]
    fn stacked_crossing_frees_the_window_once_the_title_bar_clears_the_lower_frame() {
        let (screens, _builtin_visible, external_visible) = stacked_screens();
        // Same drag continued: the title bar is now above the built-in
        // display's frame entirely, so the crossing must not be pinned back
        // down to its menu bar (#508).
        let target = NSRect::new(NSPoint::new(200., 523.), NSSize::new(800., 460.));

        let selected = select_visible_frame_for_target(target, &screens, None).unwrap();
        assert!(nsrect_approx_eq(selected, external_visible, 0.0));
        assert_eq!(clamp_frame_top_below_menu_bar(target, selected), None);
    }

    #[test]
    fn target_screen_handles_vertical_and_gap_arrangements() {
        let lower_visible = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1000., 700.));
        let upper_visible = NSRect::new(NSPoint::new(0., 900.), NSSize::new(1000., 700.));
        let vertical = [
            ScreenGeometry {
                frame: lower_visible,
                visible_frame: lower_visible,
            },
            ScreenGeometry {
                frame: upper_visible,
                visible_frame: upper_visible,
            },
        ];
        let target = NSRect::new(NSPoint::new(200., 780.), NSSize::new(500., 400.));
        let selected = select_visible_frame_for_target(target, &vertical, None).unwrap();
        assert!(nsrect_approx_eq(selected, upper_visible, 0.0));

        let left = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1000., 800.));
        let right = NSRect::new(NSPoint::new(1400., 0.), NSSize::new(1000., 800.));
        let gap = [
            ScreenGeometry {
                frame: left,
                visible_frame: left,
            },
            ScreenGeometry {
                frame: right,
                visible_frame: right,
            },
        ];
        // Wholly inside the gap between the two displays: keep the display the
        // window is already on rather than inventing a nearest one.
        let target = NSRect::new(NSPoint::new(1190., 200.), NSSize::new(100., 100.));
        let selected = select_visible_frame_for_target(target, &gap, Some(left)).unwrap();
        assert!(nsrect_approx_eq(selected, left, 0.0));
    }

    #[test]
    fn vertical_crossing_does_not_clamp_to_the_old_screen() {
        let lower_frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1000., 1000.));
        let lower_visible = NSRect::new(NSPoint::new(0., 0.), NSSize::new(1000., 975.));
        let upper_frame = NSRect::new(NSPoint::new(0., 1000.), NSSize::new(1000., 1000.));
        let upper_visible = NSRect::new(NSPoint::new(0., 1000.), NSSize::new(1000., 975.));
        let screens = [
            ScreenGeometry {
                frame: lower_frame,
                visible_frame: lower_visible,
            },
            ScreenGeometry {
                frame: upper_frame,
                visible_frame: upper_visible,
            },
        ];
        // Equal body overlap on both displays, but the title bar has already
        // crossed into the upper display. Largest-overlap selection used to
        // keep choosing the lower display and pin origin.y back to 375.
        let target = NSRect::new(NSPoint::new(200., 700.), NSSize::new(600., 600.));

        let selected = select_visible_frame_for_target(target, &screens, None).unwrap();
        assert!(nsrect_approx_eq(selected, upper_visible, 0.0));
        assert_eq!(clamp_frame_top_below_menu_bar(target, selected), None);
        assert_eq!(
            clamp_frame_top_below_menu_bar(target, lower_visible),
            Some(375.)
        );
    }

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
    fn requested_drag_survives_cached_and_native_state_disagreement() {
        for cached_maximized in [false, true] {
            for is_zoomed in [false, true] {
                for fills_visible_frame in [false, true] {
                    assert_ne!(
                        requested_window_drag_action(
                            false,
                            is_zoomed,
                            fills_visible_frame,
                            cached_maximized
                        ),
                        RequestedDragAction::None,
                        "a non-fullscreen title drag must have a working path"
                    );
                    assert_eq!(
                        requested_window_drag_action(
                            true,
                            is_zoomed,
                            fills_visible_frame,
                            cached_maximized
                        ),
                        RequestedDragAction::None
                    );
                }
            }
        }
    }

    #[test]
    fn maximized_native_drag_is_deferred_until_pointer_moves() {
        // A zoomed window that fills the frame requested the drag because it is
        // maximized: defer so a bare single click cannot snap/maximize it (#414),
        // while a real drag still pulls it off the top (#428).
        assert_eq!(
            requested_window_drag_action(false, true, true, true),
            RequestedDragAction::DeferUntilDrag
        );
    }

    #[test]
    fn non_maximized_native_drag_fires_immediately() {
        // An ordinary (un-zoomed, not-filling) window keeps the prior synchronous
        // behavior: the press itself starts the native drag.
        assert_eq!(
            requested_window_drag_action(false, false, false, false),
            RequestedDragAction::PerformNow
        );
    }

    #[test]
    fn fullscreen_drag_is_disabled_but_filled_window_drag_is_deferred() {
        assert_eq!(
            requested_window_drag_action(true, true, true, true),
            RequestedDragAction::None
        );
        // Window managers can fill the visible frame without AppKit zooming it.
        assert_eq!(
            requested_window_drag_action(false, false, true, true),
            RequestedDragAction::DeferUntilDrag
        );
    }

    #[test]
    fn native_macos_fullscreen_collection_behavior_allows_tiling() {
        let existing =
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
                | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                | fullscreen_disallows_tiling_behavior();

        let behavior = native_macos_fullscreen_collection_behavior(existing, true);

        assert!(behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
        ));
        assert!(behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
        ));
        assert!(behavior.contains(fullscreen_allows_tiling_behavior()));
        assert!(!behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
        ));
        assert!(!behavior.contains(fullscreen_disallows_tiling_behavior()));
    }

    #[test]
    fn simple_fullscreen_collection_behavior_removes_explicit_tiling_bits() {
        let existing =
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
                | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
                | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                | fullscreen_allows_tiling_behavior()
                | fullscreen_disallows_tiling_behavior();

        let behavior = native_macos_fullscreen_collection_behavior(existing, false);

        assert!(behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
        ));
        assert!(!behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
        ));
        assert!(!behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
        ));
        assert!(!behavior.contains(fullscreen_allows_tiling_behavior()));
        assert!(!behavior.contains(fullscreen_disallows_tiling_behavior()));
    }

    #[test]
    fn simple_fullscreen_follows_remaining_display_after_disconnect() {
        let external = NSRect::new(NSPoint::new(-1920., 0.), NSSize::new(1920., 1080.));
        let builtin = ScreenGeometry {
            frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1728., 1117.)),
            visible_frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1728., 1084.)),
        };
        let adjusted = frame_after_display_change(external, true, false, builtin);
        assert!(nsrect_approx_eq(adjusted, builtin.frame, 0.5));
    }

    fn production_method<'a>(source: &'a str, signature: &str) -> &'a str {
        // Native AppKit callbacks cannot run on the unit-test worker threads.
        // Guard their entrypoints as well as testing the decision helpers.
        let indent = if source.contains(&format!("\n    {signature}")) {
            "    "
        } else {
            ""
        };
        let signature = format!("\n{indent}{signature}");
        let method = &source[source.find(&signature).expect("production method missing")..];
        &method[..method
            .find(&format!("\n{indent}}}"))
            .expect("method end missing")]
    }

    #[test]
    fn dedicated_hotkey_window_never_becomes_an_ordinary_window() {
        let source = include_str!("window.rs");
        let source = &source[source.find("impl WindowInner {").unwrap()..];
        for signature in [
            "fn show(&mut self)",
            "pub(crate) fn focus(&mut self)",
            "fn restore_ordinary_window(",
            "fn toggle_fullscreen(&mut self)",
        ] {
            let method = production_method(source, signature);
            assert!(method.contains("self.dedicated_hotkey"), "{}", signature);
        }
    }

    #[test]
    fn hotkey_lifecycle_defers_migration_until_native_transition_finishes() {
        let source = include_str!("window.rs");
        let show = production_method(source, "pub(crate) fn show_global_hotkey_window(");
        let guard = show.find("self.defer_hotkey_visibility(true)");
        let detach = show.find("self.prepare_global_hotkey_panel()").unwrap();
        assert!(guard.is_some_and(|guard| guard < detach));
    }

    #[test]
    fn hotkey_lifecycle_hides_native_fullscreen_through_the_shared_safe_path() {
        let source = include_str!("window.rs");
        let hide = production_method(source, "pub(crate) fn hide_global_hotkey_window(");
        assert!(hide.contains("self.defer_hotkey_visibility(false)"));
        assert!(hide.contains("self.order_out_after_fullscreen_exit()"));
        let ordinary = production_method(source, "fn order_out(&self)");
        assert!(ordinary.contains("inner.order_out_after_fullscreen_exit()"));
    }

    #[test]
    fn hotkey_lifecycle_geometry_changes_supersede_animation() {
        let source = include_str!("window.rs");
        for signature in [
            "fn set_inner_size(&mut self,",
            "fn set_window_position(&",
            "fn maximize(&mut self)",
            "fn restore(&mut self)",
            "fn center(&mut self)",
            "fn hide(&mut self)",
        ] {
            // Only inspect the WindowInner implementation, not WindowOps.
            let source = &source[source.find("impl WindowInner {").unwrap()..];
            let method = production_method(source, signature);
            assert!(
                method.contains("self.cancel_hotkey_slide()"),
                "{}",
                signature
            );
        }
    }

    #[test]
    fn hotkey_lifecycle_callbacks_replay_only_the_latest_pending_intent() {
        let source = include_str!("window.rs");
        for signature in [
            "extern \"C\" fn did_enter_fullscreen(",
            "extern \"C\" fn did_exit_fullscreen(",
        ] {
            let callback = production_method(source, signature);
            assert!(callback.contains("this.dispatch_pending_hotkey_visibility()"));
        }
        let replay = production_method(source, "fn dispatch_pending_hotkey_visibility(");
        assert!(replay.contains("Connection::with_window_inner"));
        assert!(replay.contains("view.pending_hotkey_visibility.replace(None)"));
        assert!(replay.contains("view.is_closing.get()"));
        assert!(replay.contains("view.native_fullscreen_transition_active.get()"));
        for signature in ["fn restore_ordinary_window(", "fn close(&mut self)"] {
            assert!(production_method(source, signature)
                .contains("self.clear_pending_hotkey_visibility()"));
        }
    }

    #[test]
    fn hotkey_lifecycle_pending_request_tracks_rapid_reversal() {
        let pending = Cell::new(None);
        assert!(defer_hotkey_request(&pending, true, false));
        assert!(!hotkey_should_hide(pending.get(), true, true));
        assert!(defer_hotkey_request(&pending, true, true));
        assert!(hotkey_should_hide(pending.get(), false, false));
        assert!(defer_hotkey_request(&pending, true, false));
        assert_eq!(pending.replace(None), Some(false));
        assert_eq!(pending.replace(None), None);
        assert!(!defer_hotkey_request(&pending, false, true));
        assert_eq!(pending.get(), None);
    }

    #[test]
    fn hotkey_lifecycle_repeated_hide_does_not_reverse_a_native_exit() {
        assert!(!should_start_native_fullscreen_exit(true, Some(false)));
        assert!(should_start_native_fullscreen_exit(false, Some(false)));
        assert!(should_start_native_fullscreen_exit(true, Some(true)));
        assert!(should_start_native_fullscreen_exit(false, None));
        let hide = production_method(
            include_str!("window.rs"),
            "fn order_out_after_fullscreen_exit(",
        );
        assert!(
            hide.find("should_start_native_fullscreen_exit").unwrap()
                < hide.find("self.exit_native_fullscreen()").unwrap()
        );
    }

    #[test]
    fn hotkey_lifecycle_drag_and_live_resize_settle_animation() {
        let source = include_str!("window.rs");
        let drag = production_method(source, "extern \"C\" fn mouse_down(");
        let settle = drag.find("view.cancel_hotkey_slide(window)").unwrap();
        assert!(settle < drag.find("performWindowDragWithEvent: nsevent").unwrap());
        let resize = production_method(source, "extern \"C\" fn will_start_live_resize(");
        assert!(resize.contains("this.cancel_hotkey_slide(window)"));
        let zoom = production_method(source, "extern \"C\" fn window_will_use_standard_frame(");
        assert!(zoom.contains("view.cancel_hotkey_slide(window)"));
    }

    #[test]
    fn hotkey_lifecycle_same_size_housekeeping_does_not_cancel_reveal() {
        let size = NSSize::new(800., 600.);
        assert!(!content_size_changed(size, size));
        assert!(!content_size_changed(size, NSSize::new(800.001, 600.)));
        assert!(content_size_changed(size, NSSize::new(800.5, 600.)));
        assert!(content_size_changed(size, NSSize::new(800., 601.)));
        let method = production_method(include_str!("window.rs"), "fn set_inner_size(&mut self,");
        assert!(method.contains("if content_size_changed(frame.size, size)"));
    }

    #[test]
    fn hotkey_panel_clears_layer_rounding_on_creation_and_config_reload() {
        let source = include_str!("window.rs");
        let update = production_method(source, "fn update_window_shadow(&mut self)");
        assert!(update.contains("is_hotkey_panel(*self.window)"));
        assert!(update.contains("if force_square {\n                        0.0"));
        assert!(update.contains("setCornerRadius: corner_radius"));
        let backing = production_method(source, "extern \"C\" fn make_backing_layer(");
        assert!(backing.contains("is_hotkey_panel("));
        assert!(backing.contains("&& !force_square"));
        let prepare = production_method(source, "fn prepare_global_hotkey_panel(&mut self)");
        assert!(prepare.contains("self.update_window_shadow()"));
        let reload = production_method(source, "fn config_did_change(&mut self,");
        assert!(reload.contains("self.update_window_shadow()"));
        // A square drawer needs a borderless host plus explicit panel input opt-in.
        let panel = hotkey_panel_style_mask(NSWindowStyleMask::NSResizableWindowMask);
        assert!(!panel.contains(NSWindowStyleMask::NSTitledWindowMask));
        assert!(panel.contains(nonactivating_panel_mask()));
    }

    #[test]
    fn hotkey_panel_is_nonactivating_even_from_fullscreen_or_borderless() {
        for mask in [
            NSWindowStyleMask::NSFullScreenWindowMask,
            NSWindowStyleMask::NSBorderlessWindowMask,
            NSWindowStyleMask::NSTitledWindowMask
                | NSWindowStyleMask::NSClosableWindowMask
                | NSWindowStyleMask::NSMiniaturizableWindowMask
                | NSWindowStyleMask::NSResizableWindowMask,
        ] {
            let panel = hotkey_panel_style_mask(mask);
            assert!(panel.contains(nonactivating_panel_mask()));
            assert!(!panel.contains(NSWindowStyleMask::NSTitledWindowMask));
            assert!(panel.contains(NSWindowStyleMask::NSFullSizeContentViewWindowMask));
            assert!(!panel.intersects(
                NSWindowStyleMask::NSClosableWindowMask
                    | NSWindowStyleMask::NSMiniaturizableWindowMask
                    | NSWindowStyleMask::NSFullScreenWindowMask
            ));
            assert_eq!(
                panel.contains(NSWindowStyleMask::NSResizableWindowMask),
                mask.contains(NSWindowStyleMask::NSResizableWindowMask)
            );
        }
    }

    #[test]
    fn borderless_hotkey_view_opts_into_panel_keyboard_focus() {
        let source = include_str!("window.rs");
        let class = production_method(source, "fn define_class()");
        assert!(class.contains("sel!(needsPanelToBecomeKey)"));
        let input = production_method(source, "extern \"C\" fn needs_panel_to_become_key(");
        assert!(input.contains("is_hotkey_panel(window)"));
        assert!(input.contains("!window.is_null()"));
        assert!(!input.contains("borrow"));
        let show = production_method(source, "pub(crate) fn show_global_hotkey_window(");
        let make_key = show.find("makeKeyAndOrderFront_(nil)").unwrap();
        let make_responder = show.find("makeFirstResponder_(*self.view)").unwrap();
        assert!(make_key < make_responder);
    }

    #[test]
    fn hotkey_panel_frame_is_full_width_top_third_in_appkit_points() {
        for screen in [
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1728., 1117.)),
                visible_frame: NSRect::new(NSPoint::new(0., 70.), NSSize::new(1728., 1022.)),
            },
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(-1920., -200.), NSSize::new(1920., 1080.)),
                visible_frame: NSRect::new(NSPoint::new(-1850., -200.), NSSize::new(1850., 1055.)),
            },
            ScreenGeometry {
                frame: NSRect::new(NSPoint::new(0., 1117.), NSSize::new(1080., 1920.)),
                visible_frame: NSRect::new(NSPoint::new(0., 1117.), NSSize::new(1080., 1895.)),
            },
        ] {
            let frame = hotkey_panel_frame(screen, 100. / 3.);
            assert_eq!(frame.origin.x, screen.frame.origin.x);
            assert_eq!(frame.size.width, screen.frame.size.width);
            assert!((frame.size.height - screen.frame.size.height / 3.).abs() < 1e-9);
            assert!((rect_top(frame) - rect_top(screen.frame)).abs() < 0.001);
        }
    }

    #[test]
    fn hotkey_panel_frame_ignores_menu_bar_and_dock_reservations() {
        let screen = ScreenGeometry {
            frame: NSRect::new(NSPoint::new(-1920., -200.), NSSize::new(1920., 1080.)),
            visible_frame: NSRect::new(NSPoint::new(-1920., 650.), NSSize::new(1920., 205.)),
        };
        assert!(nsrect_approx_eq(
            hotkey_panel_frame(screen, 100. / 3.),
            NSRect::new(NSPoint::new(-1920., 520.), NSSize::new(1920., 360.)),
            0.
        ));
    }

    #[test]
    fn hotkey_panel_is_above_the_menu_bar_but_keeps_system_popups_above_it() {
        use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
        let level = hotkey_window_level();
        assert!(
            level
                > i64::from(CGWindowLevelForKey(
                    CGWindowLevelKey::MainMenuWindowLevelKey
                ))
        );
        assert!(level > i64::from(CGWindowLevelForKey(CGWindowLevelKey::StatusWindowLevelKey)));
        assert!(
            level
                < i64::from(CGWindowLevelForKey(
                    CGWindowLevelKey::PopUpMenuWindowLevelKey
                ))
        );
        for native in [-1, 0, 3, level] {
            assert_eq!(
                nswindow_level_to_window_level(native, true),
                WindowLevel::AlwaysOnTop
            );
        }
        for (native, expected) in [
            (-1, WindowLevel::AlwaysOnBottom),
            (0, WindowLevel::Normal),
            (3, WindowLevel::AlwaysOnTop),
        ] {
            assert_eq!(nswindow_level_to_window_level(native, false), expected);
        }
        let source = include_str!("window.rs");
        let prepare = production_method(source, "fn prepare_global_hotkey_panel(");
        assert!(prepare.contains("panel.setLevel_(hotkey_window_level())"));
        let level = production_method(
            &source[source.find("impl WindowInner {").unwrap()..],
            "fn set_window_level(",
        );
        assert!(level.contains("self.dedicated_hotkey"));
        assert!(level.contains("hotkey_window_level()"));
        let resize = production_method(source, "extern \"C\" fn did_resize(");
        assert!(resize.contains("nswindow_level_to_window_level(level, hotkey_panel)"));
        let parameters = production_method(source, "fn get_os_parameters(");
        assert!(parameters.contains("simple_full_screen || hotkey_panel"));
    }

    #[test]
    fn ordinary_frame_after_hotkey_preserves_cross_display_geometry() {
        let (screens, _, _) = side_by_side_screens();
        let original = NSRect::new(NSPoint::new(200., 50.), NSSize::new(2200., 700.));
        for fullscreen in [false, true] {
            let restored = frame_after_display_topology_change(
                original,
                fullscreen,
                false,
                &[screens[0].frame, screens[1].frame],
                &[screens[0].frame, screens[1].frame],
                screens[0],
            );
            assert!(nsrect_approx_eq(restored, original, 0.));
        }
        let after_disconnect = frame_after_display_topology_change(
            NSRect::new(NSPoint::new(2000., 50.), NSSize::new(2200., 700.)),
            false,
            false,
            &[screens[1].frame],
            &[screens[0].frame],
            screens[0],
        );
        assert!(fit_frame_to_visible_frame(after_disconnect, screens[0].visible_frame).is_none());
    }

    #[test]
    fn ordinary_frame_after_hotkey_detects_neighbor_disconnect_and_ignores_order() {
        let (screens, _, _) = side_by_side_screens();
        let original = NSRect::new(NSPoint::new(200., 50.), NSSize::new(2200., 700.));
        let previous = [screens[0].frame, screens[1].frame];
        let after_disconnect = frame_after_display_topology_change(
            original,
            false,
            false,
            &previous,
            &[screens[0].frame],
            screens[0],
        );
        assert!(fit_frame_to_visible_frame(after_disconnect, screens[0].visible_frame).is_none());
        let reordered = frame_after_display_topology_change(
            original,
            false,
            false,
            &previous,
            &[screens[1].frame, screens[0].frame],
            screens[0],
        );
        assert!(nsrect_approx_eq(reordered, original, 0.));
    }

    #[test]
    fn hotkey_panel_display_refresh_uses_drawer_layout_not_mouse_selection() {
        let source = include_str!("window.rs");
        let refresh = production_method(source, "pub(crate) fn refresh_after_display_change(");
        assert!(refresh.contains("hotkey_panel_frame("));
        assert!(refresh.contains("inner.config.macos_global_hotkey_height_percent"));
        assert!(refresh.contains("frame_after_display_topology_change("));
        assert!(refresh.contains("!simple_fullscreen && !hotkey_panel"));
        assert!(!refresh.contains("hotkey_screen_geometry("));
    }

    #[test]
    fn hotkey_panel_layout_never_overwrites_ordinary_geometry() {
        let source = include_str!("window.rs");
        let restore = production_method(source, "fn restore_ordinary_window(");
        assert!(!restore.contains("setFrame_display_(frame, NO)"));
        assert!(restore.contains("ordinary_frame = NSWindow::frame(*self.window)"));
        assert!(restore.contains("frame_after_display_topology_change("));
        let terminate = production_method(source, "pub(crate) fn on_app_terminating(");
        assert!(terminate.contains("inner.restore_ordinary_window()"));
        let reveal = production_method(source, "pub(crate) fn show_global_hotkey_window(");
        assert!(reveal.contains("self.config.macos_global_hotkey_height_percent"));
        let persist = production_method(source, "fn persist_window_state(");
        assert!(persist.contains("is_hotkey_panel(window)"));
        let remember = production_method(source, "fn remember_last_closed_window_position(");
        assert!(remember.contains("is_hotkey_panel(window)"));
    }

    #[test]
    fn hotkey_panel_focus_loss_hides_only_a_visible_unfocused_panel() {
        assert!(should_hide_hotkey_on_focus_loss(
            true, false, true, None, true
        ));
        assert!(should_hide_hotkey_on_focus_loss(
            true,
            false,
            true,
            Some(true),
            true
        ));
        assert!(!should_hide_hotkey_on_focus_loss(
            true,
            false,
            true,
            Some(false),
            true
        ));
        assert!(!should_hide_hotkey_on_focus_loss(
            true, true, true, None, true
        ));
        assert!(!should_hide_hotkey_on_focus_loss(
            true, false, false, None, true
        ));
        assert!(!should_hide_hotkey_on_focus_loss(
            false, false, true, None, true
        ));
        let callback =
            production_method(include_str!("window.rs"), "extern \"C\" fn did_resign_key(");
        assert!(callback.contains("Connection::with_window_inner(this.window_id.get()"));
        assert!(callback.contains("should_hide_hotkey_on_focus_loss("));
        assert!(callback.contains("inner.hide_global_hotkey_window()"));
        assert!(!callback.contains("orderOut:"));
    }

    #[test]
    fn focus_regain_reverses_only_focus_loss_hiding_not_explicit_hotkey_hiding() {
        let frame = NSRect::new(NSPoint::new(0., 600.), NSSize::new(1000., 300.));
        let hide = HotkeySlide {
            resting_frame: frame,
            from: frame.origin,
            to: NSPoint::new(0., 1000.),
            started: Instant::now(),
            duration: HOTKEY_SLIDE_DURATION,
            showing: false,
            hide_on_focus_loss: true,
        };
        let current = NSPoint::new(0., 750.);
        assert!(hide.reverse_on_focus_gain(false, current).is_none());
        let reverse = hide.reverse_on_focus_gain(true, current).unwrap();
        assert!(reverse.showing);
        assert!(!reverse.hide_on_focus_loss);
        assert_eq!((reverse.from.x, reverse.from.y), (current.x, current.y));
        assert_eq!(
            (reverse.to.x, reverse.to.y),
            (frame.origin.x, frame.origin.y)
        );
        assert!(nsrect_approx_eq(reverse.resting_frame, frame, 0.));
        assert!(HotkeySlide {
            hide_on_focus_loss: false,
            ..hide
        }
        .reverse_on_focus_gain(true, current)
        .is_none());
        let source = include_str!("window.rs");
        let step = production_method(source, "fn step_hotkey_slide(");
        assert!(
            step.find("reverse_on_focus_gain").unwrap()
                < step.find("self.cancel_hotkey_slide()").unwrap()
        );
        let animate = production_method(source, "fn animate_hotkey_slide(");
        assert!(animate.contains("hide_on_focus_loss: false"));
        let resign = production_method(source, "extern \"C\" fn did_resign_key(");
        assert!(resign.contains("slide.hide_on_focus_loss = true"));
    }

    #[test]
    fn hotkey_panel_does_not_inherit_parked_simple_fullscreen_state() {
        let source = include_str!("window.rs");
        let fullscreen = production_method(source, "pub(crate) fn is_fullscreen(");
        assert!(fullscreen.contains("self.ordinary_window.is_some()"));
        let toggle = production_method(source, "fn toggle_fullscreen(&mut self)");
        assert!(
            toggle.find("self.show()").unwrap()
                < toggle
                    .find("if self.config.native_macos_fullscreen_mode")
                    .unwrap()
        );
        for signature in [
            "fn get_os_parameters(",
            "fn update_application_presentation(",
            "extern \"C\" fn did_resize(",
        ] {
            let method = production_method(source, signature);
            assert!(method.contains("!hotkey_panel"), "{}", signature);
        }
        let resize = production_method(source, "extern \"C\" fn did_resize(");
        assert!(resize.contains("&& !hotkey_panel\n"));
    }

    #[test]
    fn hotkey_uses_panel_without_changing_ordinary_window_class() {
        assert_eq!(
            get_hotkey_window_class().superclass().unwrap().name(),
            "NSPanel"
        );
        assert_eq!(get_window_class().superclass().unwrap().name(), "NSWindow");
    }

    #[test]
    fn hotkey_slide_moves_above_each_screen_and_reaches_exact_endpoints() {
        for screen in [
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(1728., 1117.)),
            NSRect::new(NSPoint::new(-1920., 1117.), NSSize::new(1920., 1080.)),
        ] {
            let frame = NSRect::new(screen.origin, NSSize::new(800., 600.));
            let point = |p: NSPoint| (p.x, p.y);
            let offscreen = hotkey_offscreen_origin(frame, screen);
            assert_eq!(offscreen.x, frame.origin.x);
            assert_eq!(offscreen.y, screen.origin.y + screen.size.height);
            let show = HotkeySlide {
                resting_frame: frame,
                from: offscreen,
                to: frame.origin,
                started: Instant::now(),
                duration: HOTKEY_SLIDE_DURATION,
                showing: true,
                hide_on_focus_loss: false,
            };
            assert_eq!(point(show.origin_at(Duration::ZERO)), point(offscreen));
            assert_eq!(
                point(show.origin_at(HOTKEY_SLIDE_DURATION)),
                point(frame.origin)
            );
            assert_eq!(
                point(show.origin_at(Duration::from_secs(1))),
                point(frame.origin)
            );
            let midway = show.origin_at(HOTKEY_SLIDE_DURATION / 2);
            assert_eq!(midway.y, (offscreen.y + frame.origin.y) / 2.);
            let hide = HotkeySlide {
                from: midway,
                to: offscreen,
                showing: false,
                ..show
            };
            // Reversal starts at the current position, not at either endpoint.
            assert_eq!(point(hide.origin_at(Duration::ZERO)), point(midway));
            assert_eq!(
                point(hide.origin_at(HOTKEY_SLIDE_DURATION)),
                point(offscreen)
            );
            assert!(nsrect_approx_eq(hide.resting_frame, frame, 0.));
        }
    }

    #[test]
    fn hotkey_toggle_reverses_an_inflight_hide_instead_of_hiding_again() {
        assert!(hotkey_should_hide(Some(true), true, true));
        assert!(!hotkey_should_hide(Some(false), true, true));
        assert!(hotkey_should_hide(None, true, true));
        assert!(!hotkey_should_hide(None, false, true));
        assert!(!hotkey_should_hide(None, true, false));
    }

    #[test]
    fn native_fullscreen_restores_hotkey_panel_before_appkit_toggle() {
        assert!(use_appkit_fullscreen_toggle(true, false));
        assert!(!use_appkit_fullscreen_toggle(true, true));
        assert!(!use_appkit_fullscreen_toggle(false, false));
        assert!(!use_appkit_fullscreen_toggle(false, true));
    }

    #[test]
    fn missing_titlebar_buttons_are_allowed_for_fullscreen_hotkey_panels() {
        set_titlebar_button_hidden(nil, YES);
        set_titlebar_button_hidden(nil, NO);
    }

    #[test]
    fn hotkey_behavior_does_not_conflict_with_primary_fullscreen() {
        let initial =
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
                | appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces;
        let behavior = hotkey_collection_behavior(initial, 11);
        assert!(!behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary
        ));
        assert!(!behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
        ));
        assert!(behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
        ));
        assert!(behavior.contains(
            appkit::NSWindowCollectionBehavior::NSWindowCollectionBehaviorMoveToActiveSpace
        ));
    }

    #[test]
    fn simple_fullscreen_exit_only_corrects_invalid_display_topology() {
        let source = include_str!("window.rs");
        let toggle = production_method(source, "fn toggle_simple_fullscreen(&mut self)");
        assert!(toggle.contains("frame_after_display_topology_change("));
        assert!(toggle.contains("fullscreen_screens"));
        assert!(toggle.contains("fullscreen_screens = screen_frames()"));
        let refresh = production_method(source, "pub(crate) fn refresh_after_display_change(");
        assert!(refresh.contains("frame_after_display_topology_change("));
    }

    #[test]
    fn windowed_frame_fits_remaining_display_after_disconnect() {
        let external = NSRect::new(NSPoint::new(-2560., 50.), NSSize::new(2400., 1400.));
        let builtin = ScreenGeometry {
            frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1440., 900.)),
            visible_frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1440., 875.)),
        };
        let adjusted = frame_after_display_change(external, false, false, builtin);
        assert!(nsrect_approx_eq(adjusted, builtin.visible_frame, 0.5));
    }

    #[test]
    fn filled_window_expands_to_taller_remaining_display() {
        // AppKit moved and centered the external screen's 1050pt-tall window,
        // but it did not expand it to the built-in screen's 1084pt work area.
        let migrated = NSRect::new(NSPoint::new(0., 17.), NSSize::new(1728., 1050.));
        let builtin = ScreenGeometry {
            frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1728., 1117.)),
            visible_frame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(1728., 1084.)),
        };
        let adjusted = frame_after_display_change(migrated, false, true, builtin);
        assert!(nsrect_approx_eq(adjusted, builtin.visible_frame, 0.5));
        let ordinary = frame_after_display_change(migrated, false, false, builtin);
        assert!(nsrect_approx_eq(ordinary, migrated, 0.5));
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
    fn frame_top_is_clamped_below_menu_bar() {
        // 1920x1080 screen with a 25pt menu bar: visible frame tops out at 1055.
        let visible = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1920.0, 1055.0));

        // Dragged so the title bar sits behind the menu bar: pinned back down.
        let frame = NSRect::new(NSPoint::new(100.0, 500.0), NSSize::new(800.0, 600.0));
        assert_eq!(clamp_frame_top_below_menu_bar(frame, visible), Some(455.0));

        // Fully visible window is untouched.
        let frame = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(800.0, 600.0));
        assert_eq!(clamp_frame_top_below_menu_bar(frame, visible), None);

        // Exactly at the menu bar boundary is untouched.
        let frame = NSRect::new(NSPoint::new(100.0, 455.0), NSSize::new(800.0, 600.0));
        assert_eq!(clamp_frame_top_below_menu_bar(frame, visible), None);

        // Hanging off the bottom stays allowed; only the top edge is pinned.
        let frame = NSRect::new(NSPoint::new(100.0, -400.0), NSSize::new(800.0, 600.0));
        assert_eq!(clamp_frame_top_below_menu_bar(frame, visible), None);
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
    register_window_class(WINDOW_CLS_NAME, class!(NSWindow), false)
}

fn get_hotkey_window_class() -> &'static Class {
    register_window_class("KakuHotkeyPanel", class!(NSPanel), true)
}

fn register_window_class(name: &str, base: &'static Class, panel: bool) -> &'static Class {
    Class::get(name).unwrap_or_else(|| {
        let mut cls = ClassDecl::new(name, base).expect("Unable to register Window class");

        extern "C" fn yes(_: &mut Object, _: Sel) -> BOOL {
            YES
        }

        extern "C" fn no(_: &mut Object, _: Sel) -> BOOL {
            NO
        }

        extern "C" fn redirect_toggle_fullscreen(this: &mut Object, _sel: Sel, sender: id) {
            let content_view: id = unsafe { msg_send![this, contentView] };
            if !content_view.is_null() {
                if let Some(window_view) = unsafe { WindowView::get_this(&*content_view) } {
                    let (window_id, use_native) = {
                        let inner = window_view.inner.borrow();
                        (inner.window_id, inner.config.native_macos_fullscreen_mode)
                    };
                    if use_appkit_fullscreen_toggle(use_native, is_hotkey_panel(this as id)) {
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
                if panel { no } else { yes } as extern "C" fn(&mut Object, Sel) -> BOOL,
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
    fn settle_windowed_screen_state(&self, window: id) {
        if window.is_null() || self.is_closing.get() || is_hotkey_panel(window) {
            return;
        }
        unsafe {
            if let Some(screen) = screen_geometry_for_window(window) {
                // Fullscreen can outlive a display change; rebase the frozen
                // snapshot once its final ordinary frame has been restored.
                self.windowed_screen.set(self.windowed_screen.get().settled(
                    NSWindow::frame(window),
                    screen_identifier_for_window(window),
                    screen,
                ));
            }
        }
    }

    fn refresh_windowed_screen_state(&self, window: id) {
        if window.is_null()
            || self.is_closing.get()
            || is_hotkey_panel(window)
            || self.simple_fullscreen_active.get()
            || self.simple_fullscreen_transition_active.get()
            || self.native_fullscreen_transition_active.get()
            || self.hotkey_slide.get().is_some()
        {
            return;
        }
        unsafe {
            if NSWindow::styleMask(window).contains(NSWindowStyleMask::NSFullScreenWindowMask) {
                return;
            }
            if let Some(screen) = screen_geometry_for_window(window) {
                let previous = self.windowed_screen.get();
                self.windowed_screen.set(previous.after_frame_change(
                    NSWindow::frame(window),
                    screen_identifier_for_window(window),
                    screen,
                    previous.screen_id.and_then(screen_geometry_for_identifier),
                ));
            }
        }
    }

    fn cancel_hotkey_slide(&self, window: id) {
        if window.is_null() {
            return;
        }
        if let Some(slide) = self.hotkey_slide.get() {
            self.hotkey_slide_generation
                .set(self.hotkey_slide_generation.get().wrapping_add(1));
            unsafe {
                let is_key: BOOL = msg_send![window, isKeyWindow];
                if slide.should_hide_when_cancelled(is_key == YES) {
                    let _: () = msg_send![window, orderOut: nil];
                }
                // Keep the guard set while settling, so callbacks cannot save
                // a transient animation frame as the user's window geometry.
                NSWindow::setFrame_display_(window, slide.resting_frame, NO);
            }
            self.hotkey_slide.set(None);
        }
    }

    fn dispatch_pending_hotkey_visibility(&self) {
        if self.pending_hotkey_visibility.get().is_none() {
            return;
        }
        // Run after the delegate callback releases its borrows. Read the intent
        // at execution time so rapid toggles or Dock reopening can supersede it.
        Connection::with_window_inner(self.window_id.get(), |inner| {
            let showing = WindowView::get_this(unsafe { &**inner.view }).and_then(|view| {
                if view.is_closing.get() || view.native_fullscreen_transition_active.get() {
                    None
                } else {
                    view.pending_hotkey_visibility.replace(None)
                }
            });
            if let Some(showing) = showing {
                log::debug!("applying deferred hotkey visibility={}", showing);
                if !inner.dedicated_hotkey {
                    if showing {
                        inner.show_ordinary_global_hotkey_window();
                    } else {
                        inner.hide_ordinary_global_hotkey_window();
                    }
                } else if showing {
                    inner.show_global_hotkey_window();
                } else {
                    inner.hide_global_hotkey_window();
                }
            }
            Ok(())
        });
    }

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

    extern "C" fn needs_panel_to_become_key(view: &mut Object, _sel: Sel) -> BOOL {
        // Nonactivating panels consult the hit view, not just canBecomeKeyWindow.
        let window: id = unsafe { msg_send![view, window] };
        (!window.is_null() && is_hotkey_panel(window)) as BOOL
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
            let hotkey_panel = inner.window.as_ref().is_some_and(|window| {
                let window = window.load();
                is_hotkey_panel(*window)
            });
            is_simple_full_screen = !hotkey_panel && inner.fullscreen.is_some();
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
            // Recheck on the next turn: a hotkey or Dock reopen may supersede
            // this focus loss before the callback releases its native borrows.
            Connection::with_window_inner(this.window_id.get(), |inner| {
                let showing = WindowView::get_this(unsafe { &**inner.view }).and_then(|view| {
                    view.pending_hotkey_visibility
                        .get()
                        .or_else(|| view.hotkey_slide.get().map(|slide| slide.showing))
                });
                if should_hide_hotkey_on_focus_loss(
                    inner.ordinary_window.is_some(),
                    inner.is_key_window(),
                    inner.is_visible(),
                    showing,
                    inner.config.macos_global_hotkey_hide_on_focus_loss,
                ) {
                    log::debug!("global hotkey panel lost keyboard focus; sliding out");
                    inner.hide_global_hotkey_window();
                    if let Some(view) = WindowView::get_this(unsafe { &**inner.view }) {
                        if let Some(mut slide) =
                            view.hotkey_slide.get().filter(|slide| !slide.showing)
                        {
                            slide.hide_on_focus_loss = true;
                            view.hotkey_slide.set(Some(slide));
                        }
                    }
                }
                Ok(())
            });
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
    // Reads the per-view cache to avoid borrowing Inner or acquiring the
    // config Mutex on every compositor call (60-120Hz).
    extern "C" fn is_opaque(this: &Object, _sel: Sel) -> BOOL {
        Self::get_this(this)
            .map(|this| this.opaque.get())
            .unwrap_or(NO)
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
        // Safe because kakuPerformKeyAssignment: is only used with RepresentedItem
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
            Some(RepresentedItem::KeyAssignmentForPane { action, pane_id }) => {
                if let Some(this) = Self::get_this(this) {
                    // As above, release the RefCell borrow before dispatch so the
                    // native menu action remains safe under AppKit re-entry.
                    if let Ok(inner) = this.inner.try_borrow() {
                        let events = inner.events.clone();
                        drop(inner);
                        events
                            .dispatch(WindowEvent::PerformKeyAssignmentForPane { action, pane_id });
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
            OS_PARAMETER_VIEWS.with(|views| {
                views.borrow_mut().remove(&view.window_id.get());
            });
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
            let window: id = msg_send![this as id, window];
            if let Some(view) = Self::get_this(this) {
                view.refresh_windowed_screen_state(window);
            }
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
        let window_origin;
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
            // Capture the true content origin: inferring it in the gui layer
            // as screen_coords - coords mixes the primary screen's scale with
            // the window screen's backing scale and teleports the window when
            // a drag starts on a display with a different scale (#456).
            let ns_window: id = msg_send![view, window];
            window_origin = window_position(ns_window).unwrap_or_else(|| {
                ScreenPoint::new(
                    cartesian_to_screen_point(screen_coords).x - coords.x as isize,
                    cartesian_to_screen_point(screen_coords).y - coords.y as isize,
                )
            });
        }
        let platform_click_count = match &kind {
            MouseEventKind::Press(_) | MouseEventKind::Release(_) => {
                unsafe { nsevent.clickCount() }.min(255) as u8
            }
            _ => 0,
        };
        let event = MouseEvent {
            kind,
            coords: Point::new(coords.x as isize, coords.y as isize),
            screen_coords: cartesian_to_screen_point(screen_coords),
            window_origin,
            mouse_buttons,
            modifiers,
            platform_click_count,
        };

        if let Some(myself) = Self::get_this(this) {
            myself.dispatch_event(WindowEvent::MouseEvent(event));
        }
    }

    extern "C" fn mouse_up(this: &mut Object, _sel: Sel, nsevent: id) {
        // A press that armed a maximized-window native drag (see mouse_down) but
        // released without moving is a plain click: disarm so it stays a no-op
        // instead of leaking into the next drag. #414
        ARMED_MAXIMIZED_NATIVE_DRAG.with(|flag| flag.set(false));
        Self::mouse_common(this, nsevent, MouseEventKind::Release(MousePress::Left));
    }

    extern "C" fn mouse_down(this: &mut Object, _sel: Sel, nsevent: id) {
        // Check if we're in fullscreen mode - if so, disable dragging
        let window: id = unsafe { msg_send![this as id, window] };
        let in_fullscreen = if let Some(view) = Self::get_this(this) {
            let simple_fullscreen =
                !is_hotkey_panel(window) && view.inner.borrow().fullscreen.is_some();
            let native_fullscreen = unsafe {
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
        PENDING_DRAG_MOVE_FROM_MAXIMIZED.with(|flag| flag.set(false));
        ARMED_MAXIMIZED_NATIVE_DRAG.with(|flag| flag.set(false));
        Self::mouse_common(this, nsevent, MouseEventKind::Press(MousePress::Left));

        // App layer may call request_drag_move() in mouse_common to set the flag.
        // Skip in fullscreen. For a non-maximized window we start the native drag
        // synchronously here. For a maximized/zoomed window a bare single click and
        // the start of a drag look identical, and performWindowDragWithEvent: can be
        // interpreted as a snap/maximize gesture (#414); arm it instead and start the
        // native drag from the first mouseDragged: so a plain click is a no-op while a
        // real drag still pulls the maximized window off the top (#428).
        let pending_drag = PENDING_DRAG_MOVE.with(|flag| flag.replace(false));
        let pending_drag_from_maximized =
            PENDING_DRAG_MOVE_FROM_MAXIMIZED.with(|flag| flag.replace(false));
        if pending_drag && !in_fullscreen {
            unsafe {
                let window: id = msg_send![this as id, window];
                if window != nil {
                    if let Some(view) = Self::get_this(this) {
                        view.cancel_hotkey_slide(window);
                    }
                    let is_zoomed: bool = msg_send![window, isZoomed];
                    let fills_visible_frame = window_fills_visible_frame(window);
                    match requested_window_drag_action(
                        in_fullscreen,
                        is_zoomed,
                        fills_visible_frame,
                        pending_drag_from_maximized,
                    ) {
                        RequestedDragAction::PerformNow => {
                            let () = msg_send![window, performWindowDragWithEvent: nsevent];
                        }
                        RequestedDragAction::DeferUntilDrag => {
                            ARMED_MAXIMIZED_NATIVE_DRAG.with(|flag| flag.set(true));
                        }
                        RequestedDragAction::None => {}
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
        // If mouse_down armed a maximized-window native drag, the pointer has now
        // actually moved, so this is a genuine drag rather than a click: hand off to
        // performWindowDragWithEvent: to pull the window off the top (#428). Deferring
        // to real movement is what keeps a bare single click from snapping/maximizing
        // the window (#414). Only mouseDragged: (button held) should consume the arm;
        // a plain mouseMoved: must not.
        if _sel == sel!(mouseDragged:) {
            let armed = ARMED_MAXIMIZED_NATIVE_DRAG.with(|flag| flag.replace(false));
            if armed {
                unsafe {
                    let window: id = msg_send![this as id, window];
                    if window != nil {
                        let () = msg_send![window, performWindowDragWithEvent: nsevent];
                    }
                }
                return;
            }
        }
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
            // Same-DPI moves may not cause windowDidResize. Refresh after the
            // delegate releases its borrows, without moving the native window.
            Connection::with_window_inner(this.window_id.get(), |inner| {
                if let Some(view) = Self::get_this(unsafe { &**inner.view }) {
                    if view.is_closing.get() {
                        return Ok(());
                    }
                    view.inner.borrow_mut().screen_changed = true;
                }
                inner.dispatch_resize_event();
                Ok(())
            });
        }
    }

    extern "C" fn will_start_live_resize(this: &mut Object, _sel: Sel, _notification: id) {
        let window: id = unsafe { msg_send![this as id, window] };
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.cancel_hotkey_slide(window);
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
            this.dispatch_pending_hotkey_visibility();
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
        let window: id = unsafe { msg_send![this, window] };
        if let Some(this) = Self::get_this(this) {
            if this.is_closing.get() {
                return;
            }
            this.native_fullscreen_transition_active.set(false);
            this.native_fullscreen_target.set(None);
            this.transition_hide_until.set(None);
            this.settle_windowed_screen_state(window);
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

            // Complete the deferred hide requested in order_out(). Consume the
            // flag unconditionally: there is no retry trigger on this path, so
            // leaving it set would not heal a failed hide and could orderOut:
            // a window the user later brings back to fullscreen. If orderOut:
            // cannot run (inner transiently borrowed, or the window pointer is
            // gone), log it rather than silently dropping the request.
            if this.order_out_on_fullscreen_exit.replace(false) {
                let mut hidden = false;
                if let Ok(inner) = this.inner.try_borrow() {
                    if let Some(window) = inner.window.as_ref() {
                        let window = window.load();
                        if !window.is_null() {
                            unsafe {
                                let () = msg_send![*window, orderOut: nil];
                            }
                            hidden = true;
                        }
                    }
                }
                if !hidden {
                    log::warn!(
                        "fullscreen exit could not complete deferred orderOut:; \
                         window may remain visible"
                    );
                }
            }
            this.dispatch_pending_hotkey_visibility();
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
            let hotkey_panel = inner.window.as_ref().is_some_and(|window| {
                let window = window.load();
                is_hotkey_panel(*window)
            });
            let is_full_screen = !hotkey_panel
                && if native_transition_active {
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

            if !is_full_screen
                && !hotkey_panel
                && !native_transition_active
                && this.hotkey_slide.get().is_none()
            {
                if let Some(window) = inner.window.as_ref().map(|w| w.load()) {
                    this.refresh_windowed_screen_state(*window);
                }
            }

            // Note: isZoomed can falsely return YES during screen changes.
            // When screen_changed is true, carry forward the prior MAXIMIZED
            // state instead of querying isZoomed (which may give stale results
            // for the old screen frame). This avoids both false positives and
            // the prior bug where a truly maximized window lost its state
            // permanently after moving to another screen.
            // <https://github.com/wezterm/wezterm/issues/3503>
            // <https://github.com/tw93/Kaku/issues/131>
            let is_zoomed = !hotkey_panel
                && !this.windowed_screen.get().max_invalidated_by_move
                && if screen_changed {
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
                    nswindow_level_to_window_level(level, hotkey_panel)
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

            let mut window_state = screen_state | level_state;
            if hotkey_panel {
                window_state |= WindowState::HOTKEY_WINDOW;
            }
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
                if screen_changed || this.deferred_screen_resize.is_pending() {
                    // Defer the resize dispatch to the next run-loop turn when the resize
                    // was triggered by a display reconfiguration (lock-screen return,
                    // monitor connect/disconnect). Dispatching synchronously here means
                    // we are still inside _NSCGSDisplayConfigurationDidReconfigureNotificationHandler
                    // -> _setFrameCommon -> postNotification, and re-entering AppKit or
                    // doing heavy render work in that context causes a main-thread hang.
                    // Recovery can resize again before that turn. Keep the latest
                    // snapshot, including the screen-change reason, until dispatch.
                    let resize = ScreenResize {
                        dimensions: Dimensions {
                            pixel_width: width as usize,
                            pixel_height: height as usize,
                            dpi,
                        },
                        window_state,
                        live_resizing,
                    };
                    if this.deferred_screen_resize.update(resize) {
                        let window_id = inner.window_id;
                        Connection::with_window_inner(window_id, move |inner| {
                            if let Some(window_view) =
                                WindowView::get_this(unsafe { &**inner.view })
                            {
                                if window_view.is_closing.get() {
                                    return Ok(());
                                }
                                if let Some(resize) = window_view.deferred_screen_resize.take() {
                                    window_view.dispatch_event(WindowEvent::Resized {
                                        dimensions: resize.dimensions,
                                        window_state: resize.window_state,
                                        live_resizing: resize.live_resizing,
                                        screen_changed: true,
                                    });
                                }
                            }
                            Ok(())
                        });
                    }
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
            } else {
                // A fullscreen transition may suppress this snapshot, but the
                // next accepted resize must still refresh display-dependent layout.
                let pending = this.deferred_screen_resize.take();
                inner.screen_changed |= screen_changed || pending.is_some();
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
        this: &mut Object,
        _sel: Sel,
        window: id,
        default_frame: NSRect,
    ) -> NSRect {
        if let Some(view) = Self::get_this(this) {
            view.cancel_hotkey_slide(window);
        }
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
                        .window
                        .as_ref()
                        .is_some_and(|window| is_hotkey_panel(*window.load()))
                        || inner
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
            opaque: Cell::new(gl_layer_is_opaque(
                inner.borrow().config.window_background_opacity,
            )),
            window_id: Cell::new(inner.borrow().window_id),
            windowed_screen: Cell::new(WindowedScreenState::default()),
            hotkey_slide: Cell::new(None),
            hotkey_slide_generation: Cell::new(0),
            pending_hotkey_visibility: Cell::new(None),
            simple_fullscreen_active: Cell::new(false),
            simple_fullscreen_transition_active: Cell::new(false),
            transition_hide_until: Cell::new(None),
            display_change_opengl_present_until: Cell::new(None),
            native_fullscreen_transition_active: Cell::new(false),
            native_fullscreen_target: Cell::new(None),
            native_fullscreen_transition_start: Cell::new(None),
            resize_retry_scheduled: Cell::new(false),
            deferred_screen_resize: DeferredScreenResize::default(),
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
                sel!(needsPanelToBecomeKey),
                Self::needs_panel_to_become_key as extern "C" fn(&mut Object, Sel) -> BOOL,
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
