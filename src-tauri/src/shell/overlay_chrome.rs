use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// The toolbar and the settings panel are separate windows attached to the overlay, so
/// they stay above it and follow it. Keeping them out of the subtitle window lets the
/// box shrink without squeezing them; keeping them apart lets the panel appear without
/// resizing the toolbar's window, which the webview cannot repaint in step with.
pub const TOOLBAR_LABEL: &str = "overlay-chrome";
pub const PANEL_LABEL: &str = "overlay-panel";
pub const LABELS: [&str; 2] = [TOOLBAR_LABEL, PANEL_LABEL];
pub const OVERLAY_LABEL: &str = "overlay";
const SETTINGS_OPEN_EVENT: &str = "overlay://settings-open";
/// Mirrors the overlay window's minimum size in tauri.conf.json; resizing is done here,
/// not by the window system, so the bounds have to be enforced here too.
const MIN_WIDTH: f64 = 520.0;
const MIN_HEIGHT: f64 = 140.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn inset(&self, amount: f64) -> Rect {
        Rect {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - 2.0 * amount).max(0.0),
            height: (self.height - 2.0 * amount).max(0.0),
        }
    }
}

/// Where the control windows are, when they are on screen.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChromeFrames {
    pub toolbar: Option<Rect>,
    pub panel: Option<Rect>,
}

impl ChromeFrames {
    pub fn map(self, f: impl Fn(Rect) -> Rect) -> ChromeFrames {
        ChromeFrames {
            toolbar: self.toolbar.map(&f),
            panel: self.panel.map(&f),
        }
    }
}

/// Which of the box's edges a handle moves; the opposite ones stay put.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Edge {
    north: bool,
    south: bool,
    east: bool,
    west: bool,
}

impl Edge {
    /// `n`, `sw`, ... as the handles in overlay.css are named.
    fn named(name: &str) -> Edge {
        Edge {
            north: name.contains('n'),
            south: name.contains('s'),
            east: name.contains('e'),
            west: name.contains('w'),
        }
    }
}

/// Logical content sizes reported by the toolbar and panel webviews, in `LABELS` order.
static SIZES: Mutex<[(f64, f64); 2]> = Mutex::new([(0.0, 0.0); 2]);
static PANEL_OPEN: AtomicBool = AtomicBool::new(false);
/// A drag from one of the overlay's handles: the pointer watcher moves the box on every
/// cursor event, so the webview's own idea of screen coordinates never enters into it.
#[derive(Clone, Copy)]
struct Drag {
    /// `None` moves the box, an edge resizes it.
    edge: Option<Edge>,
    origin: Rect,
    cursor: (f64, f64),
}

static DRAG: Mutex<Option<Drag>> = Mutex::new(None);

