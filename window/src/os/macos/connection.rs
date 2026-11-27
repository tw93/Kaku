// let () = msg_send! is a common pattern for objc
#![allow(clippy::let_unit_value)]

use super::nsstring_to_str;
use super::window::WindowInner;
use crate::connection::ConnectionOps;
use crate::os::macos::app::{
    create_app_delegate, flush_pending_service_opens, request_app_termination,
    sync_global_hotkey_registration,
};
use crate::screen::{ScreenInfo, Screens};
use crate::spawn::*;
use crate::Appearance;
use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicyRegular, NSScreen};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSArray, NSInteger, NSPoint, NSString, NSUInteger};
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use objc::runtime::{Object, BOOL, YES};
use objc::*;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuitOrigin {
    AppKitShouldTerminate,
    AppScopeQuitApplication,
    WindowScopeQuitApplication,
    ConfirmQuitOverlay,
}

pub fn request_terminate(origin: QuitOrigin) {
    request_app_termination(origin, None);
}

pub struct Connection {
    ns_app: id,
    pub(crate) windows: RefCell<HashMap<usize, Rc<RefCell<WindowInner>>>>,
    pub(crate) next_window_id: AtomicUsize,
    pub(crate) gl_connection: RefCell<Option<Rc<crate::egl::GlConnection>>>,
}

impl Connection {
    pub(crate) fn create_new() -> anyhow::Result<Self> {
        // Ensure that the SPAWN_QUEUE is created; it will have nothing
        // to run right now.
        SPAWN_QUEUE.run();

        unsafe {
            let ns_app = NSApp();
            ns_app.setActivationPolicy_(NSApplicationActivationPolicyRegular);

            let delegate = create_app_delegate();
            let () = msg_send![ns_app, setDelegate: delegate];

            let conn = Self {
                ns_app,
                windows: RefCell::new(HashMap::new()),
                next_window_id: AtomicUsize::new(1),
                gl_connection: RefCell::new(None),
            };
            Ok(conn)
        }
    }

    pub(crate) fn next_window_id(&self) -> usize {
        self.next_window_id
            .fetch_add(1, ::std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn window_by_id(&self, window_id: usize) -> Option<Rc<RefCell<WindowInner>>> {
        self.windows.borrow().get(&window_id).map(Rc::clone)
    }

    pub(crate) fn with_window_inner<
        R,
        F: FnOnce(&mut WindowInner) -> anyhow::Result<R> + Send + 'static,
    >(
        window_id: usize,
        f: F,
    ) -> promise::Future<R>
    where
        R: Send + 'static,
    {
        let mut prom = promise::Promise::new();
        let future = prom.get_future().unwrap();
        promise::spawn::spawn_into_main_thread(async move {
            let result = match Connection::get() {
                Some(conn) => match conn.window_by_id(window_id) {
                    Some(handle) => {
                        let mut inner = handle.borrow_mut();
                        f(&mut inner)
                    }
                    None => Err(anyhow::anyhow!("invalid window id {}", window_id)),
                },
                None => Err(anyhow::anyhow!("window connection is not initialized")),
            };
            prom.result(result);
        })
        .detach();

        future
    }
}

/// `/System/Library/CoreServices/SystemVersion.plist`
#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct SoftwareVersion {
    product_build_version: String,
    product_user_visible_version: String,
    product_name: String,
}

impl SoftwareVersion {
    fn load() -> anyhow::Result<Self> {
        let vers: Self = plist::from_file("/System/Library/CoreServices/SystemVersion.plist")?;
        Ok(vers)
    }
}

const NO_ERR: i32 = 0;
/// `kLSRolesAll` from LaunchServices - matches all handler roles.
const KLS_ROLES_ALL: u32 = !0;
const KAKU_BUNDLE_IDENTIFIER: &str = "fun.tw93.kaku";

fn set_default_role_handler_for_content_type(
    content_type: &str,
    role_mask: u32,
    bundle_id: &CFString,
) -> anyhow::Result<()> {
    let content_type = CFString::new(content_type);
    let status = unsafe {
        LSSetDefaultRoleHandlerForContentType(
            content_type.as_concrete_TypeRef(),
            role_mask,
            bundle_id.as_concrete_TypeRef(),
        )
    };
    if status != NO_ERR {
        anyhow::bail!(
            "LSSetDefaultRoleHandlerForContentType({}, role={:#x}) failed: {}",
            content_type.to_string(),
            role_mask,
            status
        );
    }
    Ok(())
}

impl ConnectionOps for Connection {
    fn name(&self) -> String {
        use std::sync::OnceLock;
        static CACHED_NAME: OnceLock<String> = OnceLock::new();
        CACHED_NAME
            .get_or_init(|| {
                if let Ok(vers) = SoftwareVersion::load() {
                    format!(
                        "{} {} ({})",
                        vers.product_name,
                        vers.product_user_visible_version,
                        vers.product_build_version
                    )
                } else {
                    "macOS".to_string()
                }
            })
            .clone()
    }

