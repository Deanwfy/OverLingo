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

    /// The session behind these turns is ending, and no final will come for its open
    /// segment, which audio that never falls silent can keep open for half a minute. That
    /// segment closes too, and later turns are labelled with `next`.
    pub(super) fn finish(&mut self, next: &RouteConfig) -> Vec<TranscriptTurn> {
        let draft = std::mem::take(&mut self.draft);
        if !draft.original.is_empty() {
            self.originals.push_back(draft.original);
        }
        if !draft.translation.is_empty() {
            self.translations.push_back(draft.translation);
        }
        let outputs = self.flush_all();
        self.route = next.clone();
        outputs
    }

    fn flush_all(&mut self) -> Vec<TranscriptTurn> {
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

#[cfg(test)]
mod tests {
    use super::super::SYSTEM_ROUTE;
    use super::*;
    use crate::app_config::AppConfig;

    fn route() -> RouteConfig {
        AppConfig::default().routes.system
    }

    fn texts(turns: &[TranscriptTurn]) -> Vec<(&str, &str)> {
        turns
            .iter()
            .map(|turn| (turn.original.as_str(), turn.translation.as_str()))
            .collect()
    }

    #[test]
    fn pairs_final_fragments_in_order() {
        let mut turns = TurnAssembler::new(SYSTEM_ROUTE, &route());
        turns.push(FragmentKind::Original, "one".into(), true);
        let (outputs, _) = turns.push(FragmentKind::Translation, "一".into(), true);
        assert_eq!(texts(&outputs), [("one", "一")]);
    }

    /// Fragments are snapshots, so the newest one replaces the draft whatever the provider.
    #[test]
    fn draft_tracks_the_latest_snapshot() {
        for engine in ["qwen", "openai"] {
            let route = RouteConfig {
                engine: engine.into(),
                ..route()
            };
            let mut turns = TurnAssembler::new(SYSTEM_ROUTE, &route);
            turns.push(FragmentKind::Translation, "hel".into(), false);
            turns.push(FragmentKind::Translation, "hello".into(), false);
            assert_eq!(turns.draft.translation, "hello", "{engine}");
        }
    }

    #[test]
    fn finishing_keeps_the_open_segment() {
        let mut turns = TurnAssembler::new(SYSTEM_ROUTE, &route());
        turns.push(FragmentKind::Original, "今日は".into(), false);
        turns.push(FragmentKind::Translation, "今天".into(), false);
        assert_eq!(texts(&turns.finish(&route())), [("今日は", "今天")]);
        assert!(turns.finish(&route()).is_empty());
    }

    /// A closed side pairs with the other side's open text, not with an empty turn.
    #[test]
    fn finishing_pairs_a_closed_side_with_the_open_one() {
        let mut turns = TurnAssembler::new(SYSTEM_ROUTE, &route());
        turns.push(FragmentKind::Original, "おはよう".into(), true);
        turns.push(FragmentKind::Translation, "早".into(), false);
        assert_eq!(texts(&turns.finish(&route())), [("おはよう", "早")]);
    }

    #[test]
    fn turns_after_finishing_carry_the_next_settings() {
        let mut turns = TurnAssembler::new(SYSTEM_ROUTE, &route());
        let next = RouteConfig {
            model: "next-model".into(),
            ..route()
        };
        turns.finish(&next);
        turns.push(FragmentKind::Original, "a".into(), true);
        let (outputs, _) = turns.push(FragmentKind::Translation, "b".into(), true);
        assert_eq!(outputs[0].model, "next-model");
    }

    #[test]
    fn a_timed_flush_closes_only_queued_finals() {
        let mut turns = TurnAssembler::new(SYSTEM_ROUTE, &route());
        let (_, version) = turns.push(FragmentKind::Original, "はい".into(), true);
        turns.push(FragmentKind::Original, "それで".into(), false);
        assert_eq!(texts(&turns.flush(version.unwrap())), [("はい", "")]);
    }
}
