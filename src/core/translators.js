import TRANSLATORS from './translators.json' with { type: 'json' };

// One entry per model, shared with the backend (notes on the data sit beside its loader in
// translators.rs). A provider with several models contributes several entries, so the
// translator picker is a flat list and never has to nest a second dropdown. `name` spells
// out the product and version exactly as the provider publishes it, and `provider` names
// who owns the API key.

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

// Mirrors default_model() in translators.rs: a provider's first entry.
export function defaultModel(provider) {
    return TRANSLATORS.find(entry => entry.provider === provider)?.id ?? '';
}

export function translator(id) {
    return TRANSLATORS.find(entry => entry.id === id);
}

export function supportsLanguage(id, code) {
    const supported = translator(id)?.languages;
    return !supported || supported.includes(code);
}
