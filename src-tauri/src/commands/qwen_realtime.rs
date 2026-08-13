use super::realtime::{Connection, Event, Events, FragmentKind, ProviderState};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use http::Request;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const CURRENT_MODEL: &str = "qwen3.5-livetranslate-flash-realtime";

pub struct QwenRealtimeConfig {
    pub api_key: String,
    pub source_language: String,
    pub target_language: String,
    pub region: String,
    pub workspace_id: String,
    pub model: String,
    pub route_id: String,
}

struct Session {
    audio_tx: mpsc::UnboundedSender<Vec<u8>>,
    stop_tx: mpsc::UnboundedSender<()>,
}

impl Connection for Session {
    fn send_audio(&self, pcm: Vec<u8>) -> Result<(), String> {
        self.audio_tx
            .send(pcm)
            .map_err(|error| format!("send audio failed: {error}"))
    }

    fn stop(&self) {
        let _ = self.stop_tx.send(());
    }
}

pub fn start_session(
    config: QwenRealtimeConfig,
    events: Events,
    state: &ProviderState,
) -> Result<u64, String> {
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (stop_tx, stop_rx) = mpsc::unbounded_channel::<()>();

    let id = state.start(Session { audio_tx, stop_tx }, move |id| async move {
        diagnostic_log(
            id,
            format!(
                "start route={} model={} region={} source={} target={} workspace_configured={}",
                diagnostic_field(&config.route_id),
                diagnostic_field(&config.model),
                diagnostic_field(&config.region),
                diagnostic_field(&config.source_language),
                diagnostic_field(&config.target_language),
                !config.workspace_id.trim().is_empty(),
            ),
        );
        if let Err(error) = run_session(id, config, audio_rx, stop_rx, events.clone()).await {
            diagnostic_log(id, format!("failed error={error}"));
            events.emit(Event::error(error));
        }
        events.emit(Event::Closed("session_ended".into()));
        diagnostic_log(id, "ended");
    });
    Ok(id)
}

