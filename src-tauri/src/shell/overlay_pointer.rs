use super::overlay_chrome::{self, bounds, ChromeFrames, Rect, OVERLAY_LABEL};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// The controls sit in their own window, so CSS :hover cannot span the gap between them
/// and the subtitles, and a click-through window receives no mouse events at all. The
/// cursor is watched from outside instead: macOS reports every move through an event
/// monitor, elsewhere it is polled.
const HOVER_EVENT: &str = "overlay://pointer-hover";
/// WKWebView only tracks the mouse in the key window, and the toolbar never takes key,
/// so it is told where the cursor is and draws its own hover state.
const POINTER_EVENT: &str = "overlay://pointer-at";
/// Slack around each window so crossing the gap between them does not blink the controls.
const HOVER_SLACK: f64 = 6.0;
/// Band inside the window's edges that keeps taking the cursor while clicks pass
/// through: the move and resize handles drawn in overlay.css.
const EDGE: f64 = 9.0;

static VISIBLE: AtomicBool = AtomicBool::new(false);
static HOVERING: AtomicBool = AtomicBool::new(false);
static PASSING_THROUGH: AtomicBool = AtomicBool::new(false);
static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);
static POINTER_IN_TOOLBAR: AtomicBool = AtomicBool::new(false);
/// Height of a strip below the box's top that stays clickable while clicks pass through,
/// in logical pixels: a failure notice with buttons in it.
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
    let was_visible = VISIBLE.swap(visible, Ordering::SeqCst);
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
    #[cfg(not(target_os = "macos"))]
    if !was_visible {
        tauri::async_runtime::spawn(poll(app.clone()));
    }
    #[cfg(target_os = "macos")]
    let _ = was_visible;
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

#[cfg(not(target_os = "macos"))]
async fn poll(app: AppHandle) {
    while VISIBLE.load(Ordering::SeqCst) {
        let interval = if overlay_chrome::dragging() { 8 } else { 40 };
        tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
        update(&app);
    }
}

/// Re-reads the cursor where it is: for when the windows changed under it.
pub fn update(app: &AppHandle) {
    let (Ok(cursor), Some(scale)) = (
        app.cursor_position(),
        app.get_webview_window(OVERLAY_LABEL)
            .and_then(|overlay| overlay.scale_factor().ok()),
    ) else {
        return;
    };
    let cursor = cursor.to_logical::<f64>(scale);
    update_at(app, cursor.x, cursor.y);
}

/// `x`, `y` in logical screen coordinates.
pub fn update_at(app: &AppHandle, x: f64, y: f64) {
    if !VISIBLE.load(Ordering::SeqCst) {
        return;
    }
    if overlay_chrome::dragging() {
        overlay_chrome::drag_to(app, x, y);
        return;
    }
    let Some(window) = OVERLAY.lock().ok().and_then(|overlay| *overlay) else {
        return;
    };
    let chrome = CHROME_OFFSETS
        .lock()
        .map_or(ChromeFrames::default(), |offsets| {
            offsets.map(|offset| Rect {
                x: window.x + offset.x,
                y: window.y + offset.y,
                ..offset
            })
        });
    let near =
        |rect: Option<Rect>| rect.is_some_and(|rect| rect.inset(-HOVER_SLACK).contains(x, y));
    let hovering = overlay_chrome::panel_open()
        || window.inset(-HOVER_SLACK).contains(x, y)
        || near(chrome.toolbar)
        || near(chrome.panel);
    set_hovering(app, hovering);
    let in_toolbar = chrome.toolbar.filter(|toolbar| toolbar.contains(x, y));
    if let Some(toolbar) = in_toolbar {
        let _ = app.emit_to(
            overlay_chrome::TOOLBAR_LABEL,
            POINTER_EVENT,
            Some((x - toolbar.x, y - toolbar.y)),
        );
    } else if POINTER_IN_TOOLBAR.load(Ordering::Relaxed) {
        let _ = app.emit_to(
            overlay_chrome::TOOLBAR_LABEL,
            POINTER_EVENT,
            None::<(f64, f64)>,
        );
    }
    POINTER_IN_TOOLBAR.store(in_toolbar.is_some(), Ordering::Relaxed);
    let strip = f64::from(INTERACTIVE_HEIGHT.load(Ordering::Relaxed));
    let passing_through = CLICK_THROUGH.load(Ordering::Relaxed) && !reachable(window, x, y, strip);
    if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
        set_passing_through(&overlay, passing_through);
    }
}

/// While clicks pass through, the edges of the box and the notice strip below its top
/// still take the cursor.
fn reachable(window: Rect, x: f64, y: f64, strip: f64) -> bool {
    window.contains(x, y) && (!window.inset(EDGE).contains(x, y) || y < window.y + strip)
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
/// cursor is while clicks pass through; the local one covers our own windows.
#[cfg(target_os = "macos")]
pub fn watch_cursor(app: &AppHandle) {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSEventMask, NSScreen};
    use std::ptr::NonNull;

    let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseDragged;
    let handle = app.clone();
    let report = move || {
        let Some(marker) = MainThreadMarker::new() else {
            return;
        };
        let Some(screen) = NSScreen::screens(marker).firstObject() else {
            return;
        };
        let location = NSEvent::mouseLocation();
        update_at(&handle, location.x, screen.frame().size.height - location.y);
    };
    let global = {
        let report = report.clone();
        RcBlock::new(move |_event: NonNull<NSEvent>| report())
    };
    let local = RcBlock::new(move |event: NonNull<NSEvent>| {
        report();
        event.as_ptr()
    });
    unsafe {
        std::mem::forget(NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            mask, &global,
        ));
        std::mem::forget(NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            mask, &local,
        ));
    }
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
        assert!(reachable(WINDOW, 102.0, 150.0, 0.0));
        assert!(reachable(WINDOW, 300.0, 296.0, 0.0));
        assert!(!reachable(WINDOW, 300.0, 200.0, 0.0));
        assert!(reachable(WINDOW, 300.0, 140.0, 44.0));
        assert!(!reachable(WINDOW, 300.0, 160.0, 44.0));
        assert!(reachable(WINDOW, 108.0, 150.0, 0.0));
        assert!(!reachable(WINDOW, 109.0, 150.0, 0.0));
        assert!(!reachable(WINDOW, 99.0, 150.0, 44.0));
    }

    #[test]
    fn stores_the_interactive_height() {
        set_overlay_interactive_height(52);
        assert_eq!(INTERACTIVE_HEIGHT.load(Ordering::Relaxed), 52);
    }
}
