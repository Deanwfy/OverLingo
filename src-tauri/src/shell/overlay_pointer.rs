use super::overlay_chrome::{self, bounds, ChromeFrames, OVERLAY_LABEL};
use crate::geometry::Rect;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// The controls sit in their own window, so CSS :hover cannot span the gap between them
/// and the subtitles, and a click-through window receives no mouse events at all. The
/// cursor is watched from outside instead: macOS reports every move through an event
/// monitor, Windows through a low-level mouse hook.
const HOVER_EVENT: &str = "overlay://pointer-hover";
/// WKWebView only tracks the mouse in the key window, and the toolbar never takes key,
/// so it is told where the cursor is and draws its own hover state.
const POINTER_EVENT: &str = "overlay://pointer-at";
/// Slack around each window so crossing the gap between them does not blink the
/// controls, in CSS pixels.
const HOVER_SLACK: f64 = 6.0;
/// Band inside the window's edges that keeps taking the cursor while clicks pass
/// through: the move and resize handles drawn in overlay.css, in CSS pixels.
const EDGE: f64 = 9.0;

static VISIBLE: AtomicBool = AtomicBool::new(false);
static HOVERING: AtomicBool = AtomicBool::new(false);
static PASSING_THROUGH: AtomicBool = AtomicBool::new(false);
static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);
static POINTER_IN_TOOLBAR: AtomicBool = AtomicBool::new(false);
/// Height of a strip below the box's top that stays clickable while clicks pass through,
/// in CSS pixels: a failure notice with buttons in it.
static INTERACTIVE_HEIGHT: AtomicU32 = AtomicU32::new(0);
/// Window geometry is refreshed when the windows move, not on every cursor move: each
/// query is a round trip through the main thread.
static OVERLAY: Mutex<Option<Rect>> = Mutex::new(None);
/// The chrome windows relative to the overlay's top-left corner, so they can be derived
/// from the overlay's position while a drag moves them all.
static CHROME_OFFSETS: Mutex<ChromeFrames> = Mutex::new(ChromeFrames {
    toolbar: None,
    panel: None,
});

#[tauri::command]
pub fn set_overlay_interactive_height(height: u32) {
    INTERACTIVE_HEIGHT.store(height, Ordering::Relaxed);
}

pub fn apply(app: &AppHandle, visible: bool, click_through: bool) {
    VISIBLE.store(visible, Ordering::SeqCst);
    CLICK_THROUGH.store(click_through, Ordering::Relaxed);
    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    if !visible {
        set_hovering(app, false);
        set_passing_through(&overlay, false);
        return;
    }
    refresh_overlay_bounds(app);
    update(app);
}

pub fn refresh_overlay_bounds(app: &AppHandle) {
    let rect = app
        .get_webview_window(OVERLAY_LABEL)
        .and_then(|overlay| bounds(&overlay));
    if let Ok(mut overlay) = OVERLAY.lock() {
        *overlay = rect;
    }
}

pub fn set_chrome_frames(subtitle_box: Rect, frames: ChromeFrames) {
    if let (Ok(mut overlay), Ok(mut offsets)) = (OVERLAY.lock(), CHROME_OFFSETS.lock()) {
        *overlay = Some(subtitle_box);
        *offsets = frames.map(|frame| Rect {
            x: frame.x - subtitle_box.x,
            y: frame.y - subtitle_box.y,
            ..frame
        });
    }
}

/// Re-reads the cursor where it is: for when the windows changed under it.
pub fn update(app: &AppHandle) {
    if let Some((x, y)) = app
        .cursor_position()
        .ok()
        .and_then(|cursor| screen_point(app, cursor.x, cursor.y))
    {
        update_at(app, x, y);
    }
}

/// Physical pixels, as the cursor query and the mouse hook report them, to screen
/// coordinates.
fn screen_point(app: &AppHandle, x: f64, y: f64) -> Option<(f64, f64)> {
    let overlay = app.get_webview_window(OVERLAY_LABEL)?;
    Some(overlay_chrome::to_screen(&overlay, x, y))
}

/// The control windows in screen coordinates, derived from where the overlay is now.
fn chrome_frames(window: Rect) -> ChromeFrames {
    CHROME_OFFSETS
        .lock()
        .map_or(ChromeFrames::default(), |offsets| {
            offsets.map(|offset| Rect {
                x: window.x + offset.x,
                y: window.y + offset.y,
                ..offset
            })
        })
}

