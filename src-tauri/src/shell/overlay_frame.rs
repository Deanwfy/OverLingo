//! Keeps the subtitle box where the user left it: across launches, and across the window
//! system moving it, which it does when a display drops out and, for a window on all
//! Spaces, never undoes when the display returns.

use super::overlay_chrome::{
    bounds, screen_scale, set_frame, work_area, MIN_HEIGHT, MIN_WIDTH, OVERLAY_LABEL,
};
use crate::controller::AppController;
use crate::geometry::Rect;
use tauri::{AppHandle, Manager, WebviewWindow};

/// How much of the box must lie on a screen for its rim to be reachable.
const GRIP: f64 = 48.0;
/// The first launch's box, in CSS pixels and a share of the screen: half its width, about
/// three turns tall, sitting low like subtitles do.
const DEFAULT_HEIGHT: f64 = 220.0;
const WIDTH_SHARE: f64 = 0.5;
const BOTTOM_SHARE: f64 = 0.08;

pub fn report(app: &AppHandle, frame: Rect) {
    app.state::<AppController>().overlay_frame_changed(frame);
}

/// Places the box at launch and returns where; `None` when no screen is known, which
/// leaves the window as tauri.conf.json made it.
pub fn restore(app: &AppHandle, saved: Option<Rect>) -> Option<Rect> {
    let overlay = app.get_webview_window(OVERLAY_LABEL)?;
    let scale = screen_scale(&overlay);
    let frame = lay_out(
        saved,
        &screens(app),
        primary(app),
        (MIN_WIDTH * scale, MIN_HEIGHT * scale),
        DEFAULT_HEIGHT * scale,
    )?;
    apply(&overlay, frame);
    Some(frame)
}

/// Moves the box back to `home` once that is on a screen again.
pub fn reconcile(app: &AppHandle, home: Option<Rect>) {
    let Some(home) = home.filter(|home| reachable(*home, &screens(app))) else {
        return;
    };
    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    if bounds(&overlay).is_some_and(|current| !current.near(&home)) {
        set_frame(&overlay, home);
    }
}

/// Displays come and go in bursts of notifications; only the last one, once things have
/// settled, is acted on.
#[cfg(target_os = "macos")]
pub fn watch_screens(app: &AppHandle) {
    use block2::RcBlock;
    use objc2_app_kit::NSApplicationDidChangeScreenParametersNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    let app = app.clone();
    let generation = Arc::new(AtomicU64::new(0));
    let handler = RcBlock::new(move |_: NonNull<NSNotification>| {
        let app = app.clone();
        let generation = generation.clone();
        let this = generation.fetch_add(1, Ordering::SeqCst) + 1;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if generation.load(Ordering::SeqCst) != this {
                return;
            }
            let home = app.state::<AppController>().overlay_frame();
            reconcile(&app, home);
        });
    });
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidChangeScreenParametersNotification),
            None,
            None::<&NSOperationQueue>,
            &handler,
        )
    };
    std::mem::forget(observer);
}

fn screens(app: &AppHandle) -> Vec<Rect> {
    app.available_monitors()
        .map(|monitors| monitors.iter().map(work_area).collect())
        .unwrap_or_default()
}

fn primary(app: &AppHandle) -> Option<Rect> {
    app.primary_monitor().ok().flatten().as_ref().map(work_area)
}

/// The saved spot if it is still on a screen. Otherwise the saved size, or on a first
/// launch half the screen's width by `default_height`, laid out on the primary screen the
/// way subtitles sit: centred, near the bottom.
fn lay_out(
    saved: Option<Rect>,
    screens: &[Rect],
    primary: Option<Rect>,
    (min_width, min_height): (f64, f64),
    default_height: f64,
) -> Option<Rect> {
    let saved = saved.map(|frame| Rect {
        width: frame.width.max(min_width),
        height: frame.height.max(min_height),
        ..frame
    });
    if let Some(frame) = saved.filter(|frame| reachable(*frame, screens)) {
        return Some(frame);
    }
    let screen = primary.or_else(|| screens.first().copied())?;
    let (width, height) = saved.map_or(
        ((screen.width * WIDTH_SHARE).max(min_width), default_height),
        |frame| (frame.width, frame.height),
    );
    let width = width.min(screen.width);
    let height = height.min(screen.height);
    Some(Rect {
        x: screen.x + (screen.width - width) / 2.0,
        y: (screen.y + screen.height * (1.0 - BOTTOM_SHARE) - height).max(screen.y),
        width,
        height,
    })
}

