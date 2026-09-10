use super::tray_icon::status_icon;
use super::tray_labels::{labels, update_label};
use crate::app_config::resolve_locale;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{App, AppHandle, Manager, WebviewWindow, Wry};

const OPEN_ID: &str = "open-settings";
const TRANSLATION_ID: &str = "toggle-translation";
const OVERLAY_ID: &str = "toggle-overlay";
const UPDATE_ID: &str = "check-update";
const QUIT_ID: &str = "quit";
const RELEASES_URL: &str = "https://github.com/Deanwfy/OverLingo/releases/latest";
#[cfg(target_os = "macos")]
const OUTSIDE_CLICK_EVENT: &str = "overlay://outside-click";

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(SubtitlePanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            becomes_key_only_if_needed: true
        }
    })
}

pub struct TrayItems {
    open: MenuItem<Wry>,
    translation: MenuItem<Wry>,
    overlay: MenuItem<Wry>,
    update: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    tray: TrayIcon<Wry>,
    /// What the icon currently shows, so an unchanged state never redraws it.
    running: AtomicBool,
}

pub struct TrayPresentation {
    pub locale: String,
    /// A session exists, paused or not: what the menu offers to end.
    pub translation_active: bool,
    /// Audio is actually being translated: what the icon reports.
    pub translation_running: bool,
    pub overlay_visible: bool,
}

pub fn install(app: &mut App, locale: &str) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    app.handle()
        .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

    let labels = labels(&resolve_locale(locale));
    let version = app.package_info().version.to_string();
    let open = MenuItemBuilder::with_id(OPEN_ID, labels.open).build(app)?;
    let translation = MenuItemBuilder::with_id(TRANSLATION_ID, labels.start).build(app)?;
    let overlay = MenuItemBuilder::with_id(OVERLAY_ID, labels.show_overlay).build(app)?;
    let update = MenuItemBuilder::with_id(UPDATE_ID, update_label(&labels, &version)).build(app)?;
    let quit = MenuItemBuilder::with_id(QUIT_ID, labels.quit).build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .item(&translation)
        .item(&overlay)
        .separator()
        .item(&update)
        .item(&quit)
        .build()?;

    let tray = TrayIconBuilder::with_id("translation-status")
        .icon(status_icon(false))
        .icon_as_template(true)
        .tooltip("OverLingo")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id() {
            id if id == OPEN_ID => {
                let _ = show_settings(app);
            }
            id if id == TRANSLATION_ID => {
                let _ = app
                    .state::<crate::controller::AppController>()
                    .request(crate::controller::ControllerRequest::ToggleTranslation);
            }
            id if id == OVERLAY_ID => {
                let _ = app
                    .state::<crate::controller::AppController>()
                    .request(crate::controller::ControllerRequest::ToggleOverlay);
            }
            id if id == UPDATE_ID => {
                use tauri_plugin_opener::OpenerExt;
                let _ = app.opener().open_url(RELEASES_URL, None::<&str>);
            }
            id if id == QUIT_ID => {
                let _ = app
                    .state::<crate::controller::AppController>()
                    .request(crate::controller::ControllerRequest::Exit);
            }
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems {
        open,
        translation,
        overlay,
        update,
        quit,
        tray,
        running: AtomicBool::new(false),
    });

    if let Some(main) = app.get_webview_window("main") {
        let window = main.clone();
        main.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        });
    }
    if let Some(overlay) = app.get_webview_window("overlay") {
        configure_overlay(&overlay)?;
    }
    Ok(())
}

#[tauri::command]
pub fn show_settings_window(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

pub fn update_tray_for_app(app: &AppHandle, presentation: TrayPresentation) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let _ = apply_tray(
        &items,
        &presentation,
        &app.package_info().version.to_string(),
    );
}

fn apply_tray(
    items: &TrayItems,
    presentation: &TrayPresentation,
    version: &str,
) -> Result<(), String> {
    let labels = labels(&resolve_locale(&presentation.locale));
    items.open.set_text(labels.open).map_err(error_text)?;
    items
        .translation
        .set_text(if presentation.translation_active {
            labels.end
        } else {
            labels.start
        })
        .map_err(error_text)?;
    items
        .overlay
        .set_text(if presentation.overlay_visible {
            labels.hide_overlay
        } else {
            labels.show_overlay
        })
        .map_err(error_text)?;
    items
        .update
        .set_text(update_label(&labels, version))
        .map_err(error_text)?;
    items.quit.set_text(labels.quit).map_err(error_text)?;
    if items
        .running
        .swap(presentation.translation_running, Ordering::Relaxed)
        != presentation.translation_running
    {
        items
            .tray
            .set_icon(Some(status_icon(presentation.translation_running)))
            .map_err(error_text)?;
        // The template flag belongs to the image, not the tray, so it has to be restated
        // for the replacement; without it the glyph stays black on a dark menu bar.
        #[cfg(target_os = "macos")]
        items.tray.set_icon_as_template(true).map_err(error_text)?;
    }
    Ok(())
}

pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Settings window is unavailable".to_string())?;
    window.show().map_err(error_text)?;
    window.unminimize().map_err(error_text)?;
    activate_and_focus(app, &window)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn activate_and_focus(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindow};

    let raw_window = window.ns_window().map_err(error_text)? as usize;
    app.run_on_main_thread(move || unsafe {
        let marker = MainThreadMarker::new().expect("main thread");
        let application = NSApplication::sharedApplication(marker);
        // The Accessory app lacks an activation context, so the `activate()` call is ignored by the system, leaving the window behind the foreground app.
        #[allow(deprecated)]
        application.activateIgnoringOtherApps(true);
        let native_window = &*(raw_window as *mut NSWindow);
        native_window.makeKeyAndOrderFront(None);
    })
    .map_err(error_text)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn activate_and_focus(_app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    window.set_focus().map_err(error_text)
}

#[cfg(target_os = "macos")]
fn configure_overlay(window: &WebviewWindow) -> tauri::Result<()> {
    use tauri_nspanel::{CollectionBehavior, StyleMask, WebviewWindowExt};

    let panel = window.to_panel::<SubtitlePanel>()?;
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().resizable().into());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .into(),
    );
    panel.set_hides_on_deactivate(false);
    panel.set_works_when_modal(true);
    watch_outside_clicks(window);
    Ok(())
}

#[cfg(target_os = "macos")]
fn watch_outside_clicks(window: &WebviewWindow) {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask};
    use std::ptr::NonNull;
    use tauri::Emitter;

    let target = window.clone();
    let handler = RcBlock::new(move |_event: NonNull<NSEvent>| {
        let _ = target.emit(OUTSIDE_CLICK_EVENT, ());
    });
    let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown,
        &handler,
    );
    std::mem::forget(monitor);
}

#[cfg(not(target_os = "macos"))]
fn configure_overlay(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_focusable(false)
}

fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
