use super::{
    close_reason, dial, Connection, Event, Events, FragmentKind, Joined, ProviderState, SessionLog,
};
use crate::audio::resampler::UpsamplerTo24k;
use crate::diagnostics::field;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use http::Request;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

const OPENAI_REALTIME_URL: &str = "wss://api.openai.com/v1/realtime/translations";

/// No source language: the translation endpoint detects it from the audio and rejects
/// `session.audio.input.transcription.language` as an unknown parameter.
pub struct OpenAiRealtimeConfig {
    pub api_key: String,
    pub model: String,
    pub target_language: String,
}

/// The translations endpoint never marks segment ends — it streams deltas for as long as
/// audio arrives — so a lull in the deltas is the only turn boundary there is.
const SEGMENT_LULL: Duration = Duration::from_millis(1500);
/// Continuous audio (a film, a stream) can go minutes without a lull; past this age a
/// segment is committed mid-utterance rather than growing without bound.
const SEGMENT_CAP: Duration = Duration::from_secs(10);

/// Joins the delta stream into full-segment snapshots until [`Self::commit`] closes the
/// segment; a provider owes the controller snapshots, not deltas.
#[derive(Default)]
struct Segments {
    joined: Joined,
    last_delta: Option<Instant>,
    first_delta: Option<Instant>,
}

impl Segments {
    fn note_delta(&mut self) {
        let now = Instant::now();
        self.last_delta = Some(now);
        self.first_delta.get_or_insert(now);
    }

    fn due(&self) -> bool {
        let (Some(last), Some(first)) = (self.last_delta, self.first_delta) else {
            return false;
        };
        last.elapsed() >= SEGMENT_LULL || first.elapsed() >= SEGMENT_CAP
    }

    /// Both sides become final together so the assembler pairs them into one turn.
    fn commit(&mut self, events: &Events) {
        self.joined.finish(FragmentKind::Original, events);
        self.joined.finish(FragmentKind::Translation, events);
        self.last_delta = None;
        self.first_delta = None;
    }
}

struct Session {
    audio_tx: mpsc::UnboundedSender<Vec<u8>>,
    stop_tx: mpsc::UnboundedSender<()>,
    upsampler: Mutex<UpsamplerTo24k>,
}

impl Connection for Session {
    /// OpenAI expects 24 kHz, so the capture stream is resampled on the way in.
    fn send_audio(&self, pcm: Vec<u8>) -> Result<(), String> {
        let upsampled = self.upsampler.lock().unwrap().push(&pcm)?;
        if upsampled.is_empty() {
            return Ok(());
        }
        self.audio_tx
            .send(upsampled)
            .map_err(|error| format!("send audio failed: {error}"))
    }

    fn stop(&self) {
        let _ = self.stop_tx.send(());
    }
}

pub fn start_session(
    config: OpenAiRealtimeConfig,
    events: Events,
    state: &ProviderState,
) -> Result<u64, String> {
    let upsampler = Mutex::new(UpsamplerTo24k::new()?);
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (stop_tx, stop_rx) = mpsc::unbounded_channel::<()>();

    let details = format!(
        "model={} target={}",
        field(&config.model),
        field(&config.target_language),
    );
    Ok(state.start(
        "openai",
        details,
        Session {
            audio_tx,
            stop_tx,
            upsampler,
        },
        events,
        move |log, events| run_session(log, config, audio_rx, stop_rx, events),
    ))
}

async fn run_session(
    log: SessionLog,
    cfg: OpenAiRealtimeConfig,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut stop_rx: mpsc::UnboundedReceiver<()>,
    events: Events,
) -> Result<(), String> {
    let request = Request::builder()
        .uri(format!("{OPENAI_REALTIME_URL}?model={}", cfg.model))
        .header("Authorization", format!("Bearer {}", cfg.api_key))
        .header("Host", "api.openai.com")
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

    events.emit(Event::Ready);

    let mut segments = Segments::default();
    // Drives segment commits; the endpoint sends no boundary events to react to.
    let mut ticker = tokio::time::interval(Duration::from_millis(250));

    loop {
        tokio::select! {
            biased;

            _ = stop_rx.recv() => {
                log.log("stop_requested");
                segments.commit(&events);
                let _ = ws_sink.send(Message::Close(None)).await;
                break;
            }

            Some(audio_chunk) = audio_rx.recv() => {
                let b64 = B64.encode(&audio_chunk);
                let evt = serde_json::json!({
                    "type": "session.input_audio_buffer.append",
                    "audio": b64,
                });
                if let Err(e) = ws_sink.send(Message::Text(evt.to_string().into())).await {
                    return Err(sanitize_error(&cfg, format!("send audio: {e}")));
                }
            }

            msg = ws_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_server_event(&text, &events, &mut segments);
                    }
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Close(frame))) => {
                        segments.commit(&events);
                        events.emit(Event::Closed(close_reason(frame)));
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(sanitize_error(&cfg, format!("ws error: {e}"))),
                    None => {
                        segments.commit(&events);
                        break;
                    }
                }
            }

            _ = ticker.tick() => {
                if segments.due() {
                    segments.commit(&events);
                }
            }
        }
    }

    Ok(())
}

