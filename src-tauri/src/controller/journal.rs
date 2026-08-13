use super::{enabled_route_ids, route_config};
use crate::app_config::AppConfig;
use crate::commands::session_store::{self, Chunk, ExportMode, Segment, SessionData, SessionRoute};
use crate::controller::transcript::TranscriptTurn;
use chrono::{DateTime, Local, SecondsFormat, Utc};
use std::time::Duration;
use tauri::AppHandle;

struct JournalTurn {
    turn: TranscriptTurn,
    elapsed: Duration,
}

pub(super) struct Journal {
    id: String,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    config: AppConfig,
    turns: Vec<JournalTurn>,
    elapsed: Duration,
}

impl Journal {
    pub(super) fn new(config: &AppConfig) -> Self {
        let started_at = Utc::now();
        Self {
            id: started_at.format("%Y%m%d-%H%M%S-%3f").to_string(),
            started_at,
            ended_at: None,
            config: config.clone(),
            turns: Vec::new(),
            elapsed: Duration::ZERO,
        }
    }

    pub(super) fn add(&mut self, turn: TranscriptTurn, elapsed: Duration) {
        self.elapsed = elapsed;
        self.turns.push(JournalTurn { turn, elapsed });
    }

    pub(super) fn finish(&mut self, elapsed: Duration) {
        self.elapsed = elapsed;
        self.ended_at = Some(Utc::now());
    }

    pub(super) fn persist(&self, app: &AppHandle) -> Result<(), String> {
        if self.turns.is_empty() {
            return Ok(());
        }
        let data = self.data();
        let markdown = session_store::render_markdown(&data, ExportMode::Both);
        session_store::persist_session(app, self.id.clone(), markdown, data)
    }

    fn data(&self) -> SessionData {
        let routes = enabled_route_ids(&self.config)
            .into_iter()
            .filter_map(|id| route_config(&self.config, id).map(|route| (id, route)))
            .map(|(id, route)| SessionRoute {
                id: id.into(),
                input: route.input.clone(),
                engine: route.engine.clone(),
                model: route.model.clone(),
                source_lang: route.source_language.clone(),
                target_lang: route.target_language.clone(),
            })
            .collect::<Vec<_>>();
        let ended_at = self.ended_at;
        let first = routes.first();
        SessionData {
            schema_version: 1,
            id: self.id.clone(),
            created_at: self.started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            ended_at: ended_at.map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true)),
            title: self
                .started_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            engine: unique_engines(&routes),
            source_lang: first.map_or(String::new(), |route| route.source_lang.clone()),
            target_lang: first.map_or(String::new(), |route| route.target_lang.clone()),
            duration_sec: self.elapsed.as_secs(),
            routes,
            chunks: vec![Chunk {
                started_at: self.started_at.to_rfc3339_opts(SecondsFormat::Millis, true),
                ended_at: ended_at.map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true)),
                segments: self
                    .turns
                    .iter()
                    .map(|entry| Segment {
                        ts: elapsed_label(entry.elapsed),
                        src: entry.turn.original.clone(),
                        tgt: entry.turn.translation.clone(),
                        route_id: entry.turn.route_id.clone(),
                        engine: entry.turn.engine.clone(),
                        source_lang: entry.turn.source_language.clone(),
                        target_lang: entry.turn.target_language.clone(),
                    })
                    .collect(),
            }],
        }
    }
}

fn unique_engines(routes: &[SessionRoute]) -> String {
    let mut engines = Vec::new();
    for route in routes {
        if !engines.contains(&route.engine) {
            engines.push(route.engine.clone());
        }
    }
    engines.join("+")
}

fn elapsed_label(elapsed: Duration) -> String {
    session_store::elapsed_label(elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_active_duration() {
        let mut journal = Journal::new(&AppConfig::default());
        journal.finish(Duration::from_secs(75));

        assert_eq!(journal.data().duration_sec, 75);
        assert_eq!(elapsed_label(Duration::from_secs(75)), "00:01:15");
    }
}
