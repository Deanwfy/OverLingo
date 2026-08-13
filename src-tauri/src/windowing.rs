use std::sync::atomic::{AtomicBool, Ordering};
use tauri::image::Image;
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

struct Labels {
    open: &'static str,
    start: &'static str,
    end: &'static str,
    show_overlay: &'static str,
    hide_overlay: &'static str,
    check_update: &'static str,
    quit: &'static str,
}

fn update_label(labels: &Labels, version: &str) -> String {
    format!("{} ({version})", labels.check_update)
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

fn labels(locale: &str) -> Labels {
    match locale {
        "zh-Hans" => Labels {
            open: "设置…",
            start: "开始翻译",
            end: "停止翻译",
            show_overlay: "显示字幕",
            hide_overlay: "隐藏字幕",
            check_update: "检查更新",
            quit: "退出 OverLingo",
        },
        "es" => Labels {
            open: "Ajustes…",
            start: "Iniciar traducción",
            end: "Detener traducción",
            show_overlay: "Mostrar subtítulos",
            hide_overlay: "Ocultar subtítulos",
            check_update: "Buscar actualizaciones",
            quit: "Salir de OverLingo",
        },
        "vi" => Labels {
            open: "Cài đặt…",
            start: "Bắt đầu dịch",
            end: "Dừng dịch",
            show_overlay: "Hiện cửa sổ phụ đề",
            hide_overlay: "Ẩn cửa sổ phụ đề",
            check_update: "Kiểm tra cập nhật",
            quit: "Thoát OverLingo",
        },
        "ja" => Labels {
            open: "設定…",
            start: "翻訳を開始",
            end: "翻訳を終了",
            show_overlay: "字幕ウィンドウを表示",
            hide_overlay: "字幕ウィンドウを隠す",
            check_update: "アップデートを確認",
            quit: "OverLingoを終了",
        },
        "ko" => Labels {
            open: "설정…",
            start: "번역 시작",
            end: "번역 종료",
            show_overlay: "자막 창 표시",
            hide_overlay: "자막 창 숨기기",
            check_update: "업데이트 확인",
            quit: "OverLingo 종료",
        },
        _ => Labels {
            open: "Settings…",
            start: "Start Translation",
            end: "Stop Translation",
            show_overlay: "Show Subtitle Overlay",
            hide_overlay: "Hide Subtitle Overlay",
            check_update: "Check for Updates",
            quit: "Quit OverLingo",
        },
    }
}

pub fn resolve_locale(configured: &str) -> String {
    if configured != "auto" {
        return configured.into();
    }
    let system = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    if system.starts_with("zh") {
        "zh-Hans".into()
    } else if system.starts_with("es") {
        "es".into()
    } else if system.starts_with("ja") {
        "ja".into()
    } else if system.starts_with("ko") {
        "ko".into()
    } else if system.starts_with("vi") {
        "vi".into()
    } else {
        "en".into()
    }
}

/// Menu bar icons are sized in points, so a Retina display needs twice the pixels. macOS
/// scales the image to the bar's height, so the canvas is cropped to the glyph: padding
/// baked into the image would only shrink the glyph next to everyone else's.
const ICON_PX_H: u32 = 36;
const ICON_PX_W: u32 = 45;
/// The glyph is authored on an 18-unit grid regardless of the pixel resolution; the canvas
/// shows this window of it, sized so the glyph keeps a hairline of margin.
const ICON_REGION: RoundRect = RoundRect {
    x: 0.25,
    y: 2.05,
    w: 17.5,
    h: 13.9,
    r: 0.0,
};
const SUBSAMPLES: u32 = 4;

#[derive(Clone, Copy)]
struct RoundRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
}

impl RoundRect {
    const fn new(x: f32, y: f32, w: f32, h: f32, r: f32) -> Self {
        Self { x, y, w, h, r }
    }

    fn grown(&self, by: f32) -> Self {
        Self {
            x: self.x - by,
            y: self.y - by,
            w: self.w + by * 2.0,
            h: self.h + by * 2.0,
            r: self.r + by,
        }
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        if x < self.x || x > self.x + self.w || y < self.y || y > self.y + self.h {
            return false;
        }
        let dx = (self.x + self.r - x)
            .max(x - (self.x + self.w - self.r))
            .max(0.0);
        let dy = (self.y + self.r - y)
            .max(y - (self.y + self.h - self.r))
            .max(0.0);
        dx * dx + dy * dy <= self.r * self.r
    }
}

struct Layer {
    fill: bool,
    shape: RoundRect,
    /// Limits the layer to one region, which is how a ring becomes an arc.
    clip: Option<RoundRect>,
}

impl Layer {
    const fn fill(shape: RoundRect) -> Self {
        Self {
            fill: true,
            shape,
            clip: None,
        }
    }

    const fn erase(shape: RoundRect) -> Self {
        Self {
            fill: false,
            shape,
            clip: None,
        }
    }

    const fn within(self, clip: RoundRect) -> Self {
        Self {
            clip: Some(clip),
            ..self
        }
    }

    fn covers(&self, x: f32, y: f32) -> bool {
        self.clip.is_none_or(|clip| clip.contains(x, y)) && self.shape.contains(x, y)
    }
}

/// A circle, for the arc the running badge is cut from.
const fn disc(cx: f32, cy: f32, radius: f32) -> RoundRect {
    RoundRect {
        x: cx - radius,
        y: cy - radius,
        w: radius * 2.0,
        h: radius * 2.0,
        r: radius,
    }
}

