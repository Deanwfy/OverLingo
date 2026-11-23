use crate::app_config::{AppConfig, RouteConfig};
use crate::credentials::CredentialStore;
use crate::providers::openai_realtime::{self, OpenAiRealtimeConfig};
use crate::providers::qwen_realtime::{self, QwenRealtimeConfig};
use crate::providers::soniox_realtime::{self, SonioxRealtimeConfig};
use crate::providers::{Events, ProviderState};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Everything one route needs to dial its translator. `api_key` is resolved from the
/// store by engine id, so no provider names itself twice.
struct StartRequest<'a> {
    config: &'a AppConfig,
    route: &'a RouteConfig,
    api_key: &'a str,
    /// The catalog's protocol tag for the route's model; only the provider reads it.
    protocol: Option<&'a str>,
}

/// A provider: owns one API key and contributes one or more models. Everything
/// provider-specific lives in this table so the layers above stay engine-blind.
struct Engine {
    id: &'static str,
    /// The provider-owned settings that force a session rebuild when they change.
    fingerprint: fn(&AppConfig) -> String,
    /// A precondition beyond holding an API key, as the notice code that blocks a start.
    blocker: fn(&AppConfig) -> Option<&'static str>,
    open: fn(StartRequest, Events, &ProviderState) -> Result<u64, String>,
    /// Whether this provider can drive a model tagged with the given catalog protocol.
    speaks: fn(Option<&str>) -> bool,
}

/// Regions the LiveTranslate realtime endpoint is documented in. Model Studio runs in more
/// of them, but each region carries its own model catalog, so availability is per model.
const QWEN_REGIONS: &[(&str, &str)] = &[("beijing", "cn-beijing"), ("singapore", "ap-southeast-1")];

/// The Alibaba Cloud region id behind a stored region name.
pub(crate) fn qwen_region(name: &str) -> Option<&'static str> {
    QWEN_REGIONS
        .iter()
        .find(|(stored, _)| *stored == name)
        .map(|(_, region)| *region)
}

const ENGINES: &[Engine] = &[
    Engine {
        id: "qwen",
        fingerprint: |config| {
            format!(
                "{}\u{1f}{}",
                config.qwen.region.as_str(),
                config.qwen.workspace_id.as_str()
            )
        },
        blocker: |config| {
            config
                .qwen
                .workspace_id
                .trim()
                .is_empty()
                .then_some("workspaceRequired")
        },
        open: |request, events, state| {
            qwen_realtime::start_session(
                QwenRealtimeConfig {
                    api_key: request.api_key.to_owned(),
                    source_language: request.route.source_language.clone(),
                    target_language: request.route.target_language.clone(),
                    region: request.config.qwen.region.clone(),
                    workspace_id: request.config.qwen.workspace_id.clone(),
                    model: request.route.model.clone(),
                    protocol: qwen_realtime::Protocol::parse(request.protocol)?,
                },
                events,
                state,
            )
        },
        speaks: |protocol| qwen_realtime::Protocol::parse(protocol).is_ok(),
    },
    Engine {
        id: "soniox",
        fingerprint: |_| String::new(),
        blocker: |_| None,
        open: |request, events, state| {
            soniox_realtime::start_session(
                SonioxRealtimeConfig {
                    api_key: request.api_key.to_owned(),
                    model: request.route.model.clone(),
                    source_language: request.route.source_language.clone(),
                    target_language: request.route.target_language.clone(),
                },
                events,
                state,
            )
        },
        speaks: |protocol| protocol.is_none(),
    },
    Engine {
        id: "openai",
        fingerprint: |_| String::new(),
        blocker: |_| None,
        open: |request, events, state| {
            openai_realtime::start_session(
                OpenAiRealtimeConfig {
                    api_key: request.api_key.to_owned(),
                    model: request.route.model.clone(),
                    target_language: request.route.target_language.clone(),
                },
                events,
                state,
            )
        },
        speaks: |protocol| protocol.is_none(),
    },
];

