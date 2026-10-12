use crate::connection::ConnectionOps;
use crate::macos::menu::RepresentedItem;
use crate::macos::{nsstring, nsstring_to_str};
use crate::menu::{Menu, MenuItem};
use crate::{ApplicationEvent, Connection, KeyCode, Modifiers};
use cocoa::appkit::{
    CGFloat, NSApp, NSApplicationTerminateReply, NSEventModifierFlags, NSFilenamesPboardType,
    NSStringPboardType,
};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSInteger, NSRect, NSUInteger};
use config::keyassignment::KeyAssignment;
use config::{MacosGlobalHotkeyMode, WindowCloseConfirmation};
use core_foundation::base::{CFTypeID, TCFType};
use core_foundation::data::{CFData, CFDataGetBytePtr, CFDataRef};
use core_foundation::string::{CFStringRef, UniChar};
use core_foundation::{declare_TCFType, impl_TCFType};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, BOOL, NO, YES};
use objc::*;
use std::cell::{Cell, RefCell};
use std::convert::TryFrom;
use std::ffi::c_void;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

use super::connection::QuitOrigin;
use super::keycodes::{layout_printable_vkeys, phys_to_vkey};

const CLS_NAME: &str = "KakuAppDelegate";

type OSStatus = i32;
type EventHotKeyRef = *mut c_void;
type EventTargetRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type UniCharCount = std::os::raw::c_ulong;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

const NO_ERR: OSStatus = 0;
const K_EVENT_CLASS_KEYBOARD: u32 = 0x6b657962;
const K_EVENT_HOT_KEY_PRESSED: u32 = 6;

const HOTKEY_SIGNATURE: u32 = u32::from_be_bytes(*b"KAKU");
const HOTKEY_ID: u32 = 1;

const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const CONTROL_KEY: u32 = 1 << 12;
const CMD_KEY: u32 = 1 << 8;
#[allow(non_upper_case_globals)]
const kUCKeyActionDisplay: u16 = 3;

#[repr(C)]
pub struct __InputSource {
    _dummy: i32,
}
type InputSourceRef = *const __InputSource;

declare_TCFType!(InputSource, InputSourceRef);
impl_TCFType!(InputSource, InputSourceRef, TISInputSourceGetTypeID);

#[repr(C)]
struct UCKeyboardLayout {
    _dummy: i32,
}

unsafe extern "C" {
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus,
        num_types: u32,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> OSStatus;
    fn RegisterEventHotKey(
        hot_key_code: u32,
        hot_key_modifiers: u32,
        hot_key_id: EventHotKeyID,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OSStatus;
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn TISInputSourceGetTypeID() -> CFTypeID;
    fn TISCopyCurrentKeyboardInputSource() -> InputSourceRef;
    fn TISGetInputSourceProperty(source: InputSourceRef, property_key: CFStringRef) -> CFDataRef;
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    fn UCKeyTranslate(
        layout: *const UCKeyboardLayout,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: UniCharCount,
        actual_string_length: *mut UniCharCount,
        unicode_string: *mut UniChar,
    ) -> u32;
    fn LMGetKbdType() -> u8;
}

thread_local! {
    static LAST_OPEN_UNTITLED_SPAWN: RefCell<Option<Instant>> = RefCell::new(None);
    static HOTKEY_WINDOW_ID: Cell<Option<usize>> = Cell::new(None);
}

lazy_static::lazy_static! {
    static ref PENDING_SERVICE_OPENS: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());
    static ref PENDING_TTY_ACTIVATIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static ref PENDING_PANE_ID_ACTIVATIONS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    static ref PENDING_TAB_ID_ACTIVATIONS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    static ref LAST_SERVICE_OPEN_REQUEST: Mutex<Option<Instant>> = Mutex::new(None);
    static ref GLOBAL_HOTKEY_STATE: Mutex<GlobalHotKeyState> =
        Mutex::new(GlobalHotKeyState::default());
}
// macOS can emit applicationOpenUntitledFile twice while no window has
// materialized yet; keep a wider debounce to avoid duplicate SpawnWindow work.
const OPEN_UNTITLED_SPAWN_DEBOUNCE: Duration = Duration::from_millis(1200);
// Guard against startup races: when launched with an explicit file/folder to open,
// macOS may still emit applicationOpenUntitledFile and cause an extra “~/” tab.
// Use 2 seconds to balance startup race protection vs. user intent for new windows.
const OPEN_UNTITLED_AFTER_SERVICE_OPEN_GUARD: Duration = Duration::from_secs(2);
const DISPLAY_CHANGE_RETRY_DELAY: Duration = Duration::from_millis(100);
const DISPLAY_CHANGE_MAX_RETRIES: usize = 5;
static OPEN_UNTITLED_SPAWN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Set to `true` when the system is about to sleep (`NSWorkspaceWillSleepNotification`).
/// Cleared when `NSWorkspaceScreensDidWakeNotification` fires and the display-change
/// defer is armed. Checked by `should_defer_flush_buffer` to prevent flushing a stale
/// OpenGL surface during the gap between SkyLight's low-level wake callback and the
/// higher-level workspace notification.
static SYSTEM_SLEEPING: AtomicBool = AtomicBool::new(false);

pub fn is_system_sleeping() -> bool {
    SYSTEM_SLEEPING.load(Ordering::Acquire)
}

fn app_window_context() -> (usize, bool) {
    let Some(conn) = Connection::get() else {
        return (0, false);
    };

    let windows = conn.windows.borrow();
    let mut any_fullscreen = false;
    for window in windows.values() {
        if let Ok(mut inner) = window.try_borrow_mut() {
            if inner.is_fullscreen() {
                any_fullscreen = true;
                break;
            }
        }
    }

    (windows.len(), any_fullscreen)
}

fn log_quit_request(origin: QuitOrigin, detail: Option<&str>) {
    let (window_count, has_fullscreen) = app_window_context();
    let detail = detail.unwrap_or("none");

    log::warn!(
        "quit requested origin={origin:?} detail={detail} window_count={window_count} has_fullscreen={has_fullscreen}"
    );
}

pub fn request_app_termination(origin: QuitOrigin, detail: Option<&str>) {
    log_quit_request(origin, detail);

    if let Some(conn) = Connection::get() {
        conn.terminate_message_loop();
    } else {
        log::warn!(
            "Cannot terminate message loop for {origin:?}: GUI connection is not initialized"
        );
    }
}

fn note_service_open_request() {
    *LAST_SERVICE_OPEN_REQUEST.lock().unwrap() = Some(Instant::now());
    // Cancel any pending deferred untitled spawn from applicationOpenUntitledFile.
    OPEN_UNTITLED_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
}

fn should_suppress_open_untitled_spawn() -> bool {
    if !PENDING_SERVICE_OPENS.lock().unwrap().is_empty() {
        return true;
    }

    let now = Instant::now();
    LAST_SERVICE_OPEN_REQUEST
        .lock()
        .unwrap()
        .map(|ts| now.duration_since(ts) < OPEN_UNTITLED_AFTER_SERVICE_OPEN_GUARD)
        .unwrap_or(false)
}

fn schedule_open_untitled_spawn() {
    // Defer to the next main-thread turn (no fixed sleep), so a near-simultaneous
    // service open request can cancel this spawn without adding startup latency
    // for normal users.
    let sequence = OPEN_UNTITLED_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
    promise::spawn::spawn_into_main_thread(async move {
        if OPEN_UNTITLED_SPAWN_SEQUENCE.load(Ordering::SeqCst) != sequence {
            return;
        }

        if should_suppress_open_untitled_spawn() {
            log::debug!(
                "Skipping deferred applicationOpenUntitledFile spawn because a service \
                 open request was received"
            );
            return;
        }

        let Some(conn) = Connection::get() else {
            return;
        };

        let has_window = {
            let windows = conn.windows.borrow();
            windows
                .values()
                .any(|window| !window.borrow().is_dedicated_hotkey())
        };
        if has_window {
            return;
        }

        conn.dispatch_app_event(ApplicationEvent::PerformKeyAssignment(
            KeyAssignment::SpawnWindow,
        ));
    })
    .detach();
}