    fn default_dpi(&self) -> f64 {
        if let Ok(screens) = self.screens() {
            screens.active.effective_dpi.unwrap_or(crate::DEFAULT_DPI)
        } else {
            crate::DEFAULT_DPI
        }
    }

    fn terminate_message_loop(&self) {
        unsafe {
            // bounce via an event callback to encourage stop to apply
            // to the correct level of run loop
            promise::spawn::spawn_into_main_thread(async move {
                let () = msg_send![NSApp(), stop: nil];
                // Generate a UI event so that the run loop breaks out
                // after receiving the stop
                let () = msg_send![NSApp(), abortModal];
            })
            .detach();
        }
    }

    fn get_appearance(&self) -> Appearance {
        let name = unsafe {
            let appearance: id = msg_send![self.ns_app, effectiveAppearance];
            nsstring_to_str(msg_send![appearance, name])
        };
        log::debug!("NSAppearanceName is {name}");
        match name {
            "NSAppearanceNameVibrantDark" | "NSAppearanceNameDarkAqua" => Appearance::Dark,
            "NSAppearanceNameVibrantLight" | "NSAppearanceNameAqua" => Appearance::Light,
            "NSAppearanceNameAccessibilityHighContrastVibrantLight"
            | "NSAppearanceNameAccessibilityHighContrastAqua" => Appearance::LightHighContrast,
            "NSAppearanceNameAccessibilityHighContrastVibrantDark"
            | "NSAppearanceNameAccessibilityHighContrastDarkAqua" => Appearance::DarkHighContrast,
            _ => {
                log::warn!("Unknown NSAppearanceName {name}, assume Light");
                Appearance::Light
            }
        }
    }

    fn run_message_loop(&self) -> anyhow::Result<()> {
        unsafe {
            self.ns_app.run();
        }
        self.windows.borrow_mut().clear();
        Ok(())
    }

    fn hide_application(&self) {
        unsafe {
            let () = msg_send![self.ns_app, hide: self.ns_app];
        }
    }

    fn beep(&self) {
        unsafe {
            NSBeep();
        }
    }

    fn set_dock_badge(&self, label: Option<&str>) {
        unsafe {
            let app = NSApp();
            let dock_tile: id = msg_send![app, dockTile];
            // Keep StrongPtr alive during the msg_send call
            let ns_label_ptr = label.map(super::nsstring);
            let ns_label: id = ns_label_ptr.as_ref().map_or(nil, |p| **p);
            let () = msg_send![dock_tile, setBadgeLabel: ns_label];
            // Force refresh the dock tile display
            let () = msg_send![dock_tile, display];
        }
    }

    fn request_attention(&self, until_focused: bool) {
        // NSCriticalRequest (0) bounces until Kaku is activated,
        // NSInformationalRequest (10) bounces once. macOS ignores
        // both while Kaku is the active app.
        let request_type: NSUInteger = if until_focused { 0 } else { 10 };
        unsafe {
            let _: NSInteger = msg_send![NSApp(), requestUserAttention: request_type];
        }
    }

    fn alert(&self, title: &str, message: &str) {
        unsafe {
            let alert: id = msg_send![class!(NSAlert), alloc];
            let alert: id = msg_send![alert, init];
            let () = msg_send![alert, setMessageText: *super::nsstring(title)];
            let () = msg_send![alert, setInformativeText: *super::nsstring(message)];
            let () = msg_send![alert, addButtonWithTitle: *super::nsstring("OK")];
            let () = msg_send![alert, runModal];
        }
    }

    fn confirm(&self, title: &str, message: &str, action_label: &str) -> bool {
        unsafe {
            let alert: id = msg_send![class!(NSAlert), alloc];
            let alert: id = msg_send![alert, init];
            let () = msg_send![alert, setMessageText: *super::nsstring(title)];
            let () = msg_send![alert, setInformativeText: *super::nsstring(message)];

            let () = msg_send![alert, addButtonWithTitle: *super::nsstring(action_label)];
            let () = msg_send![alert, addButtonWithTitle: *super::nsstring("Cancel")];

            let result: NSInteger = msg_send![alert, runModal];
            // NSAlertFirstButtonReturn is 1000
            result == 1000
        }
    }

    fn set_default_terminal(&self) -> anyhow::Result<()> {
        let bundle_id = CFString::new(KAKU_BUNDLE_IDENTIFIER);

        // Match the shell-related document types Kaku declares in Info.plist.
        let shell_content_types = [
            "public.unix-executable",
            "public.script",
            "public.shell-script",
            "public.bash-script",
            "public.zsh-script",
            "com.apple.terminal.shell-script",
        ];

        for content_type in shell_content_types {
            set_default_role_handler_for_content_type(content_type, KLS_ROLES_ALL, &bundle_id)?;
        }

        Ok(())
    }

