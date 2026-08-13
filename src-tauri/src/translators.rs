use crate::app_config::{AppConfig, RouteConfig};
use crate::commands::openai_realtime::{self, OpenAiRealtimeConfig};
use crate::commands::qwen_realtime::{self, QwenRealtimeConfig};
use crate::commands::realtime::{Events, ProviderState};
use crate::commands::soniox_realtime::{self, SonioxRealtimeConfig};
use crate::credentials::CredentialStore;

/// Everything one route needs to dial its translator. `api_key` is resolved from the
/// store by engine id, so no provider names itself twice.
struct StartRequest<'a> {
    config: &'a AppConfig,
    route: &'a RouteConfig,
    route_id: &'a str,
    api_key: &'a str,
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
                    route_id: request.route_id.into(),
                },
                events,
                state,
            )
        },
    },
    Engine {
        id: "soniox",
        fingerprint: |_| String::new(),
        blocker: |_| None,
        open: |request, events, state| {
            soniox_realtime::start_session(
                SonioxRealtimeConfig {
                    api_key: request.api_key.to_owned(),
                    source_language: request.route.source_language.clone(),
                    target_language: request.route.target_language.clone(),
                },
                events,
                state,
            )
        },
    },
    Engine {
        id: "openai",
        fingerprint: |_| String::new(),
        blocker: |_| None,
        open: |request, events, state| {
            openai_realtime::start_session(
                OpenAiRealtimeConfig {
                    api_key: request.api_key.to_owned(),
                    target_language: request.route.target_language.clone(),
                },
                events,
                state,
            )
        },
    },
];

/// A single translator model. `engine` names the provider that owns its API key; a provider
/// with several models contributes several entries, so nothing downstream unpacks a nested list.
struct Translator {
    id: &'static str,
    engine: &'static str,
    /// `None` when the model accepts any language pair.
    languages: Option<&'static [&'static str]>,
}

/// The 13 languages GPT Realtime Translate can translate *into*. Its input side accepts
/// 70+, including Arabic, but a route needs both ends, so the narrower set governs.
const OPENAI_LANGUAGES: &[&str] = &[
    "zh", "en", "fr", "de", "hi", "id", "it", "ja", "ko", "pt", "ru", "es", "vi",
];

/// Soniox transcribes and translates across one shared ISO 639-1 set. Codes outside it —
/// `yue` and the other non-639-1 entries in the interface catalog — are rejected upstream.
const SONIOX_LANGUAGES: &[&str] = &[
    "af", "ar", "az", "be", "bg", "bn", "bs", "ca", "cs", "da", "de", "el", "en", "es", "et", "fa",
    "fi", "fr", "gl", "gu", "he", "hi", "hr", "hu", "id", "it", "ja", "jv", "kk", "kn", "ko", "ky",
    "lv", "mk", "ml", "mr", "ms", "nl", "pa", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "sw", "ta",
    "te", "tg", "th", "tl", "tr", "uk", "ur", "vi", "zh",
];

const TRANSLATORS: &[Translator] = &[
    Translator {
        id: "qwen3.5-livetranslate-flash-realtime",
        engine: "qwen",
        languages: None,
    },
    Translator {
        id: "stt-rt-v5",
        engine: "soniox",
        languages: Some(SONIOX_LANGUAGES),
    },
    Translator {
        id: "gpt-realtime-translate",
        engine: "openai",
        languages: Some(OPENAI_LANGUAGES),
    },
];

fn translator(model: &str) -> Option<&'static Translator> {
    TRANSLATORS.iter().find(|entry| entry.id == model)
}

fn engine(id: &str) -> Option<&'static Engine> {
    ENGINES.iter().find(|entry| entry.id == id)
}

pub(crate) fn engine_of(model: &str) -> Option<&'static str> {
    translator(model).map(|entry| entry.engine)
}

pub(crate) fn default_engine() -> &'static str {
    ENGINES[0].id
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