#[derive(Default)]
struct GlobalHotKeyState {
    handler_installed: bool,
    hotkey_ref: Option<usize>,
}

fn translated_layout_text(vkey: u16, modifier_flags: NSEventModifierFlags) -> Option<String> {
    let input_source = unsafe { TISCopyCurrentKeyboardInputSource() };
    if input_source.is_null() {
        return None;
    }
    let input_source = unsafe { InputSource::wrap_under_create_rule(input_source) };

    let layout_data = unsafe {
        let data = TISGetInputSourceProperty(
            input_source.as_concrete_TypeRef(),
            kTISPropertyUnicodeKeyLayoutData,
        );
        if data.is_null() {
            return None;
        }
        CFData::wrap_under_get_rule(data)
    };

    let layout =
        unsafe { CFDataGetBytePtr(layout_data.as_concrete_TypeRef()) as *const UCKeyboardLayout };
    if layout.is_null() {
        return None;
    }

    let modifier_key_state = (modifier_flags.bits() >> 16) as u32 & 0xFF;
    #[allow(non_upper_case_globals)]
    const kUCKeyTranslateNoDeadKeysBit: u32 = 0;

    let mut unicode_buffer = [0u16; 8];
    let mut length = 0;
    let mut dead_key_state = 0;
    let status = unsafe {
        UCKeyTranslate(
            layout,
            vkey,
            kUCKeyActionDisplay,
            modifier_key_state,
            LMGetKbdType() as u32,
            1 << kUCKeyTranslateNoDeadKeysBit,
            &mut dead_key_state,
            unicode_buffer.len() as UniCharCount,
            &mut length,
            unicode_buffer.as_mut_ptr(),
        )
    };
    if status != 0 || length == 0 {
        return None;
    }

    String::from_utf16(unsafe {
        std::slice::from_raw_parts(unicode_buffer.as_ptr(), length as usize)
    })
    .ok()
}

fn layout_translation_modifier_flags(mods: Modifiers) -> NSEventModifierFlags {
    let mut flags = NSEventModifierFlags::empty();
    if mods.intersects(Modifiers::SHIFT | Modifiers::LEFT_SHIFT | Modifiers::RIGHT_SHIFT) {
        flags |= NSEventModifierFlags::NSShiftKeyMask;
    }
    if mods.intersects(Modifiers::ALT | Modifiers::LEFT_ALT | Modifiers::RIGHT_ALT) {
        flags |= NSEventModifierFlags::NSAlternateKeyMask;
    }
    flags
}

fn mapped_char_to_macos_vkey(target: char, mods: Modifiers) -> Option<u32> {
    let target = if target.is_ascii_alphabetic() {
        target.to_ascii_lowercase()
    } else {
        target
    };
    let modifier_flags = layout_translation_modifier_flags(mods);

    for &vkey in layout_printable_vkeys() {
        let Some(text) = translated_layout_text(vkey, modifier_flags) else {
            continue;
        };
        let mut chars = text.chars();
        let Some(candidate) = chars.next() else {
            continue;
        };
        if chars.next().is_some() {
            continue;
        }

        let matches = if target.is_ascii_alphabetic() {
            candidate.to_ascii_lowercase() == target
        } else {
            candidate == target
        };
        if matches {
            return Some(u32::from(vkey));
        }
    }

    None
}

fn keycode_to_macos_vkey(key: &KeyCode, mods: Modifiers) -> Option<u32> {
    match key {
        KeyCode::RawCode(raw) => u16::try_from(*raw).ok().map(u32::from),
        KeyCode::Char(c) => mapped_char_to_macos_vkey(*c, mods)
            .or_else(|| key.to_phys().and_then(phys_to_vkey).map(u32::from)),
        KeyCode::Composed(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => mapped_char_to_macos_vkey(c, mods)
                    .or_else(|| key.to_phys().and_then(phys_to_vkey).map(u32::from)),
                _ => key.to_phys().and_then(phys_to_vkey).map(u32::from),
            }
        }
        _ => key.to_phys().and_then(phys_to_vkey).map(u32::from),
    }
}

fn mods_to_carbon_flags(mods: Modifiers) -> u32 {
    let mods = mods.remove_positional_mods();
    let mut flags = 0;
    if mods.contains(Modifiers::SHIFT) {
        flags |= SHIFT_KEY;
    }
    if mods.contains(Modifiers::ALT) {
        flags |= OPTION_KEY;
    }
    if mods.contains(Modifiers::CTRL) {
        flags |= CONTROL_KEY;
    }
    if mods.contains(Modifiers::SUPER) {
        flags |= CMD_KEY;
    }
    flags
}

fn configured_global_hotkey() -> Option<(u32, u32)> {
    let config = config::configuration();
    let hotkey = config.macos_global_hotkey.clone()?;
    let key = hotkey.key.resolve(config.key_map_preference);
    let Some(vkey) = keycode_to_macos_vkey(&key, hotkey.mods) else {
        log::warn!("macos_global_hotkey key {key:?} cannot be mapped to a macOS virtual key");
        return None;
    };

    let supported = Modifiers::SHIFT | Modifiers::ALT | Modifiers::CTRL | Modifiers::SUPER;
    let cleaned_mods = hotkey.mods.remove_positional_mods();
    let unsupported = cleaned_mods & !supported;
    if unsupported != Modifiers::NONE {
        log::warn!("macos_global_hotkey has unsupported modifiers: {unsupported:?}");
    }

    let flags = mods_to_carbon_flags(cleaned_mods);
    if flags == 0 {
        log::warn!("macos_global_hotkey requires at least one modifier key");
        return None;
    }

    Some((vkey, flags))
}

fn uninstall_registered_hotkey(state: &mut GlobalHotKeyState) {
    if let Some(hotkey_ref) = state.hotkey_ref.take() {
        let status = unsafe { UnregisterEventHotKey(hotkey_ref as EventHotKeyRef) };
        if status != NO_ERR {
            log::warn!("UnregisterEventHotKey failed with status={status}");
        }
    }
}

fn ensure_hotkey_handler_installed(state: &mut GlobalHotKeyState) -> bool {
    if state.handler_installed {
        return true;
    }

    let target = unsafe { GetApplicationEventTarget() };
    if target.is_null() {
        log::warn!("GetApplicationEventTarget returned null");
        return false;
    }

    let spec = EventTypeSpec {
        event_class: K_EVENT_CLASS_KEYBOARD,
        event_kind: K_EVENT_HOT_KEY_PRESSED,
    };
    let mut handler_ref: *mut c_void = std::ptr::null_mut();
    let status = unsafe {
        InstallEventHandler(
            target,
            global_hotkey_event_handler,
            1,
            &spec,
            std::ptr::null_mut(),
            &mut handler_ref,
        )
    };
    if status != NO_ERR {
        log::warn!("InstallEventHandler failed with status={status}");
        return false;
    }
    state.handler_installed = true;
    true
}

fn hotkey_target_window_id(remembered: Option<usize>, windows: &[usize]) -> Option<usize> {
    remembered.filter(|id| windows.contains(id))
}

pub(crate) fn register_hotkey_window(id: usize) {
    HOTKEY_WINDOW_ID.with(|remembered| remembered.set(Some(id)));
}