fn reachable(frame: Rect, screens: &[Rect]) -> bool {
    screens.iter().any(|screen| frame.overlaps_by(screen, GRIP))
}

/// Before the event loop runs, a frame set beneath tao is only partly kept, so the
/// window API is used instead of `set_frame`; the window is hidden, so nothing jumps.
#[cfg(target_os = "macos")]
fn apply(overlay: &WebviewWindow, frame: Rect) {
    let _ = overlay.set_size(tauri::LogicalSize::new(frame.width, frame.height));
    let _ = overlay.set_position(tauri::LogicalPosition::new(frame.x, frame.y));
}

#[cfg(not(target_os = "macos"))]
fn apply(overlay: &WebviewWindow, frame: Rect) {
    set_frame(overlay, frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };
    const MIN: (f64, f64) = (520.0, 140.0);

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn laid_out(saved: Option<Rect>, screens: &[Rect]) -> Option<Rect> {
        lay_out(
            saved,
            screens,
            screens.first().copied(),
            MIN,
            DEFAULT_HEIGHT,
        )
    }

    #[test]
    fn keeps_a_frame_on_screen() {
        let frame = rect(100.0, 600.0, 1000.0, 360.0);
        assert_eq!(laid_out(Some(frame), &[SCREEN]), Some(frame));
    }

    #[test]
    fn keeps_a_frame_hanging_off_the_edge_by_its_rim() {
        let frame = rect(1860.0, 200.0, 1000.0, 360.0);
        assert_eq!(laid_out(Some(frame), &[SCREEN]), Some(frame));
        let frame = rect(1900.0, 200.0, 1000.0, 360.0);
        assert_ne!(laid_out(Some(frame), &[SCREEN]), Some(frame));
    }

    #[test]
    fn accepts_a_frame_on_a_second_screen() {
        let second = rect(1920.0, 0.0, 2560.0, 1440.0);
        let frame = rect(2500.0, 900.0, 1000.0, 360.0);
        assert_eq!(laid_out(Some(frame), &[SCREEN, second]), Some(frame));
    }

    #[test]
    fn grows_a_frame_below_the_minimum() {
        let frame = laid_out(Some(rect(100.0, 100.0, 300.0, 50.0)), &[SCREEN]).unwrap();
        assert_eq!((frame.width, frame.height), MIN);
    }

    #[test]
    fn lays_a_frame_off_every_screen_out_again_at_its_size() {
        // Centred, its bottom edge 8 % of the screen height up from the bottom.
        assert_eq!(
            laid_out(Some(rect(3000.0, 200.0, 1000.0, 360.0)), &[SCREEN]),
            Some(rect(460.0, 633.6, 1000.0, 360.0))
        );
    }

    #[test]
    fn shrinks_a_frame_wider_than_the_screen_it_lands_on() {
        assert_eq!(
            laid_out(Some(rect(5000.0, 5000.0, 2500.0, 360.0)), &[SCREEN]),
            Some(rect(0.0, 633.6, 1920.0, 360.0))
        );
    }

    #[test]
    fn first_launch_takes_half_the_screen_width() {
        assert_eq!(
            laid_out(None, &[SCREEN]),
            Some(rect(480.0, 773.6, 960.0, 220.0))
        );
    }

    #[test]
    fn gives_up_without_screens() {
        assert_eq!(laid_out(None, &[]), None);
        assert_eq!(laid_out(Some(rect(0.0, 0.0, 1000.0, 360.0)), &[]), None);
    }
}