/// `x`, `y` in screen coordinates.
pub fn update_at(app: &AppHandle, x: f64, y: f64) {
    if !VISIBLE.load(Ordering::SeqCst) {
        return;
    }
    if overlay_chrome::dragging() {
        overlay_chrome::drag_to(app, x, y);
        return;
    }
    let (Some(window), Some(overlay)) = (
        OVERLAY.lock().ok().and_then(|overlay| *overlay),
        app.get_webview_window(OVERLAY_LABEL),
    ) else {
        return;
    };
    let scale = overlay_chrome::screen_scale(&overlay);
    let slack = HOVER_SLACK * scale;
    let chrome = chrome_frames(window);
    let near = |rect: Option<Rect>| rect.is_some_and(|rect| rect.inset(-slack).contains(x, y));
    let hovering = overlay_chrome::panel_open()
        || window.inset(-slack).contains(x, y)
        || near(chrome.toolbar)
        || near(chrome.panel);
    set_hovering(app, hovering);
    let in_toolbar = chrome.toolbar.filter(|toolbar| toolbar.contains(x, y));
    if let Some(toolbar) = in_toolbar {
        // Handed back in the toolbar webview's own CSS pixels.
        let toolbar_scale = app
            .get_webview_window(overlay_chrome::TOOLBAR_LABEL)
            .map_or(1.0, |window| overlay_chrome::screen_scale(&window));
        let _ = app.emit_to(
            overlay_chrome::TOOLBAR_LABEL,
            POINTER_EVENT,
            Some((
                (x - toolbar.x) / toolbar_scale,
                (y - toolbar.y) / toolbar_scale,
            )),
        );
    } else if POINTER_IN_TOOLBAR.load(Ordering::Relaxed) {
        let _ = app.emit_to(
            overlay_chrome::TOOLBAR_LABEL,
            POINTER_EVENT,
            None::<(f64, f64)>,
        );
    }
    POINTER_IN_TOOLBAR.store(in_toolbar.is_some(), Ordering::Relaxed);
    let strip = f64::from(INTERACTIVE_HEIGHT.load(Ordering::Relaxed)) * scale;
    let passing_through =
        CLICK_THROUGH.load(Ordering::Relaxed) && !reachable(window, x, y, strip, EDGE * scale);
    set_passing_through(&overlay, passing_through);
}

/// While clicks pass through, the edges of the box and the notice strip below its top
/// still take the cursor.
fn reachable(window: Rect, x: f64, y: f64, strip: f64, edge: f64) -> bool {
    window.contains(x, y) && (!window.inset(edge).contains(x, y) || y < window.y + strip)
}

/// The controls window is ordered in and out rather than told to ignore the cursor: a
/// window that stopped ignoring it does not resume hover tracking until re-entered, and
/// a hidden one cannot shadow whatever the user has under it.
fn set_hovering(app: &AppHandle, hovering: bool) {
    if HOVERING.swap(hovering, Ordering::Relaxed) == hovering {
        return;
    }
    let _ = app.emit_to(OVERLAY_LABEL, HOVER_EVENT, hovering);
    let _ = app.emit_to(overlay_chrome::TOOLBAR_LABEL, HOVER_EVENT, hovering);
    if hovering {
        overlay_chrome::reveal(app);
    } else {
        // Leave time for the fade-out, unless the cursor came back meanwhile.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(160)).await;
            if !HOVERING.load(Ordering::Relaxed) {
                overlay_chrome::conceal(&app);
            }
        });
    }
}

fn set_passing_through(overlay: &WebviewWindow, passing_through: bool) {
    if PASSING_THROUGH.swap(passing_through, Ordering::Relaxed) != passing_through {
        let _ = overlay.set_ignore_cursor_events(passing_through);
    }
}

