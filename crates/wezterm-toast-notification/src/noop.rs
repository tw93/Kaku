use crate::ToastNotification;

pub fn show_notif(toast: ToastNotification) -> Result<(), Box<dyn std::error::Error>> {
    log::debug!(
        "Toast notification (no backend): title={}, message={}",
        toast.title,
        toast.message
    );
    Ok(())
}