/// The window's outer bounds in logical screen coordinates.
pub fn bounds(window: &WebviewWindow) -> Option<Rect> {
    let scale = window.scale_factor().ok()?;
    let position = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.outer_size().ok()?.to_logical::<f64>(scale);
    Some(Rect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

/// On macOS a child window is ordered in whenever its parent is, so the windows are
/// only attached once they are meant to be seen (see `attach`); Windows needs the owner
/// relation from the start for z-order and takes no such liberty.
pub fn create(app: &AppHandle, overlay: &WebviewWindow) -> tauri::Result<[WebviewWindow; 2]> {
    let build = |label: &str, url: &str| {
        let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
            .title("OverLingo Controls")
            .inner_size(1.0, 1.0)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .accept_first_mouse(true);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.parent(overlay)?;
        #[cfg(target_os = "macos")]
        let _ = overlay;
        builder.build()
    };
    Ok([
        build(TOOLBAR_LABEL, "chrome.html")?,
        build(PANEL_LABEL, "chrome.html?view=panel")?,
    ])
}

pub fn panel_open() -> bool {
    PANEL_OPEN.load(Ordering::Relaxed)
}

#[tauri::command]
pub fn set_overlay_settings_open(app: AppHandle, open: bool) {
    set_panel_open(&app, open);
    super::overlay_pointer::update(&app);
}

#[tauri::command]
pub fn set_overlay_chrome_size(window: WebviewWindow, width: f64, height: f64) {
    let Some(index) = LABELS.iter().position(|label| *label == window.label()) else {
        return;
    };
    if let Ok(mut sizes) = SIZES.lock() {
        sizes[index] = (width, height);
    }
    place(window.app_handle());
}

/// Both windows learn about it through the same event, so the toolbar's button and the
/// panel agree whoever asked.
pub fn set_panel_open(app: &AppHandle, open: bool) {
    PANEL_OPEN.store(open, Ordering::Relaxed);
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        if open {
            place_all(app, true);
            order_front(&panel);
            attach(app, &panel);
        } else {
            let _ = panel.hide();
            // The pointer watcher must stop counting the panel's area as hovered.
            place(app);
        }
    }
    for label in LABELS {
        let _ = app.emit_to(label, SETTINGS_OPEN_EVENT, open);
    }
}

/// Moves from the box's rim or the toolbar's gaps, resizes from the corners: the webview
/// only says when a press starts and ends, and the cursor is followed from the backend.
/// WebKit reports screen coordinates relative to the window's own screen, so offsets
/// from it would jump the moment the box crossed to another display.
#[tauri::command]
pub fn drag_overlay(app: AppHandle, edge: Option<String>, begin: bool) {
    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let Ok(mut drag) = DRAG.lock() else {
        return;
    };
    if !begin {
        *drag = None;
        drop(drag);
        place(&app);
        return;
    }
    let (Some(origin), Ok(cursor), Ok(scale)) = (
        bounds(&overlay),
        app.cursor_position(),
        overlay.scale_factor(),
    ) else {
        return;
    };
    let cursor = cursor.to_logical::<f64>(scale);
    *drag = Some(Drag {
        edge: edge.as_deref().map(Edge::named),
        origin,
        cursor: (cursor.x, cursor.y),
    });
}

pub fn dragging() -> bool {
    DRAG.lock().is_ok_and(|drag| drag.is_some())
}

/// `x`, `y`: the cursor in logical screen coordinates.
pub fn drag_to(app: &AppHandle, x: f64, y: f64) {
    let Some(drag) = DRAG.lock().ok().and_then(|drag| *drag) else {
        return;
    };
    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let (dx, dy) = (x - drag.cursor.0, y - drag.cursor.1);
    match drag.edge {
        None => {
            let _ =
                overlay.set_position(LogicalPosition::new(drag.origin.x + dx, drag.origin.y + dy));
        }
        Some(edge) => set_frame(&overlay, resized(drag.origin, edge, dx, dy)),
    }
}

fn resized(start: Rect, edge: Edge, dx: f64, dy: f64) -> Rect {
    let mut frame = start;
    if edge.east {
        frame.width = (start.width + dx).max(MIN_WIDTH);
    }
    if edge.west {
        frame.width = (start.width - dx).max(MIN_WIDTH);
        frame.x = start.x + start.width - frame.width;
    }
    if edge.south {
        frame.height = (start.height + dy).max(MIN_HEIGHT);
    }
    if edge.north {
        frame.height = (start.height - dy).max(MIN_HEIGHT);
        frame.y = start.y + start.height - frame.height;
    }
    frame
}

pub fn set_always_on_top(app: &AppHandle, on_top: bool) {
    for label in LABELS {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_always_on_top(on_top);
        }
    }
}

/// The toolbar is shown and hidden with the cursor by the pointer watcher; here both
/// windows only have to leave the screen with the overlay and be back in place when it
/// returns.
pub fn set_visible(app: &AppHandle, visible: bool) {
    if visible {
        place(app);
        return;
    }
    set_panel_open(app, false);
    conceal(app);
}

/// The overlay may have moved while the toolbar was hidden, so it is placed afresh; on
/// macOS ordering a child window out also detaches it from its parent, so it is attached
/// again or it would stop following the overlay.
pub fn reveal(app: &AppHandle) {
    let Some(toolbar) = app.get_webview_window(TOOLBAR_LABEL) else {
        return;
    };
    place(app);
    order_front(&toolbar);
    attach(app, &toolbar);
}

/// Shown without being made key: `show` would hand focus back and forth between the
/// control windows, and a panel that loses focus while opening reads as closed again.
#[cfg(target_os = "macos")]
fn order_front(window: &WebviewWindow) {
    use objc2_app_kit::NSWindow;

    let Ok(raw_window) = window.ns_window() else {
        return;
    };
    let raw_window = raw_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        let native_window = &*(raw_window as *mut NSWindow);
        native_window.orderFront(None);
    });
}