    fn is_default_terminal(&self) -> bool {
        kaku_is_default_terminal()
    }

    fn flush_pending_service_events(&self) {
        flush_pending_service_opens();
    }

    fn sync_global_hotkey(&self) {
        sync_global_hotkey_registration();
    }

    fn screens(&self) -> anyhow::Result<Screens> {
        let mut by_name = HashMap::new();
        let mut virtual_rect = euclid::rect(0, 0, 0, 0);

        let screens = unsafe { NSScreen::screens(nil) };
        for idx in 0..unsafe { screens.count() } {
            let screen = unsafe { screens.objectAtIndex(idx) };
            let screen = nsscreen_to_screen_info(screen);
            virtual_rect = virtual_rect.union(&screen.rect);
            by_name.insert(screen.name.clone(), screen);
        }

        // The screen with the menu bar is always index 0
        let main = nsscreen_to_screen_info(unsafe { screens.objectAtIndex(0) });

        // The active screen is known as the "main" screen in macOS
        let active = nsscreen_to_screen_info(unsafe { NSScreen::mainScreen(nil) });

        Ok(Screens {
            by_name,
            active,
            main,
            virtual_rect,
        })
    }
}

// NOTE: LSSetDefaultRoleHandlerForContentType is deprecated since macOS 12 (Monterey).
// It still works as of macOS 15 but may be removed in a future release.
// Track https://developer.apple.com/documentation/coreservices for replacements.
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSSetDefaultRoleHandlerForContentType(
        in_content_type: CFStringRef,
        in_role: u32,
        in_handler_bundle_id: CFStringRef,
    ) -> i32;

    fn LSCopyDefaultRoleHandlerForContentType(
        in_content_type: CFStringRef,
        in_role: u32,
    ) -> CFStringRef;
}

fn kaku_is_default_terminal() -> bool {
    let content_type = CFString::new("public.unix-executable");
    let handler = unsafe {
        LSCopyDefaultRoleHandlerForContentType(content_type.as_concrete_TypeRef(), KLS_ROLES_ALL)
    };
    if handler.is_null() {
        return false;
    }
    let handler_str = unsafe { CFString::wrap_under_create_rule(handler) };
    handler_str.to_string() == KAKU_BUNDLE_IDENTIFIER
}

pub fn nsscreen_to_screen_info(screen: *mut Object) -> ScreenInfo {
    let frame = unsafe { NSScreen::frame(screen) };
    let backing_frame = unsafe { NSScreen::convertRectToBacking_(screen, frame) };
    // rect is expressed in the canonical ScreenPoint space (primary-screen
    // scale, y-down, origin at the primary screen's top left) so that it can
    // be combined directly with set_window_position / window_position.
    let top_left = super::window::cartesian_to_screen_point(NSPoint::new(
        frame.origin.x,
        frame.origin.y + frame.size.height,
    ));
    let bottom_right = super::window::cartesian_to_screen_point(NSPoint::new(
        frame.origin.x + frame.size.width,
        frame.origin.y,
    ));
    let rect = euclid::rect(
        top_left.x,
        top_left.y,
        bottom_right.x - top_left.x,
        bottom_right.y - top_left.y,
    );
    let has_name: BOOL = unsafe { msg_send!(screen, respondsToSelector: sel!(localizedName)) };
    let name = if has_name == YES {
        unsafe { nsstring_to_str(msg_send!(screen, localizedName)) }.to_string()
    } else {
        format!(
            "{}x{}@{},{}",
            backing_frame.size.width,
            backing_frame.size.height,
            backing_frame.origin.x,
            backing_frame.origin.y
        )
    };

    let has_max_fps: BOOL =
        unsafe { msg_send!(screen, respondsToSelector: sel!(maximumFramesPerSecond)) };
    let max_fps = if has_max_fps == YES {
        let max_fps: NSInteger = unsafe { msg_send!(screen, maximumFramesPerSecond) };
        Some(max_fps as usize)
    } else {
        None
    };

    let scale = backing_frame.size.width / frame.size.width;

    let config = config::configuration();
    let effective_dpi = if let Some(dpi) = config.dpi_by_screen.get(&name).copied() {
        Some(dpi)
    } else if let Some(dpi) = config.dpi {
        Some(dpi)
    } else {
        Some(crate::DEFAULT_DPI * scale)
    };

    ScreenInfo {
        name,
        rect,
        scale,
        max_fps,
        effective_dpi,
    }
}

pub fn query_appearance_early() -> Appearance {
    unsafe {
        let defaults: id = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let key = cocoa::foundation::NSString::alloc(nil).init_str("AppleInterfaceStyle");
        let value: id = msg_send![defaults, stringForKey: key];
        if value == nil {
            return Appearance::Light;
        }
        let style = nsstring_to_str(value);
        if style == "Dark" {
            Appearance::Dark
        } else {
            Appearance::Light
        }
    }
}

extern "C" {
    fn NSBeep();
}