async fn run_session(
    session_id: u64,
    cfg: QwenRealtimeConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut stop_rx: mpsc::UnboundedReceiver<()>,
    events: Events,
) -> Result<(), String> {
    let (qwen_realtime_url, qwen_host) = build_qwen_endpoint(&cfg)?;
    diagnostic_log(session_id, "endpoint_ready");
    let request = Request::builder()
        .uri(qwen_realtime_url.as_str())
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Host", qwen_host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .map_err(|e| sanitize_error(&cfg, format!("build request: {e}")))?;

    diagnostic_log(session_id, "websocket_handshake_start");
    let connect = tokio::time::timeout(Duration::from_secs(14), connect_async(request));
    tokio::pin!(connect);
    let (ws_stream, response) = tokio::select! {
        result = &mut connect => {
            result
                .map_err(|_| "websocket handshake timed out".to_string())?
                .map_err(|e| sanitize_error(&cfg, format!("websocket connect: {e}")))?
        }
        _ = stop_rx.recv() => {
            diagnostic_log(session_id, "websocket_handshake_cancelled");
            return Ok(());
        }
    };
    diagnostic_log(
        session_id,
        format!("websocket_handshake_ok status={}", response.status()),
    );

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let session_update = build_session_update(&cfg);
    diagnostic_log(session_id, "session_update_send");
    ws_sink
        .send(Message::Text(session_update.into()))
        .await
        .map_err(|e| sanitize_error(&cfg, format!("send session.update: {e}")))?;
    diagnostic_log(session_id, "session_update_sent");

    let mut last_done_response_id: Option<String> = None;
    let mut ready = false;

    loop {
        tokio::select! {
            biased;

            _ = stop_rx.recv() => {
                diagnostic_log(session_id, "stop_requested");
                let finish = serde_json::json!({
                    "event_id": format!("event_finish_{}", rand::random::<u64>()),
                    "type": "session.finish",
                });
                if ws_sink
                    .send(Message::Text(finish.to_string().into()))
                    .await
                    .is_ok()
                {
                    let deadline =
                        tokio::time::Instant::now() + std::time::Duration::from_secs(3);
                    loop {
                        let now = tokio::time::Instant::now();
                        if now >= deadline {
                            break;
                        }
                        let remaining = deadline.saturating_duration_since(now);
                        match tokio::time::timeout(remaining, ws_stream.next()).await {
                            Ok(Some(Ok(Message::Text(text)))) => {
                                if handle_server_event(
                                    &text,
                                    &events,
                                    &mut last_done_response_id,
                                ) {
                                    break;
                                }
                            }
                            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) => break,
                            Ok(Some(Ok(_))) => {}
                            Ok(Some(Err(_))) => break,
                        }
                    }
                }
                let _ = ws_sink.send(Message::Close(None)).await;
                break;
            }

            Some(pcm) = audio_rx.recv() => {
                let b64 = B64.encode(&pcm);
                let evt = serde_json::json!({
                    "event_id": format!("event_audio_{}", rand::random::<u64>()),
                    "type": "input_audio_buffer.append",
                    "audio": b64,
                });
                if let Err(e) = ws_sink.send(Message::Text(evt.to_string().into())).await {
                    return Err(format!("send audio: {}", e));
                }
            }

            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if !ready {
                            diagnostic_log(
                                session_id,
                                format!(
                                    "server_event type={}",
                                    diagnostic_field(
                                        server_event_type(&text).as_deref().unwrap_or("invalid_json")
                                    )
                                ),
                            );
                        }
                        if let Some(error) = server_error(&text) {
                            return Err(sanitize_error(&cfg, error));
                        }
                        if !ready && server_event_type(&text).as_deref() == Some("session.updated") {
                            ready = true;
                            diagnostic_log(session_id, "ready");
                            events.emit(Event::Ready);
                        }
                        handle_server_event(&text, &events, &mut last_done_response_id);
                    }
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Close(frame))) => {
                        let reason = frame
                            .map(|f| format!("{}: {}", f.code, f.reason))
                            .unwrap_or_else(|| "connection_closed".into());
                        diagnostic_log(
                            session_id,
                            format!("connection_closed reason={}", diagnostic_field(&reason)),
                        );
                        events.emit(Event::Closed(reason));
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        return Err(sanitize_error(&cfg, format!("ws error: {e}")));
                    }
                    None => {
                        diagnostic_log(session_id, "stream_ended");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn diagnostic_log(session_id: u64, message: impl AsRef<str>) {
    crate::diagnostics::log(&format!("qwen:{session_id}"), message);
}

fn diagnostic_field(value: &str) -> String {
    crate::diagnostics::field(value).chars().take(80).collect()
}

fn sanitize_error(cfg: &QwenRealtimeConfig, message: impl Into<String>) -> String {
    let mut sanitized = message.into();
    for secret in [&cfg.api_key, &cfg.workspace_id] {
        if !secret.is_empty() {
            sanitized = sanitized.replace(secret, "<redacted>");
        }
    }
    sanitized
}

fn build_qwen_endpoint(cfg: &QwenRealtimeConfig) -> Result<(String, String), String> {
    let model = cfg.model.trim();
    if model != CURRENT_MODEL {
        return Err(format!("Unsupported Qwen LiveTranslate model: {}", model));
    }

    let workspace_id = cfg.workspace_id.trim();
    if workspace_id.is_empty() {
        return Err("Qwen3.5 requires a Model Studio Workspace ID".into());
    }
    if !workspace_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Workspace ID contains unsupported characters".into());
    }

    let Some(region) = crate::translators::qwen_region(&cfg.region) else {
        return Err(format!("Unsupported Qwen region: {}", cfg.region));
    };
    let host = format!("{workspace_id}.{region}.maas.aliyuncs.com");

    let url = format!("wss://{}/api-ws/v1/realtime?model={}", host, model);
    Ok((url, host))
}

fn build_session_update(cfg: &QwenRealtimeConfig) -> String {
    let source = if cfg.source_language.is_empty() || cfg.source_language == "auto" {
        "en"
    } else {
        cfg.source_language.as_str()
    };

    let input_audio_transcription = serde_json::json!({
        "language": source,
        "model": "qwen3-asr-flash-realtime",
    });

    let session = serde_json::json!({
        "modalities": ["text"],
        "sample_rate": 16000,
        "input_audio_format": "pcm",
        "input_audio_transcription": input_audio_transcription,
        "translation": { "language": cfg.target_language },
        "turn_detection": {
            "type": "server_vad",
            "threshold": 0.2,
            "silence_duration_ms": 800,
        },
    });

    serde_json::json!({
        "event_id": format!("event_session_{}", rand::random::<u64>()),
        "type": "session.update",
        "session": session,
    })
    .to_string()
}

fn handle_server_event(
    text: &str,
    events: &Events,
    last_done_response_id: &mut Option<String>,
) -> bool {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let evt_type = match value.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return false,
    };

    match evt_type {
        "session.created" | "session.updated" | "response.created" | "response.done" => {}
        "session.finished" => return true,

        "response.text.text" => {
            let text = partial_text(&value);
            if !text.is_empty() {
                events.emit(Event::fragment(FragmentKind::Translation, text, false));
            }
        }

        "response.text.done" => {
            let response_id =
                string_field(&value, "response_id").or_else(|| string_field(&value, "item_id"));
            // The server can repeat a done event; emitting twice would duplicate the turn.
            if response_id.is_some() && response_id == *last_done_response_id {
                return false;
            }
            *last_done_response_id = response_id;

            if let Some(text) = string_field(&value, "text") {
                events.emit(Event::fragment(FragmentKind::Translation, text, true));
            }
        }

        "conversation.item.input_audio_transcription.text" => {
            let text = partial_text(&value);
            if !text.is_empty() {
                events.emit(Event::fragment(FragmentKind::Original, text, false));
            }
        }

        "conversation.item.input_audio_transcription.completed" => {
            if let Some(text) =
                string_field(&value, "transcript").or_else(|| string_field(&value, "text"))
            {
                events.emit(Event::fragment(FragmentKind::Original, text, true));
            }
        }

        _ => {}
    }
    false
}