/// Two equally sized speech bubbles (x, y, w, h, radius on the 18-unit grid) with text
/// lines punched out, plus the arcs a running session adds.
fn glyph_layers(running: bool) -> Vec<Layer> {
    let upper = RoundRect::new(1.0, 2.8, 12.0, 7.5, 2.4);
    let lower = RoundRect::new(5.0, 7.7, 12.0, 7.5, 2.4);
    let mut layers = Vec::new();
    if running {
        // Quarter rings sweeping out of the bubbles along the free diagonal. Each origin
        // sits inside a bubble, so only the part clear of the glyph shows, and the pair
        // reads as sound leaving both ends of the stack.
        layers.extend(arc(11.5, 8.5, Corner::TopRight));
        layers.extend(arc(6.5, 9.5, Corner::BottomLeft));
    }
    // Each bubble clears an oversized copy of itself first, so its outline survives macOS
    // flattening the icon to one colour whatever it overlaps.
    layers.extend([
        Layer::erase(upper.grown(0.7)),
        Layer::fill(upper),
        Layer::erase(RoundRect::new(3.6, 4.5, 6.4, 1.1, 0.55)),
        Layer::erase(lower.grown(0.7)),
        Layer::fill(lower),
        Layer::erase(RoundRect::new(7.6, 9.8, 6.4, 1.1, 0.55)),
        Layer::erase(RoundRect::new(9.6, 12.2, 3.4, 1.1, 0.55)),
    ]);
    layers
}

enum Corner {
    TopRight,
    BottomLeft,
}

const ARC_RADIUS: f32 = 5.4;
const ARC_STROKE: f32 = 1.15;

/// One quarter ring. The sweep stops short of the far bubble so the arc ends in open space
/// instead of running into an outline.
fn arc(x: f32, y: f32, corner: Corner) -> [Layer; 2] {
    let sweep = match corner {
        Corner::TopRight => RoundRect::new(x, y - 9.0, 9.0, 8.2, 0.0),
        Corner::BottomLeft => RoundRect::new(x - 9.0, y + 0.8, 9.0, 8.2, 0.0),
    };
    [
        Layer::fill(disc(x, y, ARC_RADIUS)).within(sweep),
        Layer::erase(disc(x, y, ARC_RADIUS - ARC_STROKE)).within(sweep),
    ]
}

fn status_icon(running: bool) -> Image<'static> {
    let layers = glyph_layers(running);
    let scale_x = ICON_REGION.w / ICON_PX_W as f32;
    let scale_y = ICON_REGION.h / ICON_PX_H as f32;
    let step = 1.0 / SUBSAMPLES as f32;
    let mut rgba = vec![0u8; (ICON_PX_W * ICON_PX_H * 4) as usize];

    for y in 0..ICON_PX_H {
        for x in 0..ICON_PX_W {
            let mut hits = 0u32;
            for sy in 0..SUBSAMPLES {
                for sx in 0..SUBSAMPLES {
                    let px = ICON_REGION.x + (x as f32 + (sx as f32 + 0.5) * step) * scale_x;
                    let py = ICON_REGION.y + (y as f32 + (sy as f32 + 0.5) * step) * scale_y;
                    let covered = layers.iter().fold(false, |on, layer| {
                        if layer.covers(px, py) {
                            layer.fill
                        } else {
                            on
                        }
                    });
                    if covered {
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                let alpha = (hits * 255 / (SUBSAMPLES * SUBSAMPLES)) as u8;
                let index = ((y * ICON_PX_W + x) * 4) as usize;
                rgba[index..index + 4].copy_from_slice(&[0, 0, 0, alpha]);
            }
        }
    }

    Image::new_owned(rgba, ICON_PX_W, ICON_PX_H)
}

fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localizes_tray_labels() {
        assert_eq!(labels("zh-Hans").open, "设置…");
        assert_eq!(labels("en").start, "Start Translation");
        assert_eq!(labels("es").start, "Iniciar traducción");
        assert_eq!(labels("vi").quit, "Thoát OverLingo");
        assert_eq!(labels("ja").open, "設定…");
        assert_eq!(labels("ko").show_overlay, "자막 창 표시");
        assert_ne!(resolve_locale("auto"), "auto");
    }

    #[test]
    fn appends_version_to_update_label() {
        assert_eq!(
            update_label(&labels("zh-Hans"), "0.1.0"),
            "检查更新 (0.1.0)"
        );
        assert_eq!(
            update_label(&labels("en"), "1.2.3"),
            "Check for Updates (1.2.3)"
        );
    }

    #[test]
    fn builds_template_icon_pixels() {
        for running in [false, true] {
            let icon = status_icon(running);
            assert_eq!(icon.width(), ICON_PX_W);
            assert_eq!(icon.height(), ICON_PX_H);
            let alpha: Vec<u8> = icon.rgba().iter().skip(3).step_by(4).copied().collect();
            assert!(alpha.contains(&255));
            assert!(alpha.iter().any(|&value| value > 0 && value < 255));
        }
    }

    /// The two states have to be told apart at menu-bar size, not just differ in code.
    #[test]
    fn the_running_icon_is_visibly_different() {
        let idle = status_icon(false);
        let running = status_icon(true);
        let changed = idle
            .rgba()
            .iter()
            .zip(running.rgba().iter())
            .filter(|(left, right)| left != right)
            .count();
        // Guards against the badge vanishing, not against it being small: the arc is a
        // thin stroke that still reads at menu-bar scale.
        assert!(changed > 24, "changed={changed}");
    }
}
