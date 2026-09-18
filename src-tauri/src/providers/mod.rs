pub mod openai_realtime;
pub mod qwen_realtime;
pub mod soniox_realtime;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

/// What every translator reports, whatever its wire protocol. Providers speak this so the
/// controller never has to know which one it is reading.
pub enum Event {
    Ready,
    /// `text` is always the whole segment so far, never a delta: a provider whose wire
    /// protocol streams increments joins them itself.
    Fragment {
        kind: FragmentKind,
        text: String,
        final_fragment: bool,
    },
    Error(String),
    Closed(String),
}

#[derive(Clone, Copy)]
pub enum FragmentKind {
    Original,
    Translation,
}

impl Event {
    pub fn fragment(kind: FragmentKind, text: impl Into<String>, final_fragment: bool) -> Self {
        Self::Fragment {
            kind,
            text: text.into(),
            final_fragment,
        }
    }
}

/// A handshake the server refused carries its reason in the response body, which the
/// transport's own message drops in favour of the bare status line.
pub fn connect_error(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    let Error::Http(response) = error else {
        return format!("websocket connect: {error}");
    };
    let body = response
        .body()
        .as_deref()
        .map(String::from_utf8_lossy)
        .map(|body| body.trim().to_string())
        .filter(|body| !body.is_empty());
    match body {
        Some(body) => format!(
            "{} {}",
            response.status(),
            coded_error(&body).unwrap_or(body)
        ),
        None => format!("HTTP error: {}", response.status()),
    }
}

/// `{"code","message"}` at the top level or under `error`, which is how the providers here
/// shape a rejection; anything else is shown as sent.
fn coded_error(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = value.get("error").unwrap_or(&value);
    let field = |name: &str| {
        error
            .get(name)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
    };
    let parts = [field("code"), field("message")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| parts.join(": "))
}

/// Carries provider events back to the controller. Cloneable so the session task and the
/// helpers it calls can all emit.
#[derive(Clone)]
pub struct Events(Arc<dyn Fn(Event) + Send + Sync>);

impl Events {
    pub fn callback(handler: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Self(Arc::new(handler))
    }

    pub fn emit(&self, event: Event) {
        (self.0)(event);
    }
}

/// One live link to a translator. Providers differ entirely below this: wire format,
/// sample rate, how a stop is negotiated.
pub trait Connection: Send {
    fn send_audio(&self, pcm: Vec<u8>) -> Result<(), String>;
    fn stop(&self);
}

/// Every open connection, of every provider, behind ids the controller treats as opaque.
/// An entry drops itself when its task ends, so audio can never reach a closed one.
#[derive(Default)]
pub struct ProviderState {
    connections: Arc<Mutex<HashMap<u64, Box<dyn Connection>>>>,
    next_id: Mutex<u64>,
}

impl ProviderState {
    pub fn start<F>(
        &self,
        connection: impl Connection + 'static,
        run: impl FnOnce(u64) -> F + Send + 'static,
    ) -> u64
    where
        F: Future<Output = ()> + Send,
    {
        let id = {
            let mut next = self.next_id.lock().unwrap();
            *next += 1;
            *next
        };
        self.connections
            .lock()
            .unwrap()
            .insert(id, Box::new(connection));
        let connections = self.connections.clone();
        tokio::spawn(async move {
            run(id).await;
            if let Ok(mut open) = connections.lock() {
                open.remove(&id);
            }
        });
        id
    }

    pub fn send_audio(&self, id: u64, pcm: Vec<u8>) -> Result<(), String> {
        self.connections
            .lock()
            .map_err(|error| error.to_string())?
            .get(&id)
            .ok_or_else(|| format!("Session {id} not found"))?
            .send_audio(pcm)
    }

    pub fn stop(&self, id: u64) {
        // The guard is released before the connection is told to stop, so a provider is
        // free to block in `stop` without stalling every other route.
        let closing = self
            .connections
            .lock()
            .ok()
            .and_then(|mut open| open.remove(&id));
        if let Some(connection) = closing {
            connection.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_handshake_shows_the_reason_the_server_sent() {
        for (body, shown) in [
            (
                r#"{"code":"InvalidApiKey","message":"Invalid API-key provided.","request_id":"x"}"#,
                "InvalidApiKey: Invalid API-key provided.",
            ),
            (
                r#"{"error":{"code":"invalid_api_key","message":"Incorrect API key provided"}}"#,
                "invalid_api_key: Incorrect API key provided",
            ),
            ("Unauthorized", "Unauthorized"),
        ] {
            assert_eq!(coded_error(body).unwrap_or(body.into()), shown);
        }
    }
}