/// An unknown engine is treated as unusable rather than allowed through.
pub(crate) fn start_blocker(
    id: &str,
    config: &AppConfig,
    credentials: &CredentialStore,
) -> Option<&'static str> {
    let Some(entry) = engine(id) else {
        return Some("missingCredential");
    };
    (entry.blocker)(config).or_else(|| (!credentials.has(id)).then_some("missingCredential"))
}

/// Why a route cannot run as configured, if anything. The picker deliberately lets the user
/// pass through invalid combinations — otherwise some valid destinations are unreachable,
/// since only one field can change at a time — so the check lives here instead.
pub(crate) fn route_config_error(
    model: &str,
    source_language: &str,
    target_language: &str,
) -> Option<&'static str> {
    if source_language == target_language {
        return Some("invalidLanguagePair");
    }
    if !supports_language(model, source_language) || !supports_language(model, target_language) {
        return Some("unsupportedLanguage");
    }
    None
}

pub(crate) fn open_session(
    config: &AppConfig,
    route: &RouteConfig,
    route_id: &str,
    credentials: &CredentialStore,
    events: Events,
    state: &ProviderState,
) -> Result<u64, String> {
    let Some(entry) = engine(&route.engine) else {
        return Err(format!("Unsupported translator: {}", route.engine));
    };
    let request = StartRequest {
        config,
        route,
        route_id,
        api_key: credentials.get(&route.engine),
    };
    (entry.open)(request, events, state)
}

/// The model a route falls back to when its stored one has been retired.
pub(crate) fn default_model(engine: &str) -> &'static str {
    TRANSLATORS
        .iter()
        .find(|entry| entry.engine == engine)
        .map_or("", |entry| entry.id)
}

fn supports_language(model: &str, language: &str) -> bool {
    translator(model).is_some_and(|entry| match entry.languages {
        None => true,
        Some(languages) => languages.contains(&language),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_unique_and_dispatchable() {
        for entry in TRANSLATORS {
            assert!(is_known_engine(entry.engine), "{}", entry.id);
            // A duplicate id would resolve to the earlier entry instead of this one.
            assert_eq!(engine_of(entry.id), Some(entry.engine));
        }
    }

    /// An engine with no model would leave routes that select it with nothing to dial.
    #[test]
    fn every_engine_offers_a_model() {
        for entry in ENGINES {
            assert!(!default_model(entry.id).is_empty(), "{}", entry.id);
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
            Some("missingCredential")
        );

        credentials.set_for_test("qwen", "sk-test");
        assert_eq!(start_blocker("qwen", &config, &credentials), None);
        // Qwen's workspace requirement must not leak onto other engines.
        assert_eq!(
            start_blocker("openai", &AppConfig::default(), &credentials),
            Some("missingCredential")
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
        let soniox = "stt-rt-v5";
        let qwen = "qwen3.5-livetranslate-flash-realtime";
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
        assert_eq!(
            route_config_error("gpt-realtime-translate", "en", "nl"),
            unsupported
        );
        // Qwen takes the whole catalog, so the pair Soniox refused stays valid there.
        assert_eq!(route_config_error(qwen, "yue", "en"), None);
    }

    /// A model nobody recognises must not fall through to the "accepts any language" case.
    #[test]
    fn retired_models_are_unknown() {
        let retired = "qwen3-livetranslate-flash-realtime";
        assert_eq!(engine_of(retired), None);
        assert_eq!(
            route_config_error(retired, "en", "zh"),
            Some("unsupportedLanguage")
        );
    }

    #[test]
    fn narrow_models_reject_unsupported_languages() {
        assert!(supports_language("gpt-realtime-translate", "ja"));
        assert!(!supports_language("gpt-realtime-translate", "nl"));
        assert!(supports_language(
            "qwen3.5-livetranslate-flash-realtime",
            "nl"
        ));
    }
}
