//! The desktop shell around the webviews: windows, tray, and the overlay's pointer watcher.

pub mod overlay_chrome;
pub mod overlay_frame;
pub mod overlay_pointer;
mod tray_icon;
mod tray_labels;
pub mod windowing;

pub use windowing::{
    install, set_update_badge, show_settings, update_tray_for_app, TrayPresentation,
};
