use crate::persistence::write_atomic;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const MAX_TITLE_CHARS: usize = 120;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Segment {
    pub ts: String,
    pub src: String,
    pub tgt: String,
    pub route_id: String,
    pub engine: String,
    pub source_lang: String,
    pub target_lang: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Chunk {
    pub started_at: String,
    pub ended_at: Option<String>,
    pub segments: Vec<Segment>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionRoute {
    pub id: String,
    pub input: String,
    pub engine: String,
    pub model: String,
    pub source_lang: String,
    pub target_lang: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionData {
    pub schema_version: u32,
    pub id: String,
    pub created_at: String,
    pub ended_at: Option<String>,
    pub title: String,
    pub engine: String,
    pub source_lang: String,
    pub target_lang: String,
    pub duration_sec: u64,
    pub routes: Vec<SessionRoute>,
    pub chunks: Vec<Chunk>,
}

#[derive(Serialize)]
pub struct SessionListItem {
    id: String,
    title: String,
    created_at: String,
    duration_sec: u64,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ExportMode {
    Source,
    Target,
    Both,
}

/// Route names come from the interface catalog so exports follow the selected language.
#[derive(Deserialize, Default, Debug)]
pub struct RouteLabels {
    system: String,
    microphone: String,
}

#[derive(Serialize)]
pub struct SessionReadResult {
    md: String,
    json: SessionData,
}

pub fn persist_session(
    app: &AppHandle,
    id: String,
    md_content: String,
    json_data: SessionData,
) -> Result<(), String> {
    validate_id(&id)?;
    if json_data.id != id {
        return Err("Session ID mismatch".into());
    }
    let (md_path, json_path) = session_paths(&sessions_dir(app)?, &id);
    let json = serde_json::to_vec_pretty(&json_data).map_err(error_text)?;
    write_atomic(&json_path, &json)?;
    write_atomic(&md_path, md_content.as_bytes())
}

#[tauri::command]
pub fn list_sessions(app: AppHandle) -> Result<Vec<SessionListItem>, String> {
    let mut sessions = fs::read_dir(sessions_dir(&app)?)
        .map_err(error_text)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("session-") || !name.ends_with(".json") {
                return None;
            }
            let data = fs::read_to_string(entry.path()).ok()?;
            let data: SessionData = serde_json::from_str(&data).ok()?;
            Some(SessionListItem {
                id: data.id,
                title: data.title,
                created_at: data.created_at,
                duration_sec: data.duration_sec,
            })
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sessions)
}

#[tauri::command]
pub fn read_session(app: AppHandle, id: String) -> Result<SessionReadResult, String> {
    validate_id(&id)?;
    let (md_path, json_path) = session_paths(&sessions_dir(&app)?, &id);
    let md = fs::read_to_string(md_path).map_err(error_text)?;
    let json = read_session_data(&json_path)?;
    Ok(SessionReadResult { md, json })
}

#[tauri::command]
pub fn rename_session(app: AppHandle, id: String, title: String) -> Result<(), String> {
    validate_id(&id)?;
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err("Invalid session title".into());
    }
    let (md_path, json_path) = session_paths(&sessions_dir(&app)?, &id);
    let mut data = read_session_data(&json_path)?;
    data.title = title.to_owned();
    let json = serde_json::to_vec_pretty(&data).map_err(error_text)?;
    write_atomic(&json_path, &json)?;
    write_atomic(
        &md_path,
        render_markdown(&data, ExportMode::Both).as_bytes(),
    )
}

#[tauri::command]
pub async fn export_session(
    app: AppHandle,
    id: String,
    mode: ExportMode,
    labels: RouteLabels,
) -> Result<bool, String> {
    validate_id(&id)?;
    let (_, json_path) = session_paths(&sessions_dir(&app)?, &id);
    let data = read_session_data(&json_path)?;
    let Some(target) = ask_export_path(&app, &export_file_name(&data.title)).await else {
        return Ok(false);
    };
    fs::write(target, render_transcript(&data, mode, &labels)).map_err(error_text)?;
    Ok(true)
}

/// Plain reading transcript: one time-stamped block per turn, no markup.
pub fn render_transcript(data: &SessionData, mode: ExportMode, labels: &RouteLabels) -> String {
    let mut lines = vec![
        data.title.clone(),
        format!(
            "{} · {}",
            local_timestamp(&data.created_at),
            elapsed_label(data.duration_sec)
        ),
        String::new(),
    ];
    for segment in data.chunks.iter().flat_map(|chunk| &chunk.segments) {
        let (source, target) = sides(segment, mode);
        if source.is_none() && target.is_none() {
            continue;
        }
        lines.push(format!(
            "[{}] {}",
            segment.ts,
            labels.name(&segment.route_id)
        ));
        lines.extend(source.into_iter().chain(target).map(str::to_owned));
        lines.push(String::new());
    }
    lines.join("\n")
}

pub fn render_markdown(data: &SessionData, mode: ExportMode) -> String {
    let mut lines = vec![
        format!("# {}", data.title),
        String::new(),
        data.created_at.clone(),
        String::new(),
    ];
    for segment in data.chunks.iter().flat_map(|chunk| &chunk.segments) {
        let (source, target) = sides(segment, mode);
        if source.is_none() && target.is_none() {
            continue;
        }
        lines.push(format!("## {} · {}", segment.ts, segment.route_id));
        lines.push(String::new());
        if let Some(text) = source {
            lines.push(format!("**{}**  {}", segment.source_lang, text));
        }
        if let Some(text) = target {
            lines.push(format!("**{}**  {}", segment.target_lang, text));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

#[tauri::command]
pub fn delete_session(app: AppHandle, id: String) -> Result<(), String> {
    validate_id(&id)?;
    let (md_path, json_path) = session_paths(&sessions_dir(&app)?, &id);
    remove_if_present(md_path)?;
    remove_if_present(json_path)
}

impl RouteLabels {
    fn name<'a>(&'a self, route_id: &'a str) -> &'a str {
        let label = match route_id {
            "microphone" => self.microphone.as_str(),
            _ => self.system.as_str(),
        };
        if label.is_empty() {
            route_id
        } else {
            label
        }
    }
}

fn sides(segment: &Segment, mode: ExportMode) -> (Option<&str>, Option<&str>) {
    (
        (mode != ExportMode::Target)
            .then_some(segment.src.as_str())
            .filter(|value| !value.is_empty()),
        (mode != ExportMode::Source)
            .then_some(segment.tgt.as_str())
            .filter(|value| !value.is_empty()),
    )
}

pub fn elapsed_label(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn local_timestamp(created_at: &str) -> String {
    DateTime::parse_from_rfc3339(created_at).map_or_else(
        |_| created_at.to_owned(),
        |time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

async fn ask_export_path(app: &AppHandle, file_name: &str) -> Option<PathBuf> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(file_name)
        .add_filter("Text", &["txt"])
        .save_file(move |path| {
            let _ = sender.send(path);
        });
    receiver.await.ok()?.and_then(|path| path.into_path().ok())
}

fn export_file_name(title: &str) -> String {
    let stem = title
        .chars()
        .map(|character| {
            if character.is_control() || r#"/\:*?"<>|"#.contains(character) {
                '-'
            } else {
                character
            }
        })
        .collect::<String>();
    let stem = stem.trim().trim_matches('.');
    if stem.is_empty() {
        "session.txt".into()
    } else {
        format!("{stem}.txt")
    }
}

fn read_session_data(path: &Path) -> Result<SessionData, String> {
    let data = fs::read_to_string(path).map_err(error_text)?;
    serde_json::from_str(&data).map_err(error_text)
}

fn sessions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(error_text)?
        .join("sessions");
    fs::create_dir_all(&path).map_err(error_text)?;
    Ok(path)
}

fn session_paths(dir: &Path, id: &str) -> (PathBuf, PathBuf) {
    let base = format!("session-{id}");
    (
        dir.join(format!("{base}.md")),
        dir.join(format!("{base}.json")),
    )
}

fn validate_id(id: &str) -> Result<(), String> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character));
    valid
        .then_some(())
        .ok_or_else(|| "Invalid session ID".into())
}

fn remove_if_present(path: PathBuf) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SessionData {
        SessionData {
            schema_version: 1,
            id: "20240101-101010-000".into(),
            created_at: "2024-01-01T10:10:10.000Z".into(),
            ended_at: None,
            title: "2024-01-01 10:10".into(),
            engine: "qwen".into(),
            source_lang: "en".into(),
            target_lang: "zh".into(),
            duration_sec: 12,
            routes: Vec::new(),
            chunks: vec![Chunk {
                started_at: "2024-01-01T10:10:10.000Z".into(),
                ended_at: None,
                segments: vec![Segment {
                    ts: "00:00:03".into(),
                    src: "Hello".into(),
                    tgt: "你好".into(),
                    route_id: "system".into(),
                    engine: "qwen".into(),
                    source_lang: "en".into(),
                    target_lang: "zh".into(),
                }],
            }],
        }
    }

    fn labels() -> RouteLabels {
        RouteLabels {
            system: "系统音频".into(),
            microphone: "麦克风".into(),
        }
    }

    #[test]
    fn export_modes_keep_only_the_requested_side() {
        let data = sample();

        let source = render_transcript(&data, ExportMode::Source, &labels());
        assert!(source.contains("Hello"));
        assert!(!source.contains("你好"));

        let target = render_transcript(&data, ExportMode::Target, &labels());
        assert!(target.contains("你好"));
        assert!(!target.contains("Hello"));

        let both = render_transcript(&data, ExportMode::Both, &labels());
        assert!(both.contains("Hello") && both.contains("你好"));
    }

    #[test]
    fn transcript_reads_as_a_plain_timeline() {
        let transcript = render_transcript(&sample(), ExportMode::Both, &labels());
        let lines = transcript.lines().collect::<Vec<_>>();

        assert_eq!(lines[0], "2024-01-01 10:10");
        assert!(lines[1].ends_with("· 00:00:12"));
        assert_eq!(lines[2], "");
        assert_eq!(lines[3], "[00:00:03] 系统音频");
        assert_eq!(lines[4], "Hello");
        assert_eq!(lines[5], "你好");
    }

    #[test]
    fn route_labels_fall_back_to_the_stored_identifier() {
        assert_eq!(RouteLabels::default().name("microphone"), "microphone");
        assert_eq!(labels().name("microphone"), "麦克风");
    }

    #[test]
    fn archived_markdown_keeps_both_languages() {
        let markdown = render_markdown(&sample(), ExportMode::Both);
        assert!(markdown.contains("**en**  Hello") && markdown.contains("**zh**  你好"));
    }

    #[test]
    fn export_file_name_drops_path_separators() {
        assert_eq!(export_file_name("2024-01-01 10:10"), "2024-01-01 10-10.txt");
        assert_eq!(export_file_name("a/b\\c"), "a-b-c.txt");
        assert_eq!(export_file_name("  "), "session.txt");
    }
}
