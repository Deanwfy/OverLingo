pub struct Labels {
    pub open: &'static str,
    pub start: &'static str,
    pub end: &'static str,
    pub show_overlay: &'static str,
    pub hide_overlay: &'static str,
    pub check_update: &'static str,
    pub quit: &'static str,
}

pub fn update_label(labels: &Labels, version: &str) -> String {
    format!("{} ({version})", labels.check_update)
}

pub fn labels(locale: &str) -> Labels {
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
}