/// A single translator model. `engine` names the provider that owns its API key; a provider
/// with several models contributes several entries, so nothing downstream unpacks a nested list.
#[derive(Deserialize)]
struct Translator {
    id: String,
    #[serde(rename = "provider")]
    engine: String,
    /// Which wire protocol the provider speaks for this model, when it has more than one.
    protocol: Option<String>,
    /// The model a retired route on this provider falls back to; one per provider.
    #[serde(default)]
    default: bool,
    languages: Languages,
}

/// Language codes are BCP 47 throughout; an adapter translates its provider's spellings.
#[derive(Deserialize)]
struct Languages {
    /// `None` when the model has no restriction of its own on this end.
    source: Option<Codes>,
    target: Option<Codes>,
    /// Target → the sources allowed into it, for providers that only pair some languages.
    #[serde(default)]
    pairs: HashMap<String, Vec<String>>,
}

/// A list as the catalog writes it: inline, or the name of a set several models share.
#[derive(Deserialize)]
#[serde(untagged)]
enum Codes {
    Inline(Vec<String>),
    Set(String),
}

impl Codes {
    fn resolve(&mut self, sets: &HashMap<String, Vec<String>>) {
        if let Codes::Set(name) = self {
            let codes = sets
                .get(name)
                .unwrap_or_else(|| panic!("unknown language set {name}"));
            *self = Codes::Inline(codes.clone());
        }
    }

    fn contains(&self, code: &str) -> bool {
        match self {
            Codes::Inline(codes) => codes.iter().any(|known| known == code),
            Codes::Set(name) => unreachable!("language set {name} is resolved at load"),
        }
    }
}

impl Languages {
    fn allows(&self, source: &str, target: &str) -> bool {
        let within = |codes: &Option<Codes>, code: &str| {
            codes.as_ref().is_none_or(|codes| codes.contains(code))
        };
        within(&self.source, source)
            && within(&self.target, target)
            && self
                .pairs
                .get(target)
                .is_none_or(|sources| sources.iter().any(|known| known == source))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    language_sets: HashMap<String, Vec<String>>,
    models: Vec<Translator>,
}

/// The model catalog, read from the same file as the interface so the two cannot drift.
/// - GPT Realtime Translate translates into 13 languages but recognises the 70+ listed in
///   OpenAI's realtime translation guide, by English name only. Mapped by judgement:
///   "Norwegian" is `nb` beside "Nynorsk" as `nn`; "Filipino" and "Tagalog" are one
///   language, kept once as `fil`.
/// - Soniox's lists are what `GET /v1/models` returns for the model, with its `no` and `tl`
///   written as `nb` and `fil`.
/// - Both Qwen models share Alibaba's one table of 60; 3.8 detects the source itself but
///   recognises the same set.
static TRANSLATORS: LazyLock<Vec<Translator>> = LazyLock::new(|| {
    let mut catalog: Catalog =
        serde_json::from_str(include_str!("../../src/core/translators.json"))
            .expect("src/core/translators.json is a valid catalog");
    for entry in &mut catalog.models {
        let languages = &mut entry.languages;
        for codes in [&mut languages.source, &mut languages.target]
            .into_iter()
            .flatten()
        {
            codes.resolve(&catalog.language_sets);
        }
    }
    catalog.models
});

fn translator(model: &str) -> Option<&'static Translator> {
    TRANSLATORS.iter().find(|entry| entry.id == model)
}

fn engine(id: &str) -> Option<&'static Engine> {
    ENGINES.iter().find(|entry| entry.id == id)
}

pub(crate) fn engine_of(model: &str) -> Option<&'static str> {
    translator(model).map(|entry| entry.engine.as_str())
}

pub(crate) fn is_known_engine(id: &str) -> bool {
    engine(id).is_some()
}

/// Providers in declaration order; the credential store enumerates from here rather than
/// keeping its own list.
pub(crate) fn engine_ids() -> impl Iterator<Item = &'static str> {
    ENGINES.iter().map(|entry| entry.id)
}