fn sanitize_error(cfg: &OpenAiRealtimeConfig, message: String) -> String {
    if cfg.api_key.is_empty() {
        message
    } else {
        message.replace(&cfg.api_key, "[redacted]")
    }
}

fn build_session_update(cfg: &OpenAiRealtimeConfig) -> String {
    let session = serde_json::json!({
        "audio": {
            "input": {
                "transcription": {"model": "gpt-realtime-whisper"},
                "noise_reduction": {"type": "near_field"}
            },
            "output": {"language": cfg.target_language}
        }
    });
    serde_json::json!({
        "type": "session.update",
        "session": session,
    })
    .to_string()
}

fn handle_server_event(text: &str, events: &Events, segments: &mut Segments) {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let evt_type = match value.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return,
    };
    // Deltas arrive many times a second; logging them would drown everything else.
    if !evt_type.ends_with(".delta") {
        crate::diagnostics::log(
            "openai",
            format!("event type={}", crate::diagnostics::field(evt_type)),
        );
    }

    match evt_type {
        "session.created" | "session.updated" => {}
        "session.input_transcript.delta" => {
            if let Some(delta) = value.get("delta").and_then(|v| v.as_str()) {
                segments.note_delta();
                segments.joined.push(FragmentKind::Original, delta, events);
            }
        }
        "session.output_transcript.delta" => {
            if let Some(delta) = value.get("delta").and_then(|v| v.as_str()) {
                segments.note_delta();
                segments
                    .joined
                    .push(FragmentKind::Translation, delta, events);
            }
        }
        // Translated speech; unused, subtitles only need the transcripts.
        "session.output_audio.delta" => {}
        "session.closed" => {
            events.emit(Event::Closed("session.closed".into()));
        }
        "error" => {
            let error = value.get("error");
            let field = |name: &str| {
                error
                    .and_then(|error| error.get(name))
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
            };
            let message = [field("code"), field("message")]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(": ");
            events.emit(Event::Error(if message.is_empty() {
                "OpenAI returned an unknown error".into()
            } else {
                message
            }));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::collector;

    #[test]
    /// The endpoint rejects any attempt to pin the input language, so only the output one
    /// may appear.
    fn session_update_sets_the_output_language_only() {
        let config = OpenAiRealtimeConfig {
            api_key: "test".into(),
            model: "gpt-realtime-translate".into(),
            target_language: "en".into(),
        };
        let value: serde_json::Value =
            serde_json::from_str(&build_session_update(&config)).unwrap();
        assert_eq!(value["session"]["audio"]["output"]["language"], "en");
        assert!(value["session"]["audio"]["input"]["transcription"]["language"].is_null());
    }

    /// The endpoint sends no boundary events, so the commit is the only place a segment
    /// ever becomes final.
    #[test]
    fn deltas_are_joined_into_snapshots_and_reset_on_commit() {
        let (events, collected) = collector();
        let mut segments = Segments::default();
        let delta = |value: &str| {
            format!(r#"{{"type":"session.output_transcript.delta","delta":"{value}"}}"#)
        };

        handle_server_event(&delta("hel"), &events, &mut segments);
        handle_server_event(&delta("lo"), &events, &mut segments);
        segments.commit(&events);
        // A committed segment is gone: nothing further to commit, and the next delta
        // must not inherit the previous text.
        segments.commit(&events);
        handle_server_event(&delta("bye"), &events, &mut segments);

        assert_eq!(
            *collected.lock().unwrap(),
            [
                ("translation", "hel".into(), false),
                ("translation", "hello".into(), false),
                ("translation", "hello".into(), true),
                ("translation", "bye".into(), false),
            ]
        );
    }

    /// Both transcripts close together so the assembler pairs them into one turn.
    #[test]
    fn a_commit_finalises_both_sides_at_once() {
        let (events, collected) = collector();
        let mut segments = Segments::default();
        handle_server_event(
            r#"{"type":"session.input_transcript.delta","delta":"hola"}"#,
            &events,
            &mut segments,
        );
        handle_server_event(
            r#"{"type":"session.output_transcript.delta","delta":"hello"}"#,
            &events,
            &mut segments,
        );
        segments.commit(&events);

        let finals: Vec<_> = collected
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, _, done)| *done)
            .map(|(side, text, _)| (*side, text.clone()))
            .collect();
        assert_eq!(
            finals,
            [("original", "hola".into()), ("translation", "hello".into())]
        );
        assert!(!segments.due());
    }

    #[test]
    fn redacts_credentials_from_connection_errors() {
        let config = OpenAiRealtimeConfig {
            api_key: "secret-key".into(),
            model: "gpt-realtime-translate".into(),
            target_language: "zh".into(),
        };
        let message = sanitize_error(&config, "request secret-key failed".into());
        assert_eq!(message, "request [redacted] failed");
    }
}