pub(crate) fn take_hotkey_window() -> Option<usize> {
    HOTKEY_WINDOW_ID.with(|remembered| remembered.take())
}

fn ordinary_hotkey_target_window_id(windows: &[(usize, bool, bool, bool)]) -> Option<usize> {
    let ordinary: Vec<_> = windows
        .iter()
        .filter(|(_, dedicated, _, _)| !dedicated)
        .collect();
    ordinary
        .iter()
        .find(|(_, _, key, _)| *key)
        .or_else(|| ordinary.iter().find(|(_, _, _, main)| *main))
        .or_else(|| ordinary.first())
        .map(|(id, _, _, _)| *id)
}

fn toggle_ordinary_hotkey_window(conn: &Connection) {
    let windows: Vec<_> = conn
        .windows
        .borrow()
        .iter()
        .map(|(id, window)| {
            let window = window.borrow();
            (
                *id,
                window.is_dedicated_hotkey(),
                window.is_key_window(),
                window.is_main_window(),
            )
        })
        .collect();
    let target = ordinary_hotkey_target_window_id(&windows).and_then(|id| conn.window_by_id(id));
    if let Some(target) = target {
        let mut target = target.borrow_mut();
        if target.ordinary_global_hotkey_should_hide() {
            target.hide_ordinary_global_hotkey_window();
        } else {
            target.show_ordinary_global_hotkey_window();
        }
    } else {
        conn.dispatch_app_event(ApplicationEvent::PerformKeyAssignment(
            KeyAssignment::SpawnWindow,
        ));
    }
}

fn toggle_hotkey_window() {
    let Some(conn) = Connection::get() else {
        log::warn!("global hotkey pressed before GUI connection is ready");
        return;
    };

    if config::configuration().macos_global_hotkey_mode == MacosGlobalHotkeyMode::Ordinary {
        toggle_ordinary_hotkey_window(&conn);
        return;
    }

    let existing_windows: Vec<_> = {
        let windows = conn.windows.borrow();
        windows.keys().copied().collect()
    };

    let target_id = HOTKEY_WINDOW_ID.with(|remembered| {
        let id = hotkey_target_window_id(remembered.get(), &existing_windows);
        remembered.set(id);
        id
    });
    if let Some(target_window) = target_id.and_then(|id| conn.window_by_id(id)) {
        // Keep toggling the same terminal even after the panel loses key/main.
        // Store only its ID; closing it must not keep the terminal alive.
        let mut target_window = target_window.borrow_mut();
        // Nonactivating panels can be key while Kaku is not the active app.
        if target_window.global_hotkey_should_hide() {
            target_window.hide_global_hotkey_window();
        } else {
            target_window.show_global_hotkey_window();
        }
    } else {
        conn.dispatch_app_event(ApplicationEvent::SpawnHotkeyWindow);
    }
}

extern "C" fn global_hotkey_event_handler(
    _next_handler: EventHandlerCallRef,
    _event: EventRef,
    _user_data: *mut c_void,
) -> OSStatus {
    toggle_hotkey_window();
    NO_ERR
}

pub(crate) fn sync_global_hotkey_registration() {
    let mut state = GLOBAL_HOTKEY_STATE.lock().unwrap();

    uninstall_registered_hotkey(&mut state);

    let Some((vkey, flags)) = configured_global_hotkey() else {
        return;
    };

    if !ensure_hotkey_handler_installed(&mut state) {
        return;
    }

    let mut hotkey_ref: EventHotKeyRef = std::ptr::null_mut();
    let hotkey_id = EventHotKeyID {
        signature: HOTKEY_SIGNATURE,
        id: HOTKEY_ID,
    };
    let target = unsafe { GetApplicationEventTarget() };
    let status = unsafe { RegisterEventHotKey(vkey, flags, hotkey_id, target, 0, &mut hotkey_ref) };
    if status != NO_ERR || hotkey_ref.is_null() {
        log::warn!("RegisterEventHotKey failed with status={status}");
        return;
    }

    state.hotkey_ref = Some(hotkey_ref as usize);
}

fn reap_kaku_autofill_helpers() {
    // Best-effort cleanup for macOS helper leaks where AutoFill (Kaku)
    // can accumulate across app restarts.
    const SCRIPT: &str = r#"
for pid in $(pgrep -f 'SafariPlatformSupport.Helper|CredentialProviderExtensionHelper' 2>/dev/null); do
  name=$(lsappinfo info -only name -pid "$pid" 2>/dev/null | sed -n 's/.*"LSDisplayName"="\([^"]*\)".*/\1/p')
  case "$name" in
    *"(Kaku)"*) kill "$pid" 2>/dev/null || true ;;
  esac
done
"#;

    match Command::new("/bin/sh").arg("-c").arg(SCRIPT).status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            log::debug!("reap_kaku_autofill_helpers exited with status {}", status);
        }
        Err(err) => {
            log::warn!("reap_kaku_autofill_helpers failed: {err:#}");
        }
    }
}

extern "C" fn application_should_terminate(
    _self: &mut Object,
    _sel: Sel,
    _app: *mut Object,
) -> u64 {
    unsafe {
        match config::configuration().window_close_confirmation {
            WindowCloseConfirmation::NeverPrompt => terminate_now(
                QuitOrigin::AppKitShouldTerminate,
                Some("applicationShouldTerminate"),
            ),
            // SmartPrompt's process-aware decision lives in the GUI layer
            // (it needs Mux, unreachable from here). Route the quit through
            // the frontmost window so it can run the same SmartPrompt logic
            // used by the keyboard Cmd+Q path; cancel the AppKit terminate
            // and let the GUI call request_terminate when it's ready.
            WindowCloseConfirmation::SmartPrompt => {
                if route_quit_to_frontmost_window() {
                    NSApplicationTerminateReply::NSTerminateCancel as u64
                } else {
                    terminate_now(
                        QuitOrigin::AppKitShouldTerminate,
                        Some("smart-prompt-no-window"),
                    )
                }
            }
            WindowCloseConfirmation::AlwaysPrompt => {
                let alert: id = msg_send![class!(NSAlert), alloc];
                let alert: id = msg_send![alert, init];
                let message_text = nsstring("Terminate Kaku?");
                let info_text = nsstring("Detach and close all panes and terminate Kaku?");
                let cancel = nsstring("Cancel");
                let ok = nsstring("Ok");

                let () = msg_send![alert, setMessageText: message_text];
                let () = msg_send![alert, setInformativeText: info_text];
                let () = msg_send![alert, addButtonWithTitle: cancel];
                let () = msg_send![alert, addButtonWithTitle: ok];
                #[allow(non_upper_case_globals)]
                const NSModalResponseCancel: NSInteger = 1000;
                #[allow(non_upper_case_globals, dead_code)]
                const NSModalResponseOK: NSInteger = 1001;
                let result: NSInteger = msg_send![alert, runModal];

                if result == NSModalResponseCancel {
                    NSApplicationTerminateReply::NSTerminateCancel as u64
                } else {
                    terminate_now(
                        QuitOrigin::AppKitShouldTerminate,
                        Some("applicationShouldTerminate"),
                    )
                }
            }
        }
    }
}

extern "C" fn application_should_terminate_after_last_window_closed(
    _self: &mut Object,
    _sel: Sel,
    _app: *mut Object,
) -> BOOL {
    // Keep app process alive on macOS after the last window closes,
    // so Dock reopen can create a new window without cold-start.
    NO
}

