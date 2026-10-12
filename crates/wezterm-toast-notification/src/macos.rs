#![cfg(target_os = "macos")]
#![allow(clippy::borrow_interior_mutable_const)]
#![allow(clippy::declare_interior_mutable_const)]
use crate::ToastNotification;
use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_foundation::{ns_string, NSArray, NSBundle, NSDictionary, NSError, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNNotificationSettings, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Once;

/// Opens System Settings > Notifications > Kaku.
pub const NOTIFICATION_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=fun.tw93.kaku";

const BLOCKED_NONE: u8 = 0;
const BLOCKED_PENDING: u8 = 1;
const BLOCKED_SURFACED: u8 = 2;

/// Once macOS denies authorization it never asks again, and a denied app's
/// notifications vanish without an error. Remember that one was dropped so
/// the GUI can say so once per launch instead of failing silently.
static BLOCKED_NOTICE: AtomicU8 = AtomicU8::new(BLOCKED_NONE);

fn mark_blocked(state: &AtomicU8) -> bool {
    state
        .compare_exchange(
            BLOCKED_NONE,
            BLOCKED_PENDING,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
}

fn take_blocked(state: &AtomicU8) -> bool {
    state
        .compare_exchange(
            BLOCKED_PENDING,
            BLOCKED_SURFACED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
}

/// True once per launch, after a notification was dropped because
/// notifications are turned off for Kaku in System Settings.
pub fn take_blocked_notice() -> bool {
    take_blocked(&BLOCKED_NOTICE)
}

fn note_if_blocked(center: &UNUserNotificationCenter) {
    center.getNotificationSettingsWithCompletionHandler(&RcBlock::new(
        |settings: NonNull<UNNotificationSettings>| {
            let settings = unsafe { settings.as_ref() };
            if settings.authorizationStatus() == UNAuthorizationStatus::Denied
                && mark_blocked(&BLOCKED_NOTICE)
            {
                log::warn!(
                    "notification dropped: notifications are turned off for Kaku in \
                     System Settings > Notifications"
                );
            }
        },
    ));
}

fn has_valid_bundle_identifier() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some()
}

const NEEDS_SIGN: &str = "Note that the application must be code-signed \
                          for UNUserNotificationCenter to work";

fn ns_error_to_string(err: *mut NSError) -> String {
    if err.is_null() {
        "null error".to_string()
    } else {
        unsafe {
            let err: &NSError = &*err;
            format!(
                "{} {:?}",
                err.localizedDescription(),
                err.localizedFailureReason()
            )
        }
    }
}

define_class!(
    #[unsafe(super = NSObject)]
    #[name = "WezTermNotifDelegate"]
    #[derive(Debug)]
    struct NotifDelegate;

    unsafe impl NSObjectProtocol for NotifDelegate {}
    unsafe impl UNUserNotificationCenterDelegate for NotifDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        unsafe fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::Block<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            log::debug!("will_present");
            let options = UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound
                | UNNotificationPresentationOptions::Badge
                | UNNotificationPresentationOptions::Banner;
            completion_handler.call((options,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        unsafe fn did_receive_notification(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &Block<dyn Fn()>,
        ) {
            let action = response.actionIdentifier();
            let user_info = response.notification().request().content().userInfo();
            let url = user_info.valueForKey(ns_string!("url"));

            log::debug!("did_receive_notification -> action={action:?} url={url:?}");

            if let Some(url) = url {
                if let Ok(url_str) = url.downcast::<NSString>() {
                    let url_string = url_str.to_string();
                    if url_string == "kaku://update" {
                        spawn_kaku_update();
                    } else {
                        wezterm_open_url::open_url(&url_string);
                    }
                }
            }

            completion_handler.call(());
        }
    }
);

impl NotifDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        let me: Retained<Self> = unsafe { msg_send![super(this), init] };
        log::debug!("new delegate {:?}", Retained::as_ptr(&me));
        me
    }
}

impl Drop for NotifDelegate {
    fn drop(&mut self) {
        log::debug!("dropping NotifDelegate {:?}", self as *mut Self);
    }
}

fn get_notification_center() -> Option<Retained<UNUserNotificationCenter>> {
    if has_valid_bundle_identifier() {
        Some(UNUserNotificationCenter::currentNotificationCenter())
    } else {
        None
    }
}

pub fn initialize() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let Some(center) = get_notification_center() else {
            log::warn!(
                "UNUserNotificationCenter unavailable: no valid bundle identifier. \
                 Notifications are disabled when running outside an app bundle."
            );
            return;
        };

        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert
                | UNAuthorizationOptions::Sound
                | UNAuthorizationOptions::Badge,
            &RcBlock::new(|ok: Bool, err| {
                if ok.is_false() {
                    log::error!(
                        "requestAuthorization status={ok:?} {}. {NEEDS_SIGN}",
                        ns_error_to_string(err)
                    );
                }
            }),
        );

        let show_url = UNNotificationAction::actionWithIdentifier_title_options(
            ns_string!("SHOW_URL"),
            ns_string!("Show"),
            UNNotificationActionOptions::empty(),
        );
        let show_url_cat =
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                ns_string!("SHOW_URL_ACTION"),
                &NSArray::from_retained_slice(&[show_url]),
                &NSArray::from_slice(&[]),
                UNNotificationCategoryOptions::CustomDismissAction,
            );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[show_url_cat]));

        let delegate = NotifDelegate::new();
        let delegate_proto = ProtocolObject::from_retained(delegate.clone());
        center.setDelegate(Some(&delegate_proto));
        log::debug!(
            "after setDelegate {:?}, center.delegate={:?}",
            delegate,
            center.delegate()
        );

        // Intentionally "leak" the delegate.
        // I've tried stashing it into a global to keep it alive,
        // but something still manages to drop the underlying delegate
        // and that will break the weak ref in the center.
        // This is likely not the right way to do this, but after
        // spending two hours scratching my head, this is the least
        // crazy thing.
        Retained::into_raw(delegate);
    });
}