/// Global monitors only see events bound for other apps, which is exactly where the
/// cursor is while clicks pass through; the local one covers our own windows. Presses
/// only matter elsewhere, so they get a global monitor alone.
#[cfg(target_os = "macos")]
pub fn watch_cursor(app: &AppHandle) {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSEventMask, NSScreen};
    use std::ptr::NonNull;

    // AppKit measures from the bottom of the first screen; screen coordinates here
    // grow downwards from its top.
    let cursor = || {
        let marker = MainThreadMarker::new()?;
        let screen = NSScreen::screens(marker).firstObject()?;
        let location = NSEvent::mouseLocation();
        Some((location.x, screen.frame().size.height - location.y))
    };
    let moves = NSEventMask::MouseMoved | NSEventMask::LeftMouseDragged;
    let presses =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let report = {
        let app = app.clone();
        move || {
            if let Some((x, y)) = cursor() {
                update_at(&app, x, y);
            }
        }
    };
    let global_moves = {
        let report = report.clone();
        RcBlock::new(move |_event: NonNull<NSEvent>| report())
    };
    let local_moves = RcBlock::new(move |event: NonNull<NSEvent>| {
        report();
        event.as_ptr()
    });
    let global_presses = {
        let app = app.clone();
        RcBlock::new(move |_event: NonNull<NSEvent>| {
            if let Some((x, y)) = cursor() {
                press_at(&app, x, y);
            }
        })
    };
    unsafe {
        std::mem::forget(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            moves,
            &global_moves,
        ));
        std::mem::forget(NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            moves,
            &local_moves,
        ));
        std::mem::forget(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            presses,
            &global_presses,
        ));
    }
}

/// A press anywhere but on the controls closes the settings panel, like a popover. The
/// Windows hook sees presses on our own windows too, hence the check against the
/// frames; a press on the subtitle box is reported by its webview on both platforms.
fn press_at(app: &AppHandle, x: f64, y: f64) {
    if !overlay_chrome::panel_open() {
        return;
    }
    let Some(window) = OVERLAY.lock().ok().and_then(|overlay| *overlay) else {
        return;
    };
    let chrome = chrome_frames(window);
    let on_controls = [chrome.toolbar, chrome.panel]
        .into_iter()
        .flatten()
        .any(|rect| rect.contains(x, y));
    if !on_controls {
        let _ = app.emit_to(
            overlay_chrome::PANEL_LABEL,
            overlay_chrome::OUTSIDE_CLICK_EVENT,
            (),
        );
    }
}

#[cfg(target_os = "windows")]
static HOOK_APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// A low-level mouse hook is the one way to see the cursor over other applications'
/// windows without polling. It is called on the thread that installed it, and only while
/// that thread pumps messages, so it goes on the main thread like the macOS monitors.
#[cfg(target_os = "windows")]
pub fn watch_cursor(app: &AppHandle) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_MOUSE_LL};

    if HOOK_APP.set(app.clone()).is_err() {
        return;
    }
    let _ = app.run_on_main_thread(|| unsafe {
        // Kept for the life of the process, so the handle is dropped rather than unhooked.
        let _ = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0);
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn mouse_hook(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE,
        WM_RBUTTONDOWN,
    };

    // A negative code means the event is not ours to look at. The hook has to return
    // quickly or Windows silently removes it, so nothing is looked up while hidden.
    let point = (code >= 0 && VISIBLE.load(Ordering::SeqCst))
        .then(|| (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt);
    if let (Some(point), Some(app)) = (point, HOOK_APP.get()) {
        if let Some((x, y)) = screen_point(app, f64::from(point.x), f64::from(point.y)) {
            match wparam.0 as u32 {
                WM_MOUSEMOVE => update_at(app, x, y),
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                    update_at(app, x, y);
                    press_at(app, x, y);
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Rect = Rect {
        x: 100.0,
        y: 100.0,
        width: 400.0,
        height: 200.0,
    };

    #[test]
    fn keeps_the_edges_and_the_notice_strip_reachable() {
        assert!(reachable(WINDOW, 102.0, 150.0, 0.0, EDGE));
        assert!(reachable(WINDOW, 300.0, 296.0, 0.0, EDGE));
        assert!(!reachable(WINDOW, 300.0, 200.0, 0.0, EDGE));
        assert!(reachable(WINDOW, 300.0, 140.0, 44.0, EDGE));
        assert!(!reachable(WINDOW, 300.0, 160.0, 44.0, EDGE));
        assert!(reachable(WINDOW, 108.0, 150.0, 0.0, EDGE));
        assert!(!reachable(WINDOW, 109.0, 150.0, 0.0, EDGE));
        assert!(!reachable(WINDOW, 99.0, 150.0, 44.0, EDGE));
    }

    #[test]
    fn stores_the_interactive_height() {
        set_overlay_interactive_height(52);
        assert_eq!(INTERACTIVE_HEIGHT.load(Ordering::Relaxed), 52);
    }
}