/// Find the frontmost Kaku window and dispatch QuitApplication to it so the
/// GUI layer can run its mux-aware SmartPrompt logic (and show the confirm
/// overlay if a stateful pane is still running). Returns `true` when a
/// window accepted the dispatch; `false` when there is no window to route
/// to and the caller should terminate immediately.
fn route_quit_to_frontmost_window() -> bool {
    let Some(conn) = Connection::get() else {
        return false;
    };
    let existing: Vec<_> = {
        let windows = conn.windows.borrow();
        windows.values().cloned().collect()
    };
    if existing.is_empty() {
        return false;
    }
    let target = existing
        .iter()
        .find(|w| w.borrow().is_key_window())
        .cloned()
        .or_else(|| {
            existing
                .iter()
                .find(|w| w.borrow().is_main_window())
                .cloned()
        })
        .or_else(|| existing.first().cloned());
    let Some(target) = target else {
        return false;
    };
    let dispatched = target
        .borrow()
        .dispatch_key_assignment(config::keyassignment::KeyAssignment::QuitApplication);
    if !dispatched {
        log::warn!("route_quit_to_frontmost_window: dispatch failed; falling back to terminate");
    }
    dispatched
}

fn terminate_now(origin: QuitOrigin, detail: Option<&str>) -> u64 {
    // Persist the key (frontmost) window's geometry before the event loop
    // stops. This is the reliable save path for Cmd+Q; window_will_close
    // may not fire for every window before the process exits.
    super::window::on_app_terminating();
    reap_kaku_autofill_helpers();
    uninstall_registered_hotkey(&mut GLOBAL_HOTKEY_STATE.lock().unwrap());
    request_app_termination(origin, detail);
    NSApplicationTerminateReply::NSTerminateNow as u64
}

extern "C" fn application_will_finish_launching(
    _self: &mut Object,
    _sel: Sel,
    _notif: *mut Object,
) {
    log::debug!("application_will_finish_launching");
    std::thread::spawn(reap_kaku_autofill_helpers);
}

extern "C" fn application_did_finish_launching(this: &mut Object, _sel: Sel, _notif: *mut Object) {
    log::debug!("application_did_finish_launching");
    unsafe {
        let () = msg_send![NSApp(), setServicesProvider: this as *mut Object];
        (*this).set_ivar("launched", YES);

        // Register for screen wake notifications to update OpenGL contexts.
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let notification_center: id = msg_send![workspace, notificationCenter];
        let notification_name = nsstring("NSWorkspaceScreensDidWakeNotification");
        let () = msg_send![notification_center,
            addObserver: this as *mut Object
            selector: sel!(screensDidWake:)
            name: *notification_name
            object: nil
        ];
        log::debug!("registered for NSWorkspaceScreensDidWakeNotification");

        // Register for will-sleep so we can block flushBuffer before the
        // surface is invalidated. The low-level SkyLight displayStatus callback
        // can trigger a paint between sleep and the ScreensDidWake notification.
        let will_sleep_name = nsstring("NSWorkspaceWillSleepNotification");
        let () = msg_send![notification_center,
            addObserver: this as *mut Object
            selector: sel!(workspaceWillSleep:)
            name: *will_sleep_name
            object: nil
        ];
        log::debug!("registered for NSWorkspaceWillSleepNotification");

        // Register for display topology changes (monitor connect/disconnect,
        // resolution updates) and refresh all window backends the same way.
        let app_notification_center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
        let app_notification_name = nsstring("NSApplicationDidChangeScreenParametersNotification");
        let () = msg_send![app_notification_center,
            addObserver: this as *mut Object
            selector: sel!(screenParametersDidChange:)
            name: *app_notification_name
            object: nil
        ];
        log::debug!("registered for NSApplicationDidChangeScreenParametersNotification");

        let keyboard_notification_name =
            nsstring("NSTextInputContextKeyboardSelectionDidChangeNotification");
        let () = msg_send![app_notification_center,
            addObserver: this as *mut Object
            selector: sel!(keyboardSelectionDidChange:)
            name: *keyboard_notification_name
            object: nil
        ];
        log::debug!("registered for NSTextInputContextKeyboardSelectionDidChangeNotification");

        // Register for system Light/Dark appearance changes. Windows pin their
        // NSWindow appearance, which suppresses viewDidChangeEffectiveAppearance,
        // so already-open windows must be told explicitly to re-resolve theme.
        let distributed_center: id =
            msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
        let appearance_notification_name = nsstring("AppleInterfaceThemeChangedNotification");
        let () = msg_send![distributed_center,
            addObserver: this as *mut Object
            selector: sel!(systemAppearanceDidChange:)
            name: *appearance_notification_name
            object: nil
        ];
        log::debug!("registered for AppleInterfaceThemeChangedNotification");
    }
    sync_global_hotkey_registration();
}

fn refresh_all_window_contexts_after_display_change(
    reason: &'static str,
    remaining_retries: usize,
) {
    let Some(conn) = Connection::get() else {
        return;
    };

    let windows: Vec<_> = conn.windows.borrow().values().cloned().collect();
    let mut busy_windows = false;
    for window in windows {
        if let Ok(mut inner) = window.try_borrow_mut() {
            if !inner.refresh_after_display_change() {
                busy_windows = true;
            }
        } else {
            busy_windows = true;
        }
    }

    if busy_windows && remaining_retries > 0 {
        log::debug!(
            "retrying display refresh after {} because some windows were busy (remaining_retries={})",
            reason,
            remaining_retries
        );
        promise::spawn::spawn_into_main_thread(async move {
            async_io::Timer::after(DISPLAY_CHANGE_RETRY_DELAY).await;
            refresh_all_window_contexts_after_display_change(reason, remaining_retries - 1);
        })
        .detach();
    }
}

/// Called when the system is about to sleep. Sets the SYSTEM_SLEEPING flag so
/// that `should_defer_flush_buffer` blocks flushBuffer before the GPU surface
/// is torn down.
extern "C" fn workspace_will_sleep(_self: &mut Object, _sel: Sel, _notification: *mut Object) {
    log::debug!("NSWorkspaceWillSleepNotification received, blocking OpenGL flushBuffer");
    SYSTEM_SLEEPING.store(true, Ordering::Release);
}

/// Called when the system wakes from sleep. Updates all OpenGL contexts to
/// prevent crashes from stale surfaces when AppKit tries to flush the backing layer.
extern "C" fn screens_did_wake(_self: &mut Object, _sel: Sel, _notification: *mut Object) {
    log::debug!("NSWorkspaceScreensDidWakeNotification received, updating OpenGL contexts");
    // Clear the sleep flag. The display-change defer armed by
    // refresh_all_window_contexts_after_display_change will keep flushBuffer
    // blocked for an additional period while the surface rebuilds.
    SYSTEM_SLEEPING.store(false, Ordering::Release);
    refresh_all_window_contexts_after_display_change("system wake", DISPLAY_CHANGE_MAX_RETRIES);
}

/// One entry per NSScreen. Used to detect whether a display-parameter
/// notification reflects an actual topology change (monitor connect/disconnect,
/// resolution/arrangement change) versus a spurious one fired by menubar tools
/// (sketchybar, iStat Menus) or Dock position shifts. The latter arrive at
/// 10+ Hz in some setups and each triggers a ~150ms paint defer + full
/// per-window GL refresh, which is the primary cause of perceived lag when
/// cross-screen moves or window hotkeys fire nearby.
#[derive(Clone, PartialEq)]
struct ScreenSig {
    name: String,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    scale_x100: u32,
}

static LAST_SCREEN_TOPOLOGY: Mutex<Option<Vec<ScreenSig>>> = Mutex::new(None);

