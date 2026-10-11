mod macos;

use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ToastNotification {
    pub title: String,
    pub message: String,
    pub url: Option<String>,
    pub timeout: Option<std::time::Duration>,
}

impl ToastNotification {
    pub fn show(self) {
        show(self)
    }
}

use macos as backend;

pub fn show(notif: ToastNotification) {
    if let Err(err) = backend::show_notif(notif) {
        log::error!("Failed to show notification: {}", err);
    }
}

pub fn persistent_toast_notification_with_click_to_open_url(title: &str, message: &str, url: &str) {
    show(ToastNotification {
        title: title.to_string(),
        message: message.to_string(),
        url: Some(url.to_string()),
        timeout: None,
    });
}

pub fn persistent_toast_notification(title: &str, message: &str) {
    show(ToastNotification {
        title: title.to_string(),
        message: message.to_string(),
        url: None,
        timeout: None,
    });
}

#[cfg(target_os = "macos")]
pub use macos::initialize as macos_initialize;
#[cfg(target_os = "macos")]
pub use macos::{take_blocked_notice, NOTIFICATION_SETTINGS_URL};

static UPDATE_CALLBACK: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

pub fn set_update_click_callback<F: Fn() + Send + 'static>(callback: F) {
    *UPDATE_CALLBACK.lock().unwrap() = Some(Box::new(callback));
}

pub(crate) fn invoke_update_callback() -> bool {
    if let Some(cb) = UPDATE_CALLBACK.lock().unwrap().as_ref() {
        cb();
        true
    } else {
        false
    }
}
