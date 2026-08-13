use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// A click-through window receives no mouse events, so the cursor has to be polled;
/// the interval has to stay short enough that reaching the toolbar feels instant.
const POLL_INTERVAL: Duration = Duration::from_millis(40);
/// CSS :hover is unusable while clicks pass through, so the frontend is told when the
/// cursor enters or leaves the control strip.
const HOVER_EVENT: &str = "overlay://pointer-hover";
const OVERLAY_LABEL: &str = "overlay";

/// Bumped whenever click-through is toggled; a stale poll task sees the change and exits.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static HOVERING: AtomicBool = AtomicBool::new(false);
/// Height of the top strip that stays clickable, in logical pixels; the rest passes clicks on.
static INTERACTIVE_HEIGHT: AtomicU32 = AtomicU32::new(0);

#[tauri::command]
pub fn set_overlay_interactive_height(height: u32) {
    INTERACTIVE_HEIGHT.store(height, Ordering::Relaxed);
}

pub fn apply(app: &AppHandle, click_through: bool) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    if !click_through {
        set_state(&window, true);
        return;
    }
    set_state(&window, pointer_over_controls(&window));
    tauri::async_runtime::spawn(watch(window, generation));
}

async fn watch(window: WebviewWindow, generation: u64) {
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        update(&window);
    }
}

fn update(window: &WebviewWindow) {
    let hovering = pointer_over_controls(window);
    if HOVERING.load(Ordering::Relaxed) != hovering {
        set_state(window, hovering);
    }
}

/// Suspends click-through while the cursor rests on the controls, so the toolbar and the
/// settings panel can still be hovered and clicked.
fn set_state(window: &WebviewWindow, hovering: bool) {
    HOVERING.store(hovering, Ordering::Relaxed);
    let _ = window.set_ignore_cursor_events(!hovering);
    let _ = window.emit(HOVER_EVENT, hovering);
}

// The controller stops polling with apply(false) when the overlay hides, so visibility
// needs no extra check here.
fn pointer_over_controls(window: &WebviewWindow) -> bool {
    let (Ok(cursor), Ok(origin), Ok(size), Ok(scale)) = (
        window.cursor_position(),
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) else {
        return false;
    };
    let height = f64::from(INTERACTIVE_HEIGHT.load(Ordering::Relaxed)) * scale;
    contains(
        cursor.x - f64::from(origin.x),
        cursor.y - f64::from(origin.y),
        f64::from(size.width),
        height.min(f64::from(size.height)),
    )
}

fn contains(x: f64, y: f64, width: f64, height: f64) -> bool {
    x >= 0.0 && x < width && y >= 0.0 && y < height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_control_strip_interactive() {
        assert!(contains(10.0, 5.0, 400.0, 44.0));
        assert!(!contains(10.0, 60.0, 400.0, 44.0));
        assert!(!contains(-1.0, 5.0, 400.0, 44.0));
        assert!(!contains(400.0, 5.0, 400.0, 44.0));
    }

    #[test]
    fn stores_the_interactive_height() {
        set_overlay_interactive_height(52);
        assert_eq!(INTERACTIVE_HEIGHT.load(Ordering::Relaxed), 52);
    }
}