fn current_screen_topology() -> Vec<ScreenSig> {
    unsafe {
        let screens: id = msg_send![class!(NSScreen), screens];
        if screens.is_null() {
            return vec![];
        }
        let count: NSUInteger = msg_send![screens, count];
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let scr: id = msg_send![screens, objectAtIndex: i];
            if scr.is_null() {
                continue;
            }
            let frame: NSRect = msg_send![scr, frame];
            let scale: CGFloat = msg_send![scr, backingScaleFactor];
            let has_name: BOOL = msg_send![scr, respondsToSelector: sel!(localizedName)];
            let name = if has_name == YES {
                let nsname: id = msg_send![scr, localizedName];
                if nsname.is_null() {
                    String::new()
                } else {
                    nsstring_to_str(nsname).to_string()
                }
            } else {
                String::new()
            };
            out.push(ScreenSig {
                name,
                x: frame.origin.x as i64,
                y: frame.origin.y as i64,
                w: frame.size.width as i64,
                h: frame.size.height as i64,
                scale_x100: (scale * 100.0) as u32,
            });
        }
        out
    }
}

/// Called when macOS reports a display topology change (screen attach/detach,
/// resolution or arrangement changes). Refreshes all window backends so stale
/// OpenGL drawables and cached screen-dependent values are rebuilt promptly.
///
/// Compares the current NSScreen topology against the last-known snapshot and
/// skips the refresh entirely when nothing changed. Menubar customizers and
/// window managers fire this notification frequently without a real topology
/// change; each unnecessary refresh blocks paint for 150-300ms per window.
extern "C" fn screen_parameters_did_change(
    _self: &mut Object,
    _sel: Sel,
    _notification: *mut Object,
) {
    let current = current_screen_topology();
    {
        let mut cached = LAST_SCREEN_TOPOLOGY.lock().unwrap();
        if cached.as_ref() == Some(&current) {
            log::trace!(
                "NSApplicationDidChangeScreenParametersNotification ignored; topology unchanged"
            );
            return;
        }
        *cached = Some(current);
    }
    log::debug!("NSApplicationDidChangeScreenParametersNotification received, refreshing displays");
    refresh_all_window_contexts_after_display_change(
        "screen parameter change",
        DISPLAY_CHANGE_MAX_RETRIES,
    );
}

extern "C" fn keyboard_selection_did_change(
    _self: &mut Object,
    _sel: Sel,
    _notification: *mut Object,
) {
    log::debug!(
        "NSTextInputContextKeyboardSelectionDidChangeNotification received, syncing global hotkey"
    );
    sync_global_hotkey_registration();
}

