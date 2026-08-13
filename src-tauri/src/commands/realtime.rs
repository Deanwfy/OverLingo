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
    /// `retryable` is false for anything a reconnect cannot fix: a rejected key, an
    /// exhausted balance, a configuration the provider refuses.
    Error {
        message: String,
        retryable: bool,
    },
    Closed(String),
}

#[derive(Clone, Copy)]
pub enum FragmentKind {
    Original,
    Translation,
}

impl Event {
    /// Classifies from the text, for providers whose wire errors carry no usable code.
    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        let retryable = !is_permanent(&message);
        Self::Error { message, retryable }
    }

    /// For a provider that knows from its own error shape that retrying is pointless.
    pub fn fatal(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            retryable: false,
        }
    }

    pub fn fragment(kind: FragmentKind, text: impl Into<String>, final_fragment: bool) -> Self {
        Self::Fragment {
            kind,
            text: text.into(),
            final_fragment,
        }
    }
}

/// Rate limits, outages and quotas all clear on their own; a rejected credential does not.
/// The list stays narrow because guessing wrong the other way only costs a few seconds of
/// retrying, while a false positive strands a route that would have recovered.
fn is_permanent(message: &str) -> bool {
    let message = message.to_lowercase();
    [
        "401",
        "402",
        "403",
        "unauthorized",
        "unauthorised",
        "invalid api key",
        "invalid_api_key",
        "authentication",
        "access denied",
        "forbidden",
    ]
    .iter()
    .any(|marker| message.contains(marker))
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