/// Qwen streams a committed prefix plus an unstable tail; the pair is the current snapshot.
fn partial_text(value: &serde_json::Value) -> String {
    format!(
        "{}{}",
        value.get("text").and_then(|v| v.as_str()).unwrap_or(""),
        value.get("stash").and_then(|v| v.as_str()).unwrap_or(""),
    )
}

fn server_event_type(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()?
        .get("type")?
        .as_str()
        .map(str::to_owned)
}

fn server_error(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let event_type = value.get("type")?.as_str()?;
    if !matches!(
        event_type,
        "error" | "conversation.item.input_audio_transcription.failed"
    ) {
        return None;
    }
    let error = value.get("error")?;
    let code = error.get("code").and_then(|value| value.as_str());
    let message = error.get("message").and_then(|value| value.as_str());
    let parameter = error.get("param").and_then(|value| value.as_str());
    let mut details = Vec::new();
    if let Some(code) = code.filter(|value| !value.is_empty()) {
        details.push(code.to_string());
    }
    if let Some(message) = message.filter(|value| !value.is_empty()) {
        details.push(message.to_string());
    }
    if let Some(parameter) = parameter.filter(|value| !value.is_empty()) {
        details.push(format!("parameter: {parameter}"));
    }
    Some(if details.is_empty() {
        "Qwen returned an unknown error".into()
    } else {
        details.join(": ")
    })
}

fn string_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(region: &str, workspace_id: &str, model: &str) -> QwenRealtimeConfig {
        QwenRealtimeConfig {
            api_key: "test-key".into(),
            source_language: "zh".into(),
            target_language: "en".into(),
            region: region.into(),
            workspace_id: workspace_id.into(),
            model: model.into(),
            route_id: "system".into(),
        }
    }

    #[test]
    fn rejects_retired_models() {
        let cfg = config("beijing", "ws-123", "qwen3-livetranslate-flash-realtime");
        assert!(build_qwen_endpoint(&cfg).is_err());
    }

    #[test]
    fn builds_workspace_beijing_endpoint() {
        let cfg = config("beijing", "ws-123", CURRENT_MODEL);
        let (url, host) = build_qwen_endpoint(&cfg).unwrap();
        assert_eq!(host, "ws-123.cn-beijing.maas.aliyuncs.com");
        assert_eq!(
            url,
            "wss://ws-123.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen3.5-livetranslate-flash-realtime"
        );
    }

    #[test]
    fn qwen35_requires_workspace_id() {
        let cfg = config("beijing", "", CURRENT_MODEL);
        assert!(build_qwen_endpoint(&cfg).is_err());
    }

    #[test]
    fn source_transcript_is_always_requested() {
        let cfg = config("beijing", "ws-123", CURRENT_MODEL);
        let enabled: serde_json::Value = serde_json::from_str(&build_session_update(&cfg)).unwrap();
        assert_eq!(
            enabled["session"]["input_audio_transcription"]["model"],
            "qwen3-asr-flash-realtime"
        );
    }

    #[test]
    fn continuous_audio_uses_server_vad() {
        let cfg = config("beijing", "llm-test", CURRENT_MODEL);
        let update: serde_json::Value = serde_json::from_str(&build_session_update(&cfg)).unwrap();
        assert_eq!(update["session"]["sample_rate"], 16000);
        assert_eq!(update["session"]["turn_detection"]["type"], "server_vad");
        assert!(update["event_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("event_session_")));
    }

    #[test]
    fn preserves_qwen_server_error_details() {
        let error = serde_json::json!({
            "type": "error",
            "error": {
                "code": "invalid_value",
                "message": "Invalid language",
                "param": "session.translation.language"
            }
        });
        assert_eq!(
            server_error(&error.to_string()).as_deref(),
            Some("invalid_value: Invalid language: parameter: session.translation.language")
        );
    }

    #[test]
    fn redacts_credentials_from_connection_errors() {
        let cfg = config("beijing", "workspace-secret", CURRENT_MODEL);
        let error = sanitize_error(
            &cfg,
            "failed workspace-secret.cn-beijing.maas.aliyuncs.com with test-key",
        );
        assert_eq!(
            error,
            "failed <redacted>.cn-beijing.maas.aliyuncs.com with <redacted>"
        );
    }
}
