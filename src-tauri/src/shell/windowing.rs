use super::overlay_chrome;
use super::tray_icon::{status_icon, update_badge};
use super::tray_labels::{labels, update_label};
use crate::app_config::resolve_locale;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{IconMenuItem, IconMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{App, AppHandle, Manager, WebviewWindow, Wry};

const OPEN_ID: &str = "open-settings";
const TRANSLATION_ID: &str = "toggle-translation";
const OVERLAY_ID: &str = "toggle-overlay";
const UPDATE_ID: &str = "check-update";
const QUIT_ID: &str = "quit";

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
    /// An icon item from the start: an icon cannot be added to a plain item later.
    update: IconMenuItem<Wry>,
    quit: MenuItem<Wry>,
    tray: TrayIcon<Wry>,
    /// What the icon currently shows, so an unchanged state never redraws it.
    running: AtomicBool,
    /// Likewise for the badge, which is set from every status change.
    badged: AtomicBool,
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
    {
        app.handle()
            .set_activation_policy(tauri::ActivationPolicy::Accessory)?;
        forward_menu_shortcuts();
    }

    let labels = labels(&resolve_locale(locale));
    let version = app.package_info().version.to_string();
    let open = MenuItemBuilder::with_id(OPEN_ID, labels.open).build(app)?;
    let translation = MenuItemBuilder::with_id(TRANSLATION_ID, labels.start).build(app)?;
    let overlay = MenuItemBuilder::with_id(OVERLAY_ID, labels.show_overlay).build(app)?;
    let update =
        IconMenuItemBuilder::with_id(UPDATE_ID, update_label(&labels, &version)).build(app)?;
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
            id if id == UPDATE_ID => crate::updates::open_from_tray(app),
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
        badged: AtomicBool::new(false),
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
        for chrome in overlay_chrome::create(app.handle(), &overlay)? {
            configure_chrome(&chrome)?;
        }
        #[cfg(target_os = "macos")]
        super::overlay_frame::watch_screens(app.handle());
        let handle = app.handle().clone();
        // AppKit moves a child window with its parent; repositioning it again on every
        // move only makes it stutter. Windows offers no such coupling.
        overlay.on_window_event(move |event| {
            let follow = match event {
                tauri::WindowEvent::Resized(_) => true,
                tauri::WindowEvent::Moved(_) => !cfg!(target_os = "macos"),
                _ => return,
            };
            if follow {
                overlay_chrome::place(&handle);
            } else {
                super::overlay_pointer::refresh_overlay_bounds(&handle);
            }
        });
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        super::overlay_pointer::watch_cursor(app.handle());
    }
    Ok(())
}

#[tauri::command]
pub fn show_settings_window(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

/// The badge on the update entry; an unchanged one is left alone.
pub fn set_update_badge(app: &AppHandle, wanted: bool) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    if items.badged.swap(wanted, Ordering::Relaxed) != wanted {
        let _ = items.update.set_icon(wanted.then(update_badge));
    }
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
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .into(),
    );
    panel.set_hides_on_deactivate(false);
    panel.set_works_when_modal(true);
    round_corners(window);
    Ok(())
}

/// The window resizes a frame ahead of the webview, and while it shrinks the stale
/// picture is cut by the window's edge, so that edge is rounded too. A little under the
/// CSS radius (overlay.css), so the clip never eats into the antialiased edge the webview
/// draws. Windows keeps its square edge: `SetWindowRgn` set ahead of every resize still
/// showed the frame.
#[cfg(target_os = "macos")]
fn round_corners(window: &WebviewWindow) {
    use objc2_app_kit::NSWindow;

    const RADIUS: f64 = 16.0;
    let Ok(raw_window) = window.ns_window() else {
        return;
    };
    let raw_window = raw_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        let native_window = &*(raw_window as *mut NSWindow);
        let Some(view) = native_window.contentView() else {
            return;
        };
        view.setWantsLayer(true);
        let Some(layer) = view.layer() else {
            return;
        };
        layer.setCornerRadius(RADIUS);
        layer.setMasksToBounds(true);
    });
}

/// The same non-activating panel as the subtitles, so clicking a control never brings
/// the app forward or steals focus from whatever is being watched.
#[cfg(target_os = "macos")]
fn configure_chrome(window: &WebviewWindow) -> tauri::Result<()> {
    use tauri_nspanel::{CollectionBehavior, StyleMask, WebviewWindowExt};

    let panel = window.to_panel::<SubtitlePanel>()?;
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .into(),
    );
    panel.set_hides_on_deactivate(false);
    panel.set_works_when_modal(true);
    unconstrain(window);
    Ok(())
}

/// AppKit nudges any window it places back under the menu bar; the control windows hang
/// off the subtitle box wherever it is, off screen included, so the panel class gets a
/// `constrainFrameRect:toScreen:` that returns the rect untouched. The subtitle window
/// shares the class and is freed with it, which is what lets its rim drag it past the
/// screen's edge. Adding the method to the (dynamically registered) subclass overrides
/// NSWindow's; a second call for the same class is a no-op.
#[cfg(target_os = "macos")]
fn unconstrain(window: &WebviewWindow) {
    use objc2::encode::Encode;
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::sel;
    use objc2_foundation::NSRect;

    unsafe extern "C-unwind" fn keep(
        _this: *mut AnyObject,
        _cmd: Sel,
        rect: NSRect,
        _screen: *mut AnyObject,
    ) -> NSRect {
        rect
    }

    let Ok(raw_window) = window.ns_window() else {
        return;
    };
    let raw_window = raw_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        let native_window = &*(raw_window as *mut AnyObject);
        let class = native_window.class() as *const AnyClass as *mut AnyClass;
        let types = format!("{}@:{}@\0", NSRect::ENCODING, NSRect::ENCODING);
        let imp: unsafe extern "C-unwind" fn(
            *mut AnyObject,
            Sel,
            NSRect,
            *mut AnyObject,
        ) -> NSRect = keep;
        objc2::ffi::class_addMethod(
            class,
            sel!(constrainFrameRect:toScreen:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, NSRect, *mut AnyObject) -> NSRect,
                unsafe extern "C-unwind" fn(),
            >(imp),
            types.as_ptr().cast(),
        );
    });
}

/// AppKit stops handing Cmd shortcuts to the main menu once the app is an accessory, so
/// nothing reaches the Edit items behind Cmd+C, Cmd+V and friends. The event is offered to
/// the menu here instead, and swallowed when an item takes it.
#[cfg(target_os = "macos")]
fn forward_menu_shortcuts() {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags};
    use std::ptr::NonNull;

    let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        let pass = event.as_ptr();
        let event = unsafe { event.as_ref() };
        if !event
            .modifierFlags()
            .contains(NSEventModifierFlags::Command)
        {
            return pass;
        }
        let taken = MainThreadMarker::new()
            .and_then(|marker| NSApplication::sharedApplication(marker).mainMenu())
            .is_some_and(|menu| menu.performKeyEquivalent(event));
        if taken {
            std::ptr::null_mut()
        } else {
            pass
        }
    });
    unsafe {
        std::mem::forget(NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::KeyDown,
            &handler,
        ));
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_overlay(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_focusable(false)
}

#[cfg(not(target_os = "macos"))]
fn configure_chrome(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_focusable(false)
}

fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