/// Two routes whose fingerprints match can keep their websockets across a settings change.
pub(crate) fn settings_fingerprint(id: &str, config: &AppConfig) -> String {
    engine(id).map_or_else(String::new, |entry| (entry.fingerprint)(config))
}

/// A route has no translator until a known engine with a key is chosen: an unknown engine
/// and a missing key read the same to the user, since the picker only offers keyed ones.
pub(crate) fn start_blocker(
    id: &str,
    config: &AppConfig,
    credentials: &CredentialStore,
) -> Option<&'static str> {
    let Some(entry) = engine(id) else {
        return Some("noTranslator");
    };
    (entry.blocker)(config).or_else(|| (!credentials.has(id)).then_some("noTranslator"))
}

/// Why a route cannot run as configured, if anything. The picker deliberately lets the user
/// pass through invalid combinations — otherwise some valid destinations are unreachable,
/// since only one field can change at a time — so the check lives here instead.
pub(crate) fn route_config_error(
    model: &str,
    source_language: &str,
    target_language: &str,
) -> Option<&'static str> {
    if translator(model).is_none() {
        return Some("noTranslator");
    }
    if source_language == target_language {
        return Some("invalidLanguagePair");
    }
    if !supports_pair(model, source_language, target_language) {
        return Some("unsupportedLanguage");
    }
    None
}

pub(crate) fn open_session(
    config: &AppConfig,
    route: &RouteConfig,
    credentials: &CredentialStore,
    events: Events,
    state: &ProviderState,
) -> Result<u64, String> {
    let Some(entry) = engine(&route.engine) else {
        return Err(format!("Unsupported translator: {}", route.engine));
    };
    let protocol = translator(&route.model).and_then(|model| model.protocol.as_deref());
    if !(entry.speaks)(protocol) {
        return Err(format!("Unsupported protocol for {}", route.model));
    }
    let request = StartRequest {
        config,
        route,
        api_key: credentials.get(&route.engine),
        protocol,
    };
    (entry.open)(request, events, state)
}

/// The model a route falls back to when its stored one has been retired.
pub(crate) fn default_model(engine: &str) -> &'static str {
    TRANSLATORS
        .iter()
        .find(|entry| entry.engine == engine && entry.default)
        .map_or("", |entry| entry.id.as_str())
}

#[cfg(test)]
pub(crate) fn model_ids() -> impl Iterator<Item = &'static str> {
    TRANSLATORS.iter().map(|entry| entry.id.as_str())
}

