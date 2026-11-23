use super::{
    close_reason, dial, Connection, Event, Events, FragmentKind, Joined, ProviderState, SessionLog,
};
use crate::diagnostics::field;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use http::Request;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// LiveTranslate's session setup and its streaming events change between model versions,
/// so the catalog names the version each model speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    LiveTranslate35,
    LiveTranslate38,
}

impl Protocol {
    pub fn parse(tag: Option<&str>) -> Result<Self, String> {
        match tag {
            Some("qwen-livetranslate-3.5") => Ok(Self::LiveTranslate35),
            Some("qwen-livetranslate-3.8") => Ok(Self::LiveTranslate38),
            other => Err(format!(
                "Unsupported Qwen LiveTranslate protocol: {}",
                other.unwrap_or("none")
            )),
        }
    }
}

pub struct QwenRealtimeConfig {
    pub api_key: String,
    pub source_language: String,
    pub target_language: String,
    pub region: String,
    pub workspace_id: String,
    pub model: String,
    pub protocol: Protocol,
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
    let details = format!(
        "model={} region={} source={} target={} workspace_configured={}",
        field(&config.model),
        field(&config.region),
        field(&config.source_language),
        field(&config.target_language),
        !config.workspace_id.trim().is_empty(),
    );
    Ok(state.start(
        "qwen",
        details,
        Session { audio_tx, stop_tx },
        events,
        move |log, events| run_session(log, config, audio_rx, stop_rx, events),
    ))
}

