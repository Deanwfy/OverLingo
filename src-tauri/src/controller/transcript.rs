use super::provider::FragmentKind;
use crate::app_config::RouteConfig;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct TranscriptDraft {
    pub(super) original: String,
    pub(super) translation: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TranscriptTurn {
    #[serde(rename = "type")]
    kind: &'static str,
    pub(super) route_id: String,
    pub(super) original: String,
    pub(super) translation: String,
    pub(super) timestamp: String,
    pub(super) engine: String,
    model: String,
    pub(super) source_language: String,
    pub(super) target_language: String,
}

#[derive(Clone)]
pub(super) struct TurnAssembler {
    route_id: String,
    route: RouteConfig,
    originals: VecDeque<String>,
    translations: VecDeque<String>,
    pub(super) draft: TranscriptDraft,
    flush_version: u64,
}

impl TurnAssembler {
    pub(super) fn new(route_id: &str, route: &RouteConfig) -> Self {
        Self {
            route_id: route_id.into(),
            route: route.clone(),
            originals: VecDeque::new(),
            translations: VecDeque::new(),
            draft: TranscriptDraft::default(),
            flush_version: 0,
        }
    }

    pub(super) fn push(
        &mut self,
        kind: FragmentKind,
        text: String,
        final_fragment: bool,
    ) -> (Vec<TranscriptTurn>, Option<u64>) {
        let value = text.trim().to_string();
        if value.is_empty() {
            return (Vec::new(), None);
        }
        match kind {
            FragmentKind::Original => self.draft.original = value.clone(),
            FragmentKind::Translation => self.draft.translation = value.clone(),
        }
        if !final_fragment {
            return (Vec::new(), None);
        }
        match kind {
            FragmentKind::Original => {
                self.originals.push_back(value);
                self.draft.original.clear();
            }
            FragmentKind::Translation => {
                self.translations.push_back(value);
                self.draft.translation.clear();
            }
        }
        let outputs = self.pair();
        if self.originals.is_empty() && self.translations.is_empty() {
            self.flush_version = self.flush_version.wrapping_add(1);
            (outputs, None)
        } else {
            self.flush_version = self.flush_version.wrapping_add(1);
            (outputs, Some(self.flush_version))
        }
    }

    fn pair(&mut self) -> Vec<TranscriptTurn> {
        let mut outputs = Vec::new();
        while !self.originals.is_empty() && !self.translations.is_empty() {
            let original = self.originals.pop_front().unwrap_or_default();
            let translation = self.translations.pop_front().unwrap_or_default();
            outputs.push(self.turn(original, translation));
        }
        outputs
    }

    pub(super) fn flush(&mut self, version: u64) -> Vec<TranscriptTurn> {
        if version != self.flush_version {
            return Vec::new();
        }
        self.flush_all()
    }

    pub(super) fn flush_all(&mut self) -> Vec<TranscriptTurn> {
        self.flush_version = self.flush_version.wrapping_add(1);
        let mut outputs = Vec::new();
        while !self.originals.is_empty() || !self.translations.is_empty() {
            let original = self.originals.pop_front().unwrap_or_default();
            let translation = self.translations.pop_front().unwrap_or_default();
            outputs.push(self.turn(original, translation));
        }
        self.draft = TranscriptDraft::default();
        outputs
    }

    fn turn(&self, original: String, translation: String) -> TranscriptTurn {
        TranscriptTurn {
            kind: "turn",
            route_id: self.route_id.clone(),
            original,
            translation,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            engine: self.route.engine.clone(),
            model: self.route.model.clone(),
            source_language: self.route.source_language.clone(),
            target_language: self.route.target_language.clone(),
        }
    }
}
