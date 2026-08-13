// The 13 languages GPT Realtime Translate can translate *into*. Its input side accepts
// 70+, including Arabic, but a route needs both ends, so the narrower set governs.
const OPENAI_LANGUAGES = new Set([
    'zh', 'en', 'fr', 'de', 'hi', 'id',
    'it', 'ja', 'ko', 'pt', 'ru', 'es', 'vi',
]);

// Soniox transcribes and translates across one shared ISO 639-1 set, so the catalog's
// non-639-1 codes (`yue` and friends) are absent. Mirrors SONIOX_LANGUAGES in translators.rs.
const SONIOX_LANGUAGES = new Set([
    'af', 'ar', 'az', 'be', 'bg', 'bn', 'bs', 'ca', 'cs', 'da', 'de', 'el', 'en', 'es', 'et', 'fa',
    'fi', 'fr', 'gl', 'gu', 'he', 'hi', 'hr', 'hu', 'id', 'it', 'ja', 'jv', 'kk', 'kn', 'ko', 'ky',
    'lv', 'mk', 'ml', 'mr', 'ms', 'nl', 'pa', 'pl', 'pt', 'ro', 'ru', 'sk', 'sl', 'sv', 'sw', 'ta',
    'te', 'tg', 'th', 'tl', 'tr', 'uk', 'ur', 'vi', 'zh',
]);

// One entry per model. A provider with several models contributes several entries, so the
// translator picker is a flat list and never has to nest a second dropdown. `name` spells
// out the product and version exactly as the provider publishes it, and `provider` names
// who owns the API key.
const TRANSLATORS = [
    {
        id: 'qwen3.5-livetranslate-flash-realtime',
        provider: 'qwen',
        name: 'Qwen3.5 LiveTranslate Flash Realtime',
        languages: null,
    },
    {
        id: 'gpt-realtime-translate',
        provider: 'openai',
        name: 'GPT Realtime Translate',
        languages: OPENAI_LANGUAGES,
    },
    {
        id: 'stt-rt-v5',
        provider: 'soniox',
        name: 'Soniox Real-time STT v5',
        languages: SONIOX_LANGUAGES,
    },
];

// Providers in name order, derived so the list can never drift from the models.
export function providers() {
    return [...new Set(translators().map(entry => entry.provider))];
}

export function translators() {
    return [...TRANSLATORS].sort((left, right) => left.name.localeCompare(right.name));
}

// Where a user goes when they are stuck on a field. Keyed by provider because credentials
// belong to the provider, not the model. Alibaba Cloud publishes separate Chinese and
// English sites; the others are English only.
const DOCS = {
    qwen: {
        languages: {
            'zh-Hans': 'https://help.aliyun.com/zh/model-studio/qwen3-5-livetranslate-flash-realtime#24e640a87fsrw',
            default: 'https://www.alibabacloud.com/help/en/model-studio/qwen3-5-livetranslate-flash-realtime#24e640a87fsrw',
        },
        apiKey: {
            'zh-Hans': 'https://help.aliyun.com/zh/model-studio/get-api-key',
            default: 'https://www.alibabacloud.com/help/en/model-studio/get-api-key',
        },
        workspaceId: {
            'zh-Hans': 'https://help.aliyun.com/zh/model-studio/obtain-the-app-id-and-workspace-id',
            default: 'https://www.alibabacloud.com/help/en/model-studio/obtain-the-app-id-and-workspace-id',
        },
    },
    openai: {
        languages: {
            default: 'https://developers.openai.com/cookbook/examples/voice_solutions/realtime_translation_guide#supported-languages',
        },
        // The key dashboard is behind a login wall; the quickstart explains how to get there.
        apiKey: { default: 'https://developers.openai.com/api/docs/quickstart' },
    },
    soniox: {
        languages: { default: 'https://soniox.com/docs/stt/concepts/supported-languages' },
        apiKey: { default: 'https://soniox.com/docs/stt/get-started' },
    },
};

export function docsUrl(provider, topic, locale) {
    const entry = DOCS[provider]?.[topic];
    return entry ? entry[locale] ?? entry.default : '';
}

export function translator(id) {
    return TRANSLATORS.find(entry => entry.id === id);
}

export function supportsLanguage(id, code) {
    const supported = translator(id)?.languages;
    return !supported || supported.has(code);
}

// Nothing is ever hidden or blocked: the user has to be able to change translator and
// language in either order, and only the resulting combination has to be valid.
export function routeConfigError(model, sourceLanguage, targetLanguage) {
    if (sourceLanguage === targetLanguage) return 'invalidLanguagePair';
    if (!supportsLanguage(model, sourceLanguage) || !supportsLanguage(model, targetLanguage)) {
        return 'unsupportedLanguage';
    }
    return '';
}