pub fn show_notif(toast: ToastNotification) -> Result<(), Box<dyn std::error::Error>> {
    initialize();

    let Some(center) = get_notification_center() else {
        return Err("Notifications unavailable: no valid bundle identifier".into());
    };

    // Still post the request below: if the user has just turned
    // notifications back on, the status read here may be stale.
    note_if_blocked(&center);

    unsafe {
        log::debug!("show_notif center.delegate is {:?}", center.delegate());

        let notif = UNMutableNotificationContent::new();
        notif.setTitle(&NSString::from_str(&toast.title));
        notif.setBody(&NSString::from_str(&toast.message));

        if let Some(url) = &toast.url {
            let info =
                NSDictionary::from_slices(&[ns_string!("url")], &[&*NSString::from_str(url)]);
            notif.setUserInfo(
                info.downcast_ref::<NSDictionary>()
                    .expect("is NSDictionary"),
            );
            notif.setCategoryIdentifier(ns_string!("SHOW_URL_ACTION"));
        }

        let identifier = uuid::Uuid::new_v4().to_string();
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&identifier),
            &notif,
            None,
        );

        center.addNotificationRequest_withCompletionHandler(
            &request,
            Some(&RcBlock::new(move |err: *mut NSError| {
                if err.is_null() {
                    if let Some(timeout) = toast.timeout {
                        // Spawn a thread to wait. This could be more efficient.
                        // We cannot simply use performSelector:withObject:afterDelay:
                        // because we're not guaranteed to be called from the main
                        // thread.  We also don't have access to the executor machinery
                        // from the window crate here, so we just do this basic take.
                        let identifier = identifier.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(timeout);
                            // Remove this notification
                            if let Some(center) = get_notification_center() {
                                let ident_array =
                                    NSArray::from_retained_slice(&[NSString::from_str(
                                        &identifier,
                                    )]);
                                center.removeDeliveredNotificationsWithIdentifiers(&ident_array);
                            }
                        });
                    }
                } else {
                    log::error!("notif failed {}. {NEEDS_SIGN}", ns_error_to_string(err));
                }
            })),
        );
    }

    Ok(())
}

fn spawn_kaku_update() {
    if crate::invoke_update_callback() {
        log::info!("spawn_kaku_update: handled via registered callback");
        return;
    }

    // Fallback: spawn a new window if no callback is registered
    log::info!("spawn_kaku_update: no callback registered, falling back to direct spawn");
    std::thread::spawn(|| {
        let kaku_gui = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.join("kaku-gui")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| {
                std::path::PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku-gui")
            });

        let kaku_cli = kaku_gui
            .parent()
            .map(|p| p.join("kaku"))
            .filter(|p| p.exists())
            .unwrap_or_else(|| {
                std::path::PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku")
            });

        let result = std::process::Command::new(&kaku_gui)
            .args(["start", "--", kaku_cli.to_str().unwrap_or("kaku"), "update"])
            .spawn();

        match result {
            Ok(_) => log::info!("spawn_kaku_update: process spawned successfully"),
            Err(e) => log::error!("spawn_kaku_update: failed to spawn: {}", e),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_notice_surfaces_once_per_launch() {
        let state = AtomicU8::new(BLOCKED_NONE);
        assert!(!take_blocked(&state));
        assert!(mark_blocked(&state));
        assert!(!mark_blocked(&state));
        assert!(take_blocked(&state));
        assert!(!take_blocked(&state));
        // Later drops in the same launch stay quiet.
        assert!(!mark_blocked(&state));
        assert!(!take_blocked(&state));
    }
}
