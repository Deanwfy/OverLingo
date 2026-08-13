use super::realtime::{Connection, Event, Events, FragmentKind, ProviderState};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const SONIOX_REALTIME_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const MODEL: &str = "stt-rt-v5";
/// Soniox marks the end of an utterance with this token rather than a message flag.
const END_TOKEN: &str = "<end>";

pub struct SonioxRealtimeConfig {
    pub api_key: String,
    pub source_language: String,
    pub target_language: String,
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
    config: SonioxRealtimeConfig,
    events: Events,
    state: &ProviderState,
) -> Result<u64, String> {
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (stop_tx, stop_rx) = mpsc::unbounded_channel::<()>();

    let id = state.start(Session { audio_tx, stop_tx }, move |_| async move {
        if let Err(error) = run_session(config, audio_rx, stop_rx, events.clone()).await {
            events.emit(Event::error(error));
        }
        events.emit(Event::Closed("session_ended".into()));
    });
    Ok(id)
}

async fn run_session(
    cfg: SonioxRealtimeConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut stop_rx: mpsc::UnboundedReceiver<()>,
    events: Events,
) -> Result<(), String> {
    let connect = tokio::time::timeout(
        Duration::from_secs(14),
        connect_async(SONIOX_REALTIME_URL.to_string()),
    );
    tokio::pin!(connect);
    let (ws_stream, _) = tokio::select! {
        result = &mut connect => {
            result
                .map_err(|_| "websocket handshake timed out".to_string())?
                .map_err(|error| sanitize_error(&cfg, format!("websocket connect: {error}")))?
        }
        _ = stop_rx.recv() => return Ok(()),
    };

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    ws_sink
        .send(Message::Text(build_config(&cfg).into()))
        .await
        .map_err(|error| sanitize_error(&cfg, format!("send config: {error}")))?;

    events.emit(Event::Ready);

    let mut segments = Segments::default();

    loop {
        tokio::select! {
            biased;

            _ = stop_rx.recv() => {
                // An empty frame tells Soniox no more audio is coming, so the tail of the
                // last utterance is still transcribed instead of being cut off.
                let _ = ws_sink.send(Message::Binary(Vec::new().into())).await;
                let _ = ws_sink.send(Message::Close(None)).await;
                break;
            }

            Some(pcm) = audio_rx.recv() => {
                if let Err(error) = ws_sink.send(Message::Binary(pcm.into())).await {
                    return Err(sanitize_error(&cfg, format!("send audio: {error}")));
                }
            }

            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if handle_server_message(&text, &events, &mut segments) {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let reason = frame
                            .map(|frame| format!("{}: {}", frame.code, frame.reason))
                            .unwrap_or_else(|| "connection_closed".into());
                        events.emit(Event::Closed(reason));
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        return Err(sanitize_error(&cfg, format!("ws error: {error}")))
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

fn sanitize_error(cfg: &SonioxRealtimeConfig, message: String) -> String {
    if cfg.api_key.is_empty() {
        message
    } else {
        message.replace(&cfg.api_key, "[redacted]")
    }
}

fn build_config(cfg: &SonioxRealtimeConfig) -> String {
    serde_json::json!({
        "api_key": cfg.api_key,
        "model": MODEL,
        "audio_format": "pcm_s16le",
        "sample_rate": 16_000,
        "num_channels": 1,
        "language_hints": [cfg.source_language],
        "enable_endpoint_detection": true,
        "translation": {
            "type": "one_way",
            "target_language": cfg.target_language,
        },
    })
    .to_string()
}

/// Soniox streams loose tokens: finalized ones arrive once, non-final ones are resent and
/// replaced every message. Each stream is rebuilt here so the controller sees whole-segment
/// snapshots, and `<end>` closes both streams at once because it marks one spoken utterance.
#[derive(Default)]
struct Segments {
    original: Segment,
    translation: Segment,
}

#[derive(Default)]
struct Segment {
    settled: String,
    pending: String,
}

impl Segment {
    fn snapshot(&self) -> String {
        format!("{}{}", self.settled, self.pending)
    }

    fn take(&mut self) -> String {
        self.pending.clear();
        std::mem::take(&mut self.settled)
    }
}

/// Emits whatever the message carried and reports whether it ended the session.
fn handle_server_message(text: &str, events: &Events, segments: &mut Segments) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    if let Some(error) = server_error(&value) {
        events.emit(error);
        return true;
    }
    let Some(tokens) = value.get("tokens").and_then(|tokens| tokens.as_array()) else {
        return false;
    };

    let had_pending =
        !segments.original.pending.is_empty() || !segments.translation.pending.is_empty();
    segments.original.pending.clear();
    segments.translation.pending.clear();
    let mut ended = false;
    let mut touched = had_pending;

    for token in tokens {
        // A token without text is not something to guess at, but it must not swallow the
        // ones after it either.
        let Some(text) = token.get("text").and_then(|text| text.as_str()) else {
            continue;
        };
        if text == END_TOKEN {
            ended = true;
            continue;
        }
        touched = true;
        let translated = token
            .get("translation_status")
            .and_then(|status| status.as_str())
            == Some("translation");
        let segment = if translated {
            &mut segments.translation
        } else {
            &mut segments.original
        };
        if token.get("is_final").and_then(|flag| flag.as_bool()) == Some(true) {
            segment.settled.push_str(text);
        } else {
            segment.pending.push_str(text);
        }
    }

    // Every emission costs the controller a full snapshot publish, so a message that
    // carried nothing for either stream stays quiet.
    if !touched && !ended {
        return false;
    }
    for (kind, segment) in [
        (FragmentKind::Original, &mut segments.original),
        (FragmentKind::Translation, &mut segments.translation),
    ] {
        let snapshot = if ended {
            segment.take()
        } else {
            segment.snapshot()
        };
        if !snapshot.trim().is_empty() {
            events.emit(Event::fragment(kind, snapshot, ended));
        }
    }
    false
}

fn server_error(value: &serde_json::Value) -> Option<Event> {
    let code = value.get("error_code").and_then(serde_json::Value::as_u64);
    let field = |name: &str| {
        value
            .get(name)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
    };
    let message = [field("error_type"), field("error_message")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(": ");
    let code = code?;
    let message = if message.is_empty() {
        format!("Soniox returned error {code}")
    } else {
        message
    };
    // A rejected key or a spent balance answers the same way however often it is asked.
    Some(if (401..=403).contains(&code) {
        Event::fatal(message)
    } else {
        Event::error(message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// (stream, snapshot, final) triples, in the order the provider emitted them.
    type Emitted = Arc<Mutex<Vec<(String, String, bool)>>>;

    fn collector() -> (Events, Emitted) {
        let collected: Emitted = Arc::new(Mutex::new(Vec::new()));
        let events = Events::callback({
            let collected = collected.clone();
            move |event| {
                if let Event::Fragment {
                    kind,
                    text,
                    final_fragment,
                } = event
                {
                    let kind = match kind {
                        FragmentKind::Original => "original",
                        FragmentKind::Translation => "translation",
                    };
                    collected
                        .lock()
                        .unwrap()
                        .push((kind.into(), text, final_fragment));
                }
            }
        });
        (events, collected)
    }

    #[test]
    fn config_requests_both_directions_as_raw_pcm() {
        let config = SonioxRealtimeConfig {
            api_key: "test".into(),
            source_language: "en".into(),
            target_language: "zh".into(),
        };
        let value: serde_json::Value = serde_json::from_str(&build_config(&config)).unwrap();
        assert_eq!(value["audio_format"], "pcm_s16le");
        assert_eq!(value["sample_rate"], 16_000);
        assert_eq!(value["language_hints"][0], "en");
        assert_eq!(value["translation"]["target_language"], "zh");
        assert_eq!(value["enable_endpoint_detection"], true);
    }

    /// Non-final tokens are resent every message, so they must replace rather than append.
    #[test]
    fn pending_tokens_replace_instead_of_accumulating() {
        let (events, collected) = collector();
        let mut segments = Segments::default();

        handle_server_message(
            r#"{"tokens":[{"text":"He","is_final":false,"translation_status":"original"}]}"#,
            &events,
            &mut segments,
        );
        handle_server_message(
            r#"{"tokens":[{"text":"Hello","is_final":false,"translation_status":"original"}]}"#,
            &events,
            &mut segments,
        );

        assert_eq!(
            *collected.lock().unwrap(),
            vec![
                ("original".into(), "He".into(), false),
                ("original".into(), "Hello".into(), false),
            ]
        );
    }

    #[test]
    fn an_end_token_closes_both_streams_and_resets_them() {
        let (events, collected) = collector();
        let mut segments = Segments::default();

        handle_server_message(
            r#"{"tokens":[
                {"text":"Hello","is_final":true,"translation_status":"original"},
                {"text":"你好","is_final":true,"translation_status":"translation"},
                {"text":"<end>","is_final":true}
            ]}"#,
            &events,
            &mut segments,
        );
        // The next utterance must not inherit the previous one's text.
        handle_server_message(
            r#"{"tokens":[{"text":"Bye","is_final":false,"translation_status":"original"}]}"#,
            &events,
            &mut segments,
        );

        assert_eq!(
            *collected.lock().unwrap(),
            vec![
                ("original".into(), "Hello".into(), true),
                ("translation".into(), "你好".into(), true),
                ("original".into(), "Bye".into(), false),
            ]
        );
    }

    /// Publishing a snapshot is expensive, so keep-alive messages must not trigger one.
    #[test]
    fn a_message_with_no_tokens_emits_nothing() {
        let (events, collected) = collector();
        let mut segments = Segments::default();
        handle_server_message(r#"{"tokens":[]}"#, &events, &mut segments);
        assert!(collected.lock().unwrap().is_empty());
    }

    /// A rejected key answers the same way however often it is asked, so it must end the
    /// session outright instead of spending the retry budget.
    #[test]
    fn a_rejected_key_ends_the_session_without_a_retry() {
        let error = server_error(&serde_json::json!({
            "tokens": [],
            "error_code": 401,
            "error_type": "unauthorized",
            "error_message": "bad key",
        }));
        assert!(matches!(
            error,
            Some(Event::Error { ref message, retryable: false }) if message == "unauthorized: bad key"
        ));
    }

    #[test]
    fn a_server_outage_is_worth_retrying() {
        let error = server_error(&serde_json::json!({
            "tokens": [],
            "error_code": 503,
            "error_type": "service_unavailable",
        }));
        assert!(matches!(
            error,
            Some(Event::Error {
                retryable: true,
                ..
            })
        ));
    }
}