async fn run_session(
    log: SessionLog,
    cfg: QwenRealtimeConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut stop_rx: mpsc::UnboundedReceiver<()>,
    events: Events,
) -> Result<(), String> {
    let (qwen_realtime_url, qwen_host) = build_qwen_endpoint(&cfg)?;
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

    let Some(ws_stream) = dial(&log, request, &mut stop_rx)
        .await
        .map_err(|error| sanitize_error(&cfg, error))?
    else {
        return Ok(());
    };

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    let session_update = build_session_update(&cfg);
    ws_sink
        .send(Message::Text(session_update.into()))
        .await
        .map_err(|e| sanitize_error(&cfg, format!("send session.update: {e}")))?;
    log.log("session_update_sent");

    let mut stream = Stream::default();
    let mut ready = false;

    loop {
        tokio::select! {
            biased;

            _ = stop_rx.recv() => {
                log.log("stop_requested");
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
                                if handle_server_event(&text, &events, &mut stream) {
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
                        // Before `session.updated`, every event is a clue to why a session
                        // never became ready.
                        if !ready {
                            let kind = server_event_type(&text);
                            log.log(format!(
                                "server_event type={}",
                                field(kind.as_deref().unwrap_or("invalid_json"))
                            ));
                        }
                        if let Some(error) = server_error(&text) {
                            return Err(sanitize_error(&cfg, error));
                        }
                        if !ready && server_event_type(&text).as_deref() == Some("session.updated") {
                            ready = true;
                            events.emit(Event::Ready);
                        }
                        handle_server_event(&text, &events, &mut stream);
                    }
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Close(frame))) => {
                        events.emit(Event::Closed(close_reason(frame)));
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        return Err(sanitize_error(&cfg, format!("ws error: {e}")));
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
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
    let workspace_id = cfg.workspace_id.trim();
    if workspace_id.is_empty() {
        return Err("Qwen LiveTranslate requires a Model Studio Workspace ID".into());
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
    let session = match cfg.protocol {
        Protocol::LiveTranslate35 => {
            let source = if cfg.source_language.is_empty() || cfg.source_language == "auto" {
                "en"
            } else {
                cfg.source_language.as_str()
            };
            serde_json::json!({
                "modalities": ["text"],
                "sample_rate": 16000,
                "input_audio_format": "pcm",
                "input_audio_transcription": {
                    "language": source,
                    "model": "qwen3-asr-flash-realtime",
                },
                "translation": { "language": cfg.target_language },
                "turn_detection": {
                    "type": "server_vad",
                    "threshold": 0.2,
                    "silence_duration_ms": 800,
                },
            })
        }
        // 3.8 always transcribes and detects the source language itself, so it has no
        // field for either. Turn detection stays on its default, speaker detection: server
        // VAD stalled on audio that never falls silent (a podcast over music).
        Protocol::LiveTranslate38 => serde_json::json!({
            "output_modalities": ["text"],
            "audio": {
                "input": {
                    "format": { "type": "pcm", "sample_rate": 16000 },
                },
            },
            "translation": { "language": cfg.target_language },
        }),
    };

    serde_json::json!({
        "event_id": format!("event_session_{}", rand::random::<u64>()),
        "type": "session.update",
        "session": session,
    })
    .to_string()
}

/// 3.5 resends whole snapshots; 3.8 streams deltas, which only it fills `joined` with.
#[derive(Default)]
struct Stream {
    last_done_response_id: Option<String>,
    joined: Joined,
}

fn handle_server_event(text: &str, events: &Events, stream: &mut Stream) -> bool {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let evt_type = match value.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return false,
    };

    match evt_type {
        "session.created" | "session.updated" | "response.created" => {}
        "session.finished" => return true,

        // 3.8 does not promise a `response.text.done`; whatever is still open is final here.
        "response.done" => stream.joined.finish(FragmentKind::Translation, events),

        "response.text.delta" => {
            if let Some(delta) = value.get("delta").and_then(|v| v.as_str()) {
                stream.joined.push(FragmentKind::Translation, delta, events);
            }
        }

        "conversation.item.input_audio_transcription.delta" => {
            if let Some(delta) = value.get("delta").and_then(|v| v.as_str()) {
                stream.joined.push(FragmentKind::Original, delta, events);
            }
        }

        "response.text.text" => {
            let text = partial_text(&value);
            if !text.is_empty() {
                events.emit(Event::fragment(FragmentKind::Translation, text, false));
            }
        }

        "response.text.done" => {
            let response_id =
                string_field(&value, "response_id").or_else(|| string_field(&value, "item_id"));
            stream.joined.clear(FragmentKind::Translation);
            // The server can repeat a done event; emitting twice would duplicate the turn.
            if response_id.is_some() && response_id == stream.last_done_response_id {
                return false;
            }
            stream.last_done_response_id = response_id;

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
            stream.joined.clear(FragmentKind::Original);
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

    const QWEN35: &str = "qwen3.5-livetranslate-flash-realtime";

    fn config(region: &str, workspace_id: &str, protocol: Protocol) -> QwenRealtimeConfig {
        QwenRealtimeConfig {
            api_key: "test-key".into(),
            source_language: "zh".into(),
            target_language: "en".into(),
            region: region.into(),
            workspace_id: workspace_id.into(),
            model: match protocol {
                Protocol::LiveTranslate35 => QWEN35,
                Protocol::LiveTranslate38 => "qwen3.8-livetranslate-flash-realtime",
            }
            .into(),
            protocol,
        }
    }

    fn session_update(protocol: Protocol) -> serde_json::Value {
        serde_json::from_str(&build_session_update(&config("beijing", "ws", protocol))).unwrap()
    }

    fn replay(events: &[serde_json::Value]) -> Vec<(&'static str, String, bool)> {
        let (callback, collected) = crate::providers::collector();
        let mut stream = Stream::default();
        for event in events {
            handle_server_event(&event.to_string(), &callback, &mut stream);
        }
        let fragments = collected.lock().unwrap().clone();
        fragments
    }

    /// A model the code was not written for must fail before it dials.
    #[test]
    fn refuses_an_unknown_protocol() {
        assert!(Protocol::parse(None).is_err());
        assert!(Protocol::parse(Some("qwen-livetranslate-9")).is_err());
        assert_eq!(
            Protocol::parse(Some("qwen-livetranslate-3.8")),
            Ok(Protocol::LiveTranslate38)
        );
    }

    #[test]
    fn builds_workspace_beijing_endpoint() {
        let cfg = config("beijing", "ws-123", Protocol::LiveTranslate35);
        let (url, host) = build_qwen_endpoint(&cfg).unwrap();
        assert_eq!(host, "ws-123.cn-beijing.maas.aliyuncs.com");
        assert_eq!(
            url,
            format!("wss://ws-123.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model={QWEN35}")
        );
    }

    #[test]
    fn requires_workspace_id() {
        let cfg = config("beijing", "", Protocol::LiveTranslate38);
        assert!(build_qwen_endpoint(&cfg).is_err());
    }

    #[test]
    fn qwen35_requests_the_source_transcript_and_server_vad() {
        let update = session_update(Protocol::LiveTranslate35);
        let session = &update["session"];
        assert_eq!(
            session["input_audio_transcription"]["model"],
            "qwen3-asr-flash-realtime"
        );
        assert_eq!(session["input_audio_transcription"]["language"], "zh");
        assert_eq!(session["sample_rate"], 16000);
        assert_eq!(session["turn_detection"]["type"], "server_vad");
        assert!(update["event_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("event_session_")));
    }

    /// 3.8 rejects or ignores 3.5's flat fields, so none of them may leak into its setup.
    #[test]
    fn qwen38_nests_audio_and_sends_no_source_language() {
        let session = &session_update(Protocol::LiveTranslate38)["session"];
        assert_eq!(session["output_modalities"], serde_json::json!(["text"]));
        assert_eq!(session["audio"]["input"]["format"]["type"], "pcm");
        assert_eq!(session["audio"]["input"]["format"]["sample_rate"], 16000);
        assert!(session["audio"]["input"].get("turn_detection").is_none());
        assert_eq!(session["translation"]["language"], "en");
        for field in [
            "modalities",
            "sample_rate",
            "input_audio_format",
            "input_audio_transcription",
            "turn_detection",
        ] {
            assert!(session.get(field).is_none(), "{field}");
        }
    }

    #[test]
    fn qwen38_deltas_become_whole_segments() {
        let fragments = replay(&[
            serde_json::json!({"type": "conversation.item.input_audio_transcription.delta", "delta": "你好"}),
            serde_json::json!({"type": "conversation.item.input_audio_transcription.delta", "delta": "世界"}),
            serde_json::json!({"type": "conversation.item.input_audio_transcription.completed", "transcript": "你好世界"}),
            serde_json::json!({"type": "response.text.delta", "delta": "Hello"}),
            serde_json::json!({"type": "response.text.delta", "delta": " world"}),
            serde_json::json!({"type": "response.text.done", "response_id": "r1", "text": "Hello world"}),
            serde_json::json!({"type": "response.done", "response": {"id": "r1"}}),
            serde_json::json!({"type": "conversation.item.input_audio_transcription.delta", "delta": "再见"}),
        ]);
        let expected = [
            ("original", "你好", false),
            ("original", "你好世界", false),
            ("original", "你好世界", true),
            ("translation", "Hello", false),
            ("translation", "Hello world", false),
            ("translation", "Hello world", true),
            ("original", "再见", false),
        ]
        .map(|(side, text, done)| (side, text.to_string(), done));
        assert_eq!(fragments, expected);
    }

    #[test]
    fn qwen38_translation_closes_at_response_done() {
        let fragments = replay(&[
            serde_json::json!({"type": "response.text.delta", "delta": "Hi"}),
            serde_json::json!({"type": "response.done", "response": {"id": "r1"}}),
            serde_json::json!({"type": "response.done", "response": {"id": "r2"}}),
        ]);
        assert_eq!(
            fragments,
            [
                ("translation", "Hi".into(), false),
                ("translation", "Hi".into(), true)
            ]
        );
    }

    #[test]
    fn qwen35_snapshots_pass_through() {
        let fragments = replay(&[
            serde_json::json!({"type": "response.text.text", "text": "Hel", "stash": "lo"}),
            serde_json::json!({"type": "response.text.done", "response_id": "r1", "text": "Hello"}),
            serde_json::json!({"type": "response.text.done", "response_id": "r1", "text": "Hello"}),
            serde_json::json!({"type": "response.done", "response": {"id": "r1"}}),
        ]);
        assert_eq!(
            fragments,
            [
                ("translation", "Hello".into(), false),
                ("translation", "Hello".into(), true)
            ]
        );
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
        let cfg = config("beijing", "workspace-secret", Protocol::LiveTranslate35);
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