fn supports_pair(model: &str, source: &str, target: &str) -> bool {
    translator(model).is_some_and(|entry| entry.languages.allows(source, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_unique_and_dispatchable() {
        for entry in TRANSLATORS.iter() {
            assert!(is_known_engine(&entry.engine), "{}", entry.id);
            // A duplicate id would resolve to the earlier entry instead of this one.
            assert_eq!(engine_of(&entry.id), Some(entry.engine.as_str()));
        }
    }

    /// An engine with no default would leave a retired route with nothing to dial, and two
    /// would make the fallback depend on file order.
    #[test]
    fn every_engine_has_exactly_one_default() {
        for engine in ENGINES {
            let defaults = TRANSLATORS
                .iter()
                .filter(|entry| entry.engine == engine.id && entry.default)
                .count();
            assert_eq!(defaults, 1, "{}", engine.id);
        }
    }

    /// A model whose protocol its provider cannot speak would fail only when dialled.
    #[test]
    fn every_model_names_a_protocol_its_engine_speaks() {
        for entry in TRANSLATORS.iter() {
            let engine = engine(&entry.engine).unwrap();
            assert!((engine.speaks)(entry.protocol.as_deref()), "{}", entry.id);
        }
    }

    /// Distinct settings must not collide into one fingerprint, or a route would keep a
    /// websocket opened with the wrong workspace.
    #[test]
    fn fingerprint_separates_adjacent_fields() {
        let with = |region: &str, workspace: &str| {
            let mut config = AppConfig::default();
            config.qwen.region = region.into();
            config.qwen.workspace_id = workspace.into();
            settings_fingerprint("qwen", &config)
        };
        assert_ne!(with("beijing", "ws"), with("beijingws", ""));
    }

    #[test]
    fn start_is_blocked_until_the_engine_is_usable() {
        let mut config = AppConfig::default();
        let mut credentials = CredentialStore::default();
        assert_eq!(
            start_blocker("qwen", &config, &credentials),
            Some("workspaceRequired")
        );

        config.qwen.workspace_id = "ws-123".into();
        assert_eq!(
            start_blocker("qwen", &config, &credentials),
            Some("noTranslator")
        );

        credentials.set_for_test("qwen", "sk-test");
        assert_eq!(start_blocker("qwen", &config, &credentials), None);
        // Qwen's workspace requirement must not leak onto other engines.
        assert_eq!(
            start_blocker("openai", &AppConfig::default(), &credentials),
            Some("noTranslator")
        );
    }

    #[test]
    fn an_unknown_engine_can_never_start() {
        let credentials = CredentialStore::default();
        assert!(start_blocker("gemini", &AppConfig::default(), &credentials).is_some());
    }

    /// The mapped id becomes part of the hostname, so a typo here is a connection failure.
    #[test]
    fn regions_map_to_their_alibaba_cloud_ids() {
        assert_eq!(qwen_region("beijing"), Some("cn-beijing"));
        assert_eq!(qwen_region("singapore"), Some("ap-southeast-1"));
    }

    /// The picker lets a half-edited route through, so this is the only gate before a dial.
    #[test]
    fn a_route_is_invalid_until_the_whole_combination_works() {
        let soniox = default_model("soniox");
        let qwen = default_model("qwen");
        let openai = default_model("openai");
        let unsupported = Some("unsupportedLanguage");

        assert_eq!(
            route_config_error(soniox, "en", "en"),
            Some("invalidLanguagePair")
        );
        // Cantonese is in the interface catalog but outside Soniox's ISO 639-1 set. Either
        // end of the pair being unusable is enough to hold the route back.
        assert_eq!(route_config_error(soniox, "yue", "en"), unsupported);
        assert_eq!(route_config_error(soniox, "en", "yue"), unsupported);
        assert_eq!(route_config_error(soniox, "en", "zh"), None);
        // OpenAI hears far more languages than it speaks, so each end is held to its own set.
        assert_eq!(route_config_error(openai, "en", "nl"), unsupported);
        assert_eq!(route_config_error(openai, "nl", "en"), None);
        // Qwen takes the whole catalog, so the pair Soniox refused stays valid there.
        assert_eq!(route_config_error(qwen, "yue", "en"), None);
    }

    /// A model nobody recognises must not fall through to the "accepts any language" case,
    /// and a route that never got one is held back the same way.
    #[test]
    fn a_route_needs_a_known_model() {
        let retired = "qwen3-livetranslate-flash-realtime";
        assert_eq!(engine_of(retired), None);
        assert_eq!(
            route_config_error(retired, "en", "zh"),
            Some("noTranslator")
        );
        assert_eq!(route_config_error("", "en", "zh"), Some("noTranslator"));
    }

    #[test]
    fn narrow_models_reject_unsupported_languages() {
        let openai = default_model("openai");
        assert!(supports_pair(openai, "en", "ja"));
        assert!(!supports_pair(openai, "en", "nl"));
        assert!(supports_pair(default_model("qwen"), "en", "nl"));
    }

    /// The interface runs the same cases, so its labels and this gate cannot disagree.
    #[test]
    fn language_rules_match_the_shared_cases() {
        #[derive(Deserialize)]
        struct Rule {
            about: String,
            languages: Languages,
            cases: Vec<(String, String, bool)>,
        }
        let rules: Vec<Rule> =
            serde_json::from_str(include_str!("../../tests/fixtures/language-rules.json")).unwrap();
        for rule in rules {
            for (source, target, allowed) in rule.cases {
                let shown = format!("{}: {source} -> {target}", rule.about);
                assert_eq!(rule.languages.allows(&source, &target), allowed, "{shown}");
            }
        }
    }
}
