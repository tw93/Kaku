use crate::screen::Screens;
use crate::{Appearance, Connection, GeometryOrigin, RequestedWindowGeometry, ResolvedGeometry};
use anyhow::Result as Fallible;
use config::keyassignment::KeyAssignment;
use config::DimensionContext;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

thread_local! {
    static CONN: RefCell<Option<Rc<Connection>>> = RefCell::new(None);
}

fn nop_event_handler(_event: ApplicationEvent) {}

static EVENT_HANDLER: Mutex<fn(ApplicationEvent)> = Mutex::new(nop_event_handler);
static APP_EVENT_HANDLER_READY: AtomicBool = AtomicBool::new(false);
static STARTUP_PENDING_FIRST_WINDOW: AtomicBool = AtomicBool::new(false);

pub(crate) fn app_event_handler_ready() -> bool {
    APP_EVENT_HANDLER_READY.load(Ordering::Acquire)
}

pub fn mark_app_event_handler_ready() {
    APP_EVENT_HANDLER_READY.store(true, Ordering::Release);
}

/// Called synchronously by the GUI process during cold start, before any AppKit
/// event can fire, to claim ownership of creating the first window. While this is
/// set, macOS's applicationOpenUntitledFile must not spawn its own window; the
/// normal startup pipeline is responsible for the first one. The flag is cleared
/// as soon as that window registers (see `clear_startup_pending_first_window`),
/// so a later dock-icon reopen with no windows still spawns normally.
pub fn mark_startup_pending_first_window() {
    STARTUP_PENDING_FIRST_WINDOW.store(true, Ordering::Release);
}

pub(crate) fn startup_pending_first_window() -> bool {
    STARTUP_PENDING_FIRST_WINDOW.load(Ordering::Acquire)
}

pub(crate) fn clear_startup_pending_first_window() {
    STARTUP_PENDING_FIRST_WINDOW.store(false, Ordering::Release);
}

pub fn shutdown() {
    {
        let mut handler = EVENT_HANDLER
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *handler = nop_event_handler;
    }
    APP_EVENT_HANDLER_READY.store(false, Ordering::Release);
    STARTUP_PENDING_FIRST_WINDOW.store(false, Ordering::Release);
    CONN.with(|m| drop(m.borrow_mut().take()));
}

#[derive(Debug)]
pub enum ApplicationEvent {
    /// The system wants to open a command in the terminal
    OpenCommandScript(String),
    /// The system wants to open a command in a new tab when possible
    OpenCommandScriptInTab(String),
    /// The system requests focusing the tab/pane that owns this tty.
    ActivatePaneForTty(String),
    /// The system requests focusing a pane by pane id.
    ActivatePaneById(usize),
    /// The system requests focusing a tab by tab id.
    ActivateTabById(usize),
    /// Create the dedicated terminal used by the macOS global hotkey.
    #[cfg(target_os = "macos")]
    SpawnHotkeyWindow,
    PerformKeyAssignment(KeyAssignment),
}

pub trait ConnectionOps {
    fn get() -> Option<Rc<Connection>> {
        let mut res = None;
        CONN.with(|m| {
            if let Some(mux) = &*m.borrow() {
                res = Some(Rc::clone(mux));
            }
        });
        res
    }

    fn name(&self) -> String;

    fn set_event_handler(&self, func: fn(ApplicationEvent)) {
        let mut handler = EVENT_HANDLER
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *handler = func;
    }

    fn dispatch_app_event(&self, event: ApplicationEvent) {
        // Copy the function pointer while holding the lock, then invoke it
        // after unlocking to avoid lock reentry from callback code.
        let func = *EVENT_HANDLER
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        func(event);
    }

    fn default_dpi(&self) -> f64 {
        crate::DEFAULT_DPI
    }

    fn init() -> Fallible<Rc<Connection>> {
        let conn = Rc::new(Connection::create_new()?);
        CONN.with(|m| *m.borrow_mut() = Some(Rc::clone(&conn)));
        crate::spawn::SPAWN_QUEUE.register_promise_schedulers();
        Ok(conn)
    }

    fn terminate_message_loop(&self);
    fn run_message_loop(&self) -> Fallible<()>;

    /// Retrieve the current appearance for the application.
    fn get_appearance(&self) -> Appearance {
        Appearance::Light
    }

    /// Hide the application.
    /// This actions hides all of the windows of the application and switches
    /// focus away from it.
    fn hide_application(&self) {}

    /// Show a modal alert dialog
    fn alert(&self, _title: &str, _message: &str) {}

    /// Show a modal confirmation dialog
    /// Returns true if the primary button (e.g. "OK", "Download") was clicked
    fn confirm(&self, _title: &str, _message: &str, _action_label: &str) -> bool {
        false
    }

    /// Set this app as the system default terminal handler.
    fn set_default_terminal(&self) -> Fallible<()> {
        Err(anyhow::anyhow!(
            "setting default terminal is not supported on this platform"
        ))
    }

    /// Returns true if this app is already the system default terminal handler.
    fn is_default_terminal(&self) -> bool {
        false
    }

    /// Replay any queued platform service events once app event handlers are ready.
    fn flush_pending_service_events(&self) {}

    /// Synchronize platform global hotkey registration from current config.
    fn sync_global_hotkey(&self) {}

    /// Perform the system beep/notification sound
    fn beep(&self) {}

    /// Set the Dock badge label (macOS only).
    /// Pass None to clear the badge.
    fn set_dock_badge(&self, _label: Option<&str>) {}

    /// Returns information about the screens
    fn screens(&self) -> anyhow::Result<Screens> {
        anyhow::bail!("Unable to query screen information");
    }

    fn resolve_geometry(&self, geometry: RequestedWindowGeometry) -> ResolvedGeometry {
        let bounds = match self.screens() {
            Ok(screens) => {
                log::trace!("{screens:?}");

                match geometry.origin {
                    GeometryOrigin::ScreenCoordinateSystem => screens.virtual_rect,
                    GeometryOrigin::MainScreen => screens.main.rect,
                    GeometryOrigin::ActiveScreen => screens.active.rect,
                    GeometryOrigin::Named(name) => match screens.by_name.get(&name) {
                        Some(info) => info.rect,
                        None => {
                            log::error!(
                            "Requested display {} was not found; available displays are: {:?}. \
                             Using primary display instead",
                            name,
                            screens.by_name,
                        );
                            screens.main.rect
                        }
                    },
                }
            }
            Err(_) => euclid::rect(0, 0, 65535, 65535),
        };

        let dpi = self.default_dpi();
        let width_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: bounds.width() as f32,
            pixel_cell: bounds.width() as f32,
        };
        let height_context = DimensionContext {
            dpi: dpi as f32,
            pixel_max: bounds.height() as f32,
            pixel_cell: bounds.height() as f32,
        };
        let width = geometry.width.evaluate_as_pixels(width_context) as usize;
        let height = geometry.height.evaluate_as_pixels(height_context) as usize;
        let x = geometry
            .x
            .map(|x| x.evaluate_as_pixels(width_context) as i32 + bounds.origin.x as i32);
        let y = geometry
            .y
            .map(|y| y.evaluate_as_pixels(height_context) as i32 + bounds.origin.y as i32);

        ResolvedGeometry {
            x,
            y,
            width,
            height,
        }
    }
}