/// Called when the system Light/Dark appearance changes. The broadcast is
/// deferred onto the next main-thread pass so `NSApp.effectiveAppearance` is
/// settled before each window re-resolves its color scheme.
extern "C" fn system_appearance_did_change(
    _self: &mut Object,
    _sel: Sel,
    _notification: *mut Object,
) {
    log::debug!("AppleInterfaceThemeChangedNotification received, refreshing open windows");
    promise::spawn::spawn_into_main_thread(async move {
        super::window::broadcast_system_appearance_change();
    })
    .detach();
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn hotkey_options_ordinary_mode_has_a_separate_route() {
        let source = include_str!("app.rs");
        let toggle = source
            .split("fn toggle_hotkey_window()")
            .nth(1)
            .unwrap()
            .split("extern \"C\" fn global_hotkey_event_handler")
            .next()
            .unwrap();
        assert!(toggle.contains("macos_global_hotkey_mode"));
        assert!(toggle.contains("MacosGlobalHotkeyMode::Ordinary"));
        assert!(toggle.contains("toggle_ordinary_hotkey_window"));
        let ordinary = source
            .split("fn toggle_ordinary_hotkey_window(")
            .nth(1)
            .unwrap()
            .split("fn toggle_hotkey_window()")
            .next()
            .unwrap();
        assert!(ordinary.contains("is_dedicated_hotkey()"));
        assert!(ordinary.contains("ordinary_hotkey_target_window_id(&windows)"));
        assert!(ordinary.contains("show_ordinary_global_hotkey_window()"));
        assert!(ordinary.contains("hide_ordinary_global_hotkey_window()"));
        assert!(ordinary.contains("KeyAssignment::SpawnWindow"));
        assert!(!ordinary.contains("SpawnHotkeyWindow"));
        assert!(!ordinary.contains("hide_application()"));
        let native = include_str!("window.rs");
        let hide = native
            .split("pub(crate) fn hide_ordinary_global_hotkey_window(")
            .nth(1)
            .unwrap()
            .split("pub(crate) fn is_key_window(")
            .next()
            .unwrap();
        assert!(hide.contains("if !self.dedicated_hotkey"));
        assert!(hide.contains("order_out_after_fullscreen_exit()"));
        assert!(hide.contains("!self.defer_hotkey_visibility(false)"));
        assert!(!hide.contains("orderOut:"));
        let show = native
            .split("pub(crate) fn show_ordinary_global_hotkey_window(")
            .nth(1)
            .unwrap()
            .split("pub(crate) fn hide_ordinary_global_hotkey_window(")
            .next()
            .unwrap();
        assert!(show.contains("if self.dedicated_hotkey"));
        assert!(show.contains("order_out_on_fullscreen_exit.set(false)"));
        assert!(show.contains("self.focus()"));
        assert!(show.contains("self.defer_hotkey_visibility(true)"));
        assert!(!show.contains("prepare_global_hotkey_panel"));
        assert!(!show.contains("setFrame"));
        assert!(!show.contains("setStyleMask"));
        let replay = native
            .rsplit("fn dispatch_pending_hotkey_visibility(")
            .next()
            .unwrap()
            .split("fn cancel_pending_perform_requests(")
            .next()
            .unwrap();
        assert!(replay.contains("if !inner.dedicated_hotkey"));
        assert!(replay.contains("inner.show_ordinary_global_hotkey_window()"));
        assert!(replay.contains("inner.hide_ordinary_global_hotkey_window()"));
    }

    #[test]
    fn hotkey_options_ordinary_selection_excludes_drawer_and_keeps_key_main_priority() {
        assert_eq!(
            ordinary_hotkey_target_window_id(&[(42, true, true, true)]),
            None
        );
        assert_eq!(
            ordinary_hotkey_target_window_id(&[
                (42, true, true, true),
                (7, false, false, true),
                (3, false, true, false),
            ]),
            Some(3)
        );
        assert_eq!(
            ordinary_hotkey_target_window_id(&[
                (42, true, true, true),
                (7, false, false, true),
                (3, false, false, false),
            ]),
            Some(7)
        );
        assert_eq!(
            ordinary_hotkey_target_window_id(&[
                (42, true, true, true),
                (3, false, false, false),
                (7, false, false, false),
            ]),
            Some(3)
        );
        assert_eq!(ordinary_hotkey_target_window_id(&[]), None);
    }

    #[test]
    fn hotkey_lifecycle_target_survives_hide_and_other_window_focus() {
        let windows = [1, 2];
        assert_eq!(hotkey_target_window_id(Some(1), &windows), Some(1));
        assert_eq!(hotkey_target_window_id(Some(3), &windows), None);
        assert_eq!(hotkey_target_window_id(None, &windows), None);
        assert_eq!(hotkey_target_window_id(Some(1), &[]), None);
    }

    #[test]
    fn dock_reopen_never_focuses_or_reuses_the_dedicated_hotkey_window() {
        let source = include_str!("app.rs");
        let reopen = source
            .split("extern \"C\" fn application_open_untitled_file(")
            .nth(1)
            .unwrap()
            .split("extern \"C\" fn kaku_perform_key_assignment")
            .next()
            .unwrap();
        assert!(reopen.contains("!window.borrow().is_dedicated_hotkey()"));
        let schedule = source
            .split("fn schedule_open_untitled_spawn()")
            .nth(1)
            .unwrap()
            .split("#[derive(Default)]")
            .next()
            .unwrap();
        assert!(schedule.contains("!window.borrow().is_dedicated_hotkey()"));
        let toggle = source
            .split("fn toggle_hotkey_window()")
            .nth(1)
            .unwrap()
            .split("extern \"C\" fn global_hotkey_event_handler")
            .next()
            .unwrap();
        assert!(toggle.contains("ApplicationEvent::SpawnHotkeyWindow"));
        assert!(!toggle.contains("KeyAssignment::SpawnWindow"));
    }

    fn unique_test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("kaku-app-tests-{}-{nanos}", name))
    }

    #[test]
    fn layout_translation_modifier_flags_include_shift_and_option() {
        let flags = layout_translation_modifier_flags(Modifiers::SHIFT | Modifiers::ALT);

        assert!(flags.contains(NSEventModifierFlags::NSShiftKeyMask));
        assert!(flags.contains(NSEventModifierFlags::NSAlternateKeyMask));
    }

    #[test]
    fn layout_translation_modifier_flags_accept_positional_option_bits() {
        let flags = layout_translation_modifier_flags(Modifiers::LEFT_ALT | Modifiers::RIGHT_SHIFT);

        assert!(flags.contains(NSEventModifierFlags::NSAlternateKeyMask));
        assert!(flags.contains(NSEventModifierFlags::NSShiftKeyMask));
    }

    #[test]
    fn layout_translation_modifier_flags_ignore_non_text_modifiers() {
        let flags = layout_translation_modifier_flags(Modifiers::CTRL | Modifiers::SUPER);

        assert_eq!(flags, NSEventModifierFlags::empty());
    }

    #[test]
    fn normalize_finder_service_path_uses_parent_for_files() {
        let dir = unique_test_path("file-parent");
        fs::create_dir_all(&dir).expect("create temp dir");
        let file = dir.join("demo.txt");
        fs::write(&file, "demo").expect("create temp file");

        let normalized = normalize_finder_service_path(file.to_string_lossy().into_owned());

        assert_eq!(normalized, dir.to_string_lossy().into_owned());

        fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn normalize_finder_service_path_keeps_directories() {
        let dir = unique_test_path("dir-stays-dir");
        fs::create_dir_all(&dir).expect("create temp dir");

        let normalized = normalize_finder_service_path(dir.to_string_lossy().into_owned());

        assert_eq!(normalized, dir.to_string_lossy().into_owned());

        fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn normalize_finder_service_path_keeps_unknown_paths() {
        let path = unique_test_path("missing-path");
        let path = path.to_string_lossy().into_owned();

        let normalized = normalize_finder_service_path(path.clone());

        assert_eq!(normalized, path);
    }

    #[test]
    fn parse_url_action_supports_open_tab_by_tty() {
        let url = Url::parse("kaku://open-tab?tty=/dev/ttys012").expect("valid url");
        assert_eq!(
            parse_url_action(&url),
            Some(UrlAction::OpenTabByTty("/dev/ttys012".to_string()))
        );
    }

    #[test]
    fn parse_url_action_supports_activate_pane_path_form() {
        let url = Url::parse("kaku://activate-pane/42").expect("valid url");
        assert_eq!(
            parse_url_action(&url),
            Some(UrlAction::ActivatePaneById(42))
        );
    }

    #[test]
    fn parse_url_action_supports_activate_tab_query_form() {
        let url = Url::parse("kaku://activate-tab?tab_id=7").expect("valid url");
        assert_eq!(parse_url_action(&url), Some(UrlAction::ActivateTabById(7)));
    }

    #[test]
    fn parse_url_action_rejects_missing_or_invalid_ids() {
        let missing = Url::parse("kaku://activate-pane").expect("valid url");
        let invalid = Url::parse("kaku://activate-tab/not-a-number").expect("valid url");
        assert_eq!(parse_url_action(&missing), None);
        assert_eq!(parse_url_action(&invalid), None);
    }
}

extern "C" fn application_open_untitled_file(
    this: &mut Object,
    _sel: Sel,
    _app: *mut Object,
) -> BOOL {
    let launched: BOOL = unsafe { *this.get_ivar("launched") };
    if let Some(conn) = Connection::get() {
        if launched == YES {
            let existing_windows: Vec<_> = {
                let windows = conn.windows.borrow();
                windows
                    .values()
                    .filter(|window| !window.borrow().is_dedicated_hotkey())
                    .cloned()
                    .collect()
            };
            if !existing_windows.is_empty() {
                LAST_OPEN_UNTITLED_SPAWN.with(|last| {
                    last.borrow_mut().take();
                });
                for window in existing_windows {
                    window.borrow_mut().focus();
                }
            } else {
                if should_suppress_open_untitled_spawn() {
                    log::debug!(
                        "Skipping applicationOpenUntitledFile because a service open request \
                         was received recently"
                    );
                    return YES;
                }

                let should_spawn = LAST_OPEN_UNTITLED_SPAWN.with(|last| {
                    let now = Instant::now();
                    let mut last = last.borrow_mut();
                    let elapsed = last.map(|prior| now.duration_since(prior));
                    let too_soon = elapsed
                        .map(|duration| duration < OPEN_UNTITLED_SPAWN_DEBOUNCE)
                        .unwrap_or(false);
                    if too_soon {
                        false
                    } else {
                        *last = Some(now);
                        true
                    }
                });

                if should_spawn {
                    if crate::connection::startup_pending_first_window() {
                        // During cold startup the normal startup pipeline owns the first
                        // window. This flag is set synchronously before any AppKit event
                        // can fire and only cleared once that window registers, so we
                        // never race the pipeline into a second empty window. This avoids
                        // a fixed per-launch delay while still letting service-open
                        // requests take over.
                        log::debug!(
                            "Ignoring applicationOpenUntitledFile while startup pipeline owns \
                             the first window"
                        );
                    } else {
                        schedule_open_untitled_spawn();
                    }
                }
            }
        }
        return YES;
    }
    NO
}

extern "C" fn kaku_perform_key_assignment(_self: &mut Object, _sel: Sel, menu_item: *mut Object) {
    let menu_item = crate::os::macos::menu::MenuItem::with_menu_item(menu_item);
    // Safe because kakuPerformKeyAssignment: is only used with KeyAssignment
    let action = menu_item.get_represented_item();
    log::debug!("kaku_perform_key_assignment {action:?}",);
    match action {
        Some(RepresentedItem::KeyAssignment(action)) => {
            if let Some(conn) = Connection::get() {
                conn.dispatch_app_event(ApplicationEvent::PerformKeyAssignment(action));
            }
        }
        Some(RepresentedItem::KeyAssignmentForPane { .. }) | None => {}
    }
}

extern "C" fn show_settings_window(_self: &mut Object, _sel: Sel, _sender: *mut Object) {
    if let Some(conn) = Connection::get() {
        conn.dispatch_app_event(ApplicationEvent::PerformKeyAssignment(
            KeyAssignment::EmitEvent("open-kaku-config".to_string()),
        ));
    }
}

extern "C" fn application_open_file(
    _this: &mut Object,
    _sel: Sel,
    _app: *mut Object,
    file_name: *mut Object,
) -> BOOL {
    let file_name = unsafe { nsstring_to_str(file_name) }.to_string();
    if file_name.is_empty() {
        return NO;
    }

    log::debug!("application_open_file {file_name}");
    dispatch_or_queue_service_open(file_name, true);
    YES
}

extern "C" fn application_open_files(
    _this: &mut Object,
    _sel: Sel,
    app: *mut Object,
    file_names: *mut Object,
) {
    #[allow(non_upper_case_globals)]
    const NSApplicationDelegateReplySuccess: NSInteger = 0;
    #[allow(non_upper_case_globals)]
    const NSApplicationDelegateReplyFailure: NSInteger = 2;

    let mut reply = NSApplicationDelegateReplyFailure;
    let mut dispatched = false;
    unsafe {
        let count: NSInteger = msg_send![file_names, count];
        for i in 0..count {
            let file_name: *mut Object = msg_send![file_names, objectAtIndex: i];
            let file_str = nsstring_to_str(file_name).to_string();
            if file_str.is_empty() {
                continue;
            }
            log::debug!("application_open_files {file_str}");
            dispatch_or_queue_service_open(file_str, true);
            dispatched = true;
        }
    }
    if dispatched {
        reply = NSApplicationDelegateReplySuccess;
    }

    unsafe {
        let target = if app.is_null() { NSApp() } else { app };
        let _: () = msg_send![target, replyToOpenOrPrint: reply];
    }
}

fn first_service_path(pasteboard: *mut Object) -> Option<String> {
    if pasteboard.is_null() {
        return None;
    }

    unsafe {
        let files: id = msg_send![pasteboard, propertyListForType: NSFilenamesPboardType];
        if !files.is_null() {
            let count: NSInteger = msg_send![files, count];
            if count > 0 {
                let file_name: *mut Object = msg_send![files, objectAtIndex: 0];
                if !file_name.is_null() {
                    return Some(nsstring_to_str(file_name).to_string());
                }
            }
        }

        let text: *mut Object = msg_send![pasteboard, stringForType: NSStringPboardType];
        if text.is_null() {
            return None;
        }

        let raw = nsstring_to_str(text).trim().to_string();
        if raw.is_empty() {
            return None;
        }

        if let Ok(url) = Url::parse(&raw) {
            if url.scheme() == "file" {
                if let Ok(path) = url.to_file_path() {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }

        Some(raw)
    }
}

fn normalize_finder_service_path(path: String) -> String {
    let path_ref = Path::new(&path);

    if path_ref.is_file() {
        if let Some(parent) = path_ref.parent() {
            return parent.to_string_lossy().into_owned();
        }
    }

    path
}

fn dispatch_or_queue_service_open(path: String, prefer_existing_window: bool) {
    note_service_open_request();

    if let Some(conn) = Connection::get() {
        if crate::connection::app_event_handler_ready() {
            let event = if prefer_existing_window {
                ApplicationEvent::OpenCommandScriptInTab(path)
            } else {
                ApplicationEvent::OpenCommandScript(path)
            };
            conn.dispatch_app_event(event);
            return;
        }
    }

    if Connection::get().is_some() {
        log::debug!("service request queued until app event handler is ready");
    } else {
        log::debug!("service request queued until GUI connection is ready");
    }
    PENDING_SERVICE_OPENS
        .lock()
        .unwrap()
        .push((path, prefer_existing_window));
}

fn dispatch_or_queue_tty_activation(tty: String) {
    if let Some(conn) = Connection::get() {
        if crate::connection::app_event_handler_ready() {
            conn.dispatch_app_event(ApplicationEvent::ActivatePaneForTty(tty));
            return;
        }
    }

    if Connection::get().is_some() {
        log::debug!("tty activation queued until app event handler is ready");
    } else {
        log::debug!("tty activation queued until GUI connection is ready");
    }
    PENDING_TTY_ACTIVATIONS.lock().unwrap().push(tty);
}

fn dispatch_or_queue_pane_id_activation(pane_id: usize) {
    if let Some(conn) = Connection::get() {
        if crate::connection::app_event_handler_ready() {
            conn.dispatch_app_event(ApplicationEvent::ActivatePaneById(pane_id));
            return;
        }
    }

    if Connection::get().is_some() {
        log::debug!("pane-id activation queued until app event handler is ready");
    } else {
        log::debug!("pane-id activation queued until GUI connection is ready");
    }
    PENDING_PANE_ID_ACTIVATIONS.lock().unwrap().push(pane_id);
}

fn dispatch_or_queue_tab_id_activation(tab_id: usize) {
    if let Some(conn) = Connection::get() {
        if crate::connection::app_event_handler_ready() {
            conn.dispatch_app_event(ApplicationEvent::ActivateTabById(tab_id));
            return;
        }
    }

    if Connection::get().is_some() {
        log::debug!("tab-id activation queued until app event handler is ready");
    } else {
        log::debug!("tab-id activation queued until GUI connection is ready");
    }
    PENDING_TAB_ID_ACTIVATIONS.lock().unwrap().push(tab_id);
}

pub(crate) fn flush_pending_service_opens() {
    let pending = {
        let mut queued = PENDING_SERVICE_OPENS.lock().unwrap();
        std::mem::take(&mut *queued)
    };
    let pending_ttys = {
        let mut queued = PENDING_TTY_ACTIVATIONS.lock().unwrap();
        std::mem::take(&mut *queued)
    };
    let pending_pane_ids = {
        let mut queued = PENDING_PANE_ID_ACTIVATIONS.lock().unwrap();
        std::mem::take(&mut *queued)
    };
    let pending_tab_ids = {
        let mut queued = PENDING_TAB_ID_ACTIVATIONS.lock().unwrap();
        std::mem::take(&mut *queued)
    };

    if pending.is_empty()
        && pending_ttys.is_empty()
        && pending_pane_ids.is_empty()
        && pending_tab_ids.is_empty()
    {
        return;
    }

    if let Some(conn) = Connection::get() {
        for (path, prefer_existing_window) in pending {
            let event = if prefer_existing_window {
                ApplicationEvent::OpenCommandScriptInTab(path)
            } else {
                ApplicationEvent::OpenCommandScript(path)
            };
            conn.dispatch_app_event(event);
        }
        for tty in pending_ttys {
            conn.dispatch_app_event(ApplicationEvent::ActivatePaneForTty(tty));
        }
        for pane_id in pending_pane_ids {
            conn.dispatch_app_event(ApplicationEvent::ActivatePaneById(pane_id));
        }
        for tab_id in pending_tab_ids {
            conn.dispatch_app_event(ApplicationEvent::ActivateTabById(tab_id));
        }
    } else {
        let mut queued = PENDING_SERVICE_OPENS.lock().unwrap();
        queued.extend(pending);
        let mut queued_ttys = PENDING_TTY_ACTIVATIONS.lock().unwrap();
        queued_ttys.extend(pending_ttys);
        let mut queued_panes = PENDING_PANE_ID_ACTIVATIONS.lock().unwrap();
        queued_panes.extend(pending_pane_ids);
        let mut queued_tabs = PENDING_TAB_ID_ACTIVATIONS.lock().unwrap();
        queued_tabs.extend(pending_tab_ids);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum UrlAction {
    OpenTabByTty(String),
    ActivatePaneById(usize),
    ActivateTabById(usize),
}

fn parse_id_from_url(url: &Url, path_segment: Option<&str>, query_key: &str) -> Option<usize> {
    let raw = path_segment
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            url.query_pairs()
                .find_map(|(key, value)| (key == query_key).then(|| value.into_owned()))
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })?;
    raw.parse::<usize>().ok()
}

fn parse_url_action(url: &Url) -> Option<UrlAction> {
    if url.scheme() != "kaku" {
        return None;
    }

    let mut segments = url.path_segments().into_iter().flatten();
    let action = url
        .host_str()
        .filter(|host| !host.is_empty())
        .or_else(|| segments.find(|segment| !segment.is_empty()))?;
    let path_arg = segments.find(|segment| !segment.is_empty());

    match action {
        "open-tab" => {
            let tty = url
                .query_pairs()
                .find_map(|(key, value)| (key == "tty").then(|| value.into_owned()))?
                .trim()
                .to_string();
            if tty.is_empty() {
                return None;
            }
            Some(UrlAction::OpenTabByTty(tty))
        }
        "activate-pane" => {
            parse_id_from_url(url, path_arg, "pane_id").map(UrlAction::ActivatePaneById)
        }
        "activate-tab" => {
            parse_id_from_url(url, path_arg, "tab_id").map(UrlAction::ActivateTabById)
        }
        _ => None,
    }
}

extern "C" fn application_open_urls(
    _this: &mut Object,
    _sel: Sel,
    _app: *mut Object,
    urls: *mut Object,
) {
    unsafe {
        let count: NSInteger = msg_send![urls, count];
        for i in 0..count {
            let ns_url: *mut Object = msg_send![urls, objectAtIndex: i];
            let abs_string: *mut Object = msg_send![ns_url, absoluteString];
            if abs_string.is_null() {
                continue;
            }

            let raw_url = nsstring_to_str(abs_string).to_string();
            match Url::parse(&raw_url) {
                Ok(parsed) => {
                    if parsed.scheme() == "file" {
                        match parsed.to_file_path() {
                            Ok(path) => {
                                let path = path.to_string_lossy().into_owned();
                                log::debug!("application_open_urls file {path}");
                                dispatch_or_queue_service_open(path, true);
                            }
                            Err(_) => {
                                log::warn!("application_open_urls invalid file url: {raw_url}");
                            }
                        }
                        continue;
                    }

                    match parse_url_action(&parsed) {
                        Some(UrlAction::OpenTabByTty(tty)) => {
                            log::debug!("application_open_urls open-tab tty={tty}");
                            dispatch_or_queue_tty_activation(tty);
                        }
                        Some(UrlAction::ActivatePaneById(pane_id)) => {
                            log::debug!("application_open_urls activate-pane pane_id={pane_id}");
                            dispatch_or_queue_pane_id_activation(pane_id);
                        }
                        Some(UrlAction::ActivateTabById(tab_id)) => {
                            log::debug!("application_open_urls activate-tab tab_id={tab_id}");
                            dispatch_or_queue_tab_id_activation(tab_id);
                        }
                        None => {
                            log::warn!("application_open_urls unsupported url: {raw_url}");
                        }
                    }
                }
                Err(err) => {
                    log::warn!("application_open_urls invalid url {raw_url}: {err:#}");
                }
            }
        }
    }
}

extern "C" fn open_in_kaku_service(
    _self: &mut Object,
    _sel: Sel,
    pasteboard: *mut Object,
    _user_data: *mut Object,
    _error: *mut Object,
) {
    let Some(path) = first_service_path(pasteboard) else {
        log::warn!("openInKakuService: Finder provided no usable paths");
        return;
    };
    let path = normalize_finder_service_path(path);

    log::debug!("openInKakuService {path}");
    dispatch_or_queue_service_open(path, true);
}

extern "C" fn open_in_kaku_window_service(
    _self: &mut Object,
    _sel: Sel,
    pasteboard: *mut Object,
    _user_data: *mut Object,
    _error: *mut Object,
) {
    let Some(path) = first_service_path(pasteboard) else {
        log::warn!("openInKakuWindowService: Finder provided no usable paths");
        return;
    };
    let path = normalize_finder_service_path(path);

    log::debug!("openInKakuWindowService {path}");
    dispatch_or_queue_service_open(path, false);
}

extern "C" fn application_dock_menu(
    _self: &mut Object,
    _sel: Sel,
    _app: *mut Object,
) -> *mut Object {
    let dock_menu = Menu::new_with_title("");
    let new_window_item =
        MenuItem::new_with("New Window", Some(sel!(kakuPerformKeyAssignment:)), "");
    new_window_item
        .set_represented_item(RepresentedItem::KeyAssignment(KeyAssignment::SpawnWindow));
    dock_menu.add_item(&new_window_item);
    dock_menu.autorelease()
}

fn get_class() -> &'static Class {
    Class::get(CLS_NAME).unwrap_or_else(|| {
        let mut cls = ClassDecl::new(CLS_NAME, class!(NSObject))
            .expect("Unable to register application delegate class");

        cls.add_ivar::<BOOL>("launched");

        unsafe {
            cls.add_method(
                sel!(applicationShouldTerminate:),
                application_should_terminate as extern "C" fn(&mut Object, Sel, *mut Object) -> u64,
            );
            cls.add_method(
                sel!(applicationShouldTerminateAfterLastWindowClosed:),
                application_should_terminate_after_last_window_closed
                    as extern "C" fn(&mut Object, Sel, *mut Object) -> BOOL,
            );
            cls.add_method(
                sel!(applicationWillFinishLaunching:),
                application_will_finish_launching as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(applicationDidFinishLaunching:),
                application_did_finish_launching as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(application:openFile:),
                application_open_file
                    as extern "C" fn(&mut Object, Sel, *mut Object, *mut Object) -> BOOL,
            );
            cls.add_method(
                sel!(application:openFiles:),
                application_open_files as extern "C" fn(&mut Object, Sel, *mut Object, *mut Object),
            );
            cls.add_method(
                sel!(application:openURLs:),
                application_open_urls as extern "C" fn(&mut Object, Sel, *mut Object, *mut Object),
            );
            cls.add_method(
                sel!(applicationDockMenu:),
                application_dock_menu
                    as extern "C" fn(&mut Object, Sel, *mut Object) -> *mut Object,
            );
            cls.add_method(
                sel!(kakuPerformKeyAssignment:),
                kaku_perform_key_assignment as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            // macOS may route "Settings..." through one of these standard selectors
            // instead of our custom menu-item selector.
            cls.add_method(
                sel!(showSettingsWindow:),
                show_settings_window as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(showPreferencesWindow:),
                show_settings_window as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(applicationOpenUntitledFile:),
                application_open_untitled_file
                    as extern "C" fn(&mut Object, Sel, *mut Object) -> BOOL,
            );
            cls.add_method(
                sel!(openInKakuService:userData:error:),
                open_in_kaku_service
                    as extern "C" fn(&mut Object, Sel, *mut Object, *mut Object, *mut Object),
            );
            cls.add_method(
                sel!(openInKakuWindowService:userData:error:),
                open_in_kaku_window_service
                    as extern "C" fn(&mut Object, Sel, *mut Object, *mut Object, *mut Object),
            );
            cls.add_method(
                sel!(workspaceWillSleep:),
                workspace_will_sleep as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(screensDidWake:),
                screens_did_wake as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(screenParametersDidChange:),
                screen_parameters_did_change as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(keyboardSelectionDidChange:),
                keyboard_selection_did_change as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            cls.add_method(
                sel!(systemAppearanceDidChange:),
                system_appearance_did_change as extern "C" fn(&mut Object, Sel, *mut Object),
            );
        }

        cls.register()
    })
}

/// Creates the application delegate as a process-lifetime singleton.
/// The returned pointer is intentionally never released - NSApplication's
/// `setDelegate:` is `assign` (non-retaining), so the delegate must outlive
/// the application. Leaking a single small object for the entire process
/// lifetime is the correct ownership model here.
pub fn create_app_delegate() -> id {
    let cls = get_class();
    unsafe {
        let delegate: id = msg_send![cls, alloc];
        let delegate: id = msg_send![delegate, init];
        (*delegate).set_ivar("launched", NO);
        delegate
    }
}