#[cfg(not(target_os = "macos"))]
fn order_front(window: &WebviewWindow) {
    let _ = window.show();
}

pub fn conceal(app: &AppHandle) {
    if let Some(toolbar) = app.get_webview_window(TOOLBAR_LABEL) {
        let _ = toolbar.hide();
    }
}

#[cfg(target_os = "macos")]
fn attach(app: &AppHandle, child: &WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowOrderingMode};

    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let (Ok(parent), Ok(child_window)) = (overlay.ns_window(), child.ns_window()) else {
        return;
    };
    let (parent, child_window) = (parent as usize, child_window as usize);
    let _ = child.run_on_main_thread(move || unsafe {
        let parent = &*(parent as *mut NSWindow);
        let child = &*(child_window as *mut NSWindow);
        if child.parentWindow().is_none() {
            parent.addChildWindow_ordered(child, NSWindowOrderingMode::Above);
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn attach(_app: &AppHandle, _child: &WebviewWindow) {}

/// Hangs the toolbar off the box's top-right corner. The panel is only positioned when
/// it opens (above the toolbar when the screen allows, so it does not cover the
/// subtitles) and then stays where the user saw it appear, like a popover; on macOS it
/// rides along with the overlay as a child window, elsewhere it has to be moved along.
pub fn place(app: &AppHandle) {
    place_all(app, cfg!(not(target_os = "macos")));
}

fn place_all(app: &AppHandle, move_panel: bool) {
    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let Some(subtitle_box) = bounds(&overlay) else {
        return;
    };
    let sizes = SIZES.lock().map(|sizes| *sizes).unwrap_or_default();
    let work_area = overlay.current_monitor().ok().flatten().map(|monitor| {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let position = area.position.to_logical::<f64>(scale);
        let extent = area.size.to_logical::<f64>(scale);
        Rect {
            x: position.x,
            y: position.y,
            width: extent.width,
            height: extent.height,
        }
    });
    let [toolbar_size, panel_size] = sizes;
    let toolbar_frame = toolbar_frame(subtitle_box, toolbar_size);
    let panel_frame = panel_frame(toolbar_frame, panel_size, work_area);
    let mut frames = ChromeFrames::default();
    if let Some(toolbar) = app.get_webview_window(TOOLBAR_LABEL) {
        if toolbar_frame.width > 0.0 && toolbar_frame.height > 0.0 {
            set_frame(&toolbar, toolbar_frame);
            frames.toolbar = Some(toolbar_frame);
        }
    }
    if let Some(panel) = app.get_webview_window(PANEL_LABEL) {
        if move_panel && panel_frame.width > 0.0 && panel_frame.height > 0.0 {
            set_frame(&panel, panel_frame);
            if panel_open() {
                frames.panel = Some(panel_frame);
            }
        } else if panel_open() {
            frames.panel = bounds(&panel);
        }
    }
    super::overlay_pointer::set_chrome_frames(subtitle_box, frames);
}

/// Growing upwards means the top edge moves as the height changes; done as two calls the
/// window would show a frame hanging below the toolbar first, so AppKit gets both at once.
#[cfg(target_os = "macos")]
pub fn set_frame(window: &WebviewWindow, frame: Rect) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Ok(raw_window) = window.ns_window() else {
        return;
    };
    let raw_window = raw_window as usize;
    let _ = window.run_on_main_thread(move || unsafe {
        let marker = MainThreadMarker::new().expect("main thread");
        let Some(screen) = NSScreen::screens(marker).firstObject() else {
            return;
        };
        let screen_height = screen.frame().size.height;
        let native_window = &*(raw_window as *mut NSWindow);
        native_window.setFrame_display(
            NSRect::new(
                NSPoint::new(frame.x, screen_height - frame.y - frame.height),
                NSSize::new(frame.width, frame.height),
            ),
            true,
        );
    });
}

#[cfg(not(target_os = "macos"))]
pub fn set_frame(window: &WebviewWindow, frame: Rect) {
    let _ = window.set_size(tauri::LogicalSize::new(frame.width, frame.height));
    let _ = window.set_position(LogicalPosition::new(frame.x, frame.y));
}

/// Always hangs off the box's top-right corner, even off screen: the box itself can be
/// dragged back by its rim, and moving the toolbar about would only make it hard to find.
fn toolbar_frame(subtitle_box: Rect, (width, height): (f64, f64)) -> Rect {
    Rect {
        x: subtitle_box.x + subtitle_box.width - width,
        y: subtitle_box.y - height,
        width,
        height,
    }
}

/// A dropdown on the toolbar: right-aligned with it, below it like any menu, above it
/// only when the screen ends too soon, never off the left edge.
fn panel_frame(toolbar: Rect, (width, height): (f64, f64), work_area: Option<Rect>) -> Rect {
    let bottom = work_area.map_or(f64::MAX, |area| area.y + area.height);
    let left = work_area.map_or(f64::MIN, |area| area.x);
    let below = toolbar.y + toolbar.height;
    let y = if below + height <= bottom {
        below
    } else {
        toolbar.y - height
    };
    Rect {
        x: (toolbar.x + toolbar.width - width).max(left),
        y,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect {
        x: 100.0,
        y: 600.0,
        width: 800.0,
        height: 200.0,
    };
    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 25.0,
        width: 1440.0,
        height: 875.0,
    };
    const TOOLBAR: (f64, f64) = (240.0, 40.0);
    const PANEL: (f64, f64) = (608.0, 328.0);

    #[test]
    fn hangs_the_toolbar_off_the_top_right_corner() {
        let toolbar = toolbar_frame(BOX, TOOLBAR);
        assert_eq!((toolbar.x, toolbar.y), (660.0, 560.0));
        let high = toolbar_frame(Rect { y: 10.0, ..BOX }, TOOLBAR);
        assert_eq!(high.y, -30.0);
    }

    #[test]
    fn opens_the_panel_below_the_toolbar_when_it_fits() {
        let toolbar = toolbar_frame(Rect { y: 200.0, ..BOX }, TOOLBAR);
        let panel = panel_frame(toolbar, PANEL, Some(SCREEN));
        assert_eq!((panel.x, panel.y), (292.0, 200.0));
    }

    #[test]
    fn opens_the_panel_above_the_toolbar_near_the_bottom() {
        let toolbar = toolbar_frame(Rect { y: 800.0, ..BOX }, TOOLBAR);
        let panel = panel_frame(toolbar, PANEL, Some(SCREEN));
        assert_eq!(panel.y, 760.0 - 328.0);
    }

    #[test]
    fn keeps_the_panel_on_screen_on_the_left() {
        let toolbar = toolbar_frame(
            Rect {
                x: 10.0,
                width: 300.0,
                ..BOX
            },
            TOOLBAR,
        );
        assert_eq!(panel_frame(toolbar, PANEL, Some(SCREEN)).x, 0.0);
    }

    #[test]
    fn resizes_from_any_edge_and_keeps_the_opposite_one() {
        let east = resized(BOX, Edge::named("e"), 50.0, 0.0);
        assert_eq!((east.x, east.width), (100.0, 850.0));
        let west = resized(BOX, Edge::named("w"), 50.0, 0.0);
        assert_eq!((west.x, west.width), (150.0, 750.0));
        let corner = resized(BOX, Edge::named("nw"), -20.0, -30.0);
        assert_eq!((corner.x, corner.y), (80.0, 570.0));
        assert_eq!((corner.width, corner.height), (820.0, 230.0));
        let small = resized(BOX, Edge::named("se"), -900.0, -900.0);
        assert_eq!((small.width, small.height), (MIN_WIDTH, MIN_HEIGHT));
        let pinned = resized(BOX, Edge::named("nw"), 900.0, 900.0);
        assert_eq!(pinned.x + pinned.width, 900.0);
        assert_eq!(pinned.y + pinned.height, 800.0);
    }

    #[test]
    fn insets_a_rect_on_every_side() {
        let inner = BOX.inset(8.0);
        assert_eq!(inner.x, 108.0);
        assert_eq!(inner.width, 784.0);
        assert!(BOX.contains(102.0, 602.0));
        assert!(!inner.contains(102.0, 602.0));
    }
}
