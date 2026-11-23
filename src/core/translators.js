import CATALOG from './translators.json' with { type: 'json' };

// One entry per model, shared with the backend (notes on the data sit beside its loader in
// translators.rs). A provider with several models contributes several entries, so the
// translator picker is a flat list and never has to nest a second dropdown. `name` spells
// out the product and version exactly as the provider publishes it, and `provider` names
// who owns the API key.

// A list is written inline or as the name of a set several models share; null is open.
const resolve = codes => typeof codes === 'string' ? CATALOG.languageSets[codes] : codes;

const TRANSLATORS = CATALOG.models.map(entry => ({
    ...entry,
    languages: {
        ...entry.languages,
        source: resolve(entry.languages.source),
        target: resolve(entry.languages.target),
    },
}));

// Every language some model can hear or produce, so a model brings its languages with it.
export const LANGUAGES = [...new Set(TRANSLATORS.flatMap(entry => [
    ...entry.languages.source ?? [],
    ...entry.languages.target ?? [],
]))];

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

// Mirrors default_model() in translators.rs.
export function defaultModel(provider) {
    return TRANSLATORS.find(entry => entry.provider === provider && entry.default)?.id ?? '';
}

export function translator(id) {
    return TRANSLATORS.find(entry => entry.id === id);
}

// Whether one end of a route, 'source' or 'target', can be `code` on this model. A route
// with no model yet restricts nothing, so its language pickers still offer everything.
export function accepts(id, field, code) {
    const supported = translator(id)?.languages[field];
    return !supported || supported.includes(code);
}

// Mirrors Languages::allows in translators.rs; both run tests/fixtures/language-rules.json.
export function allows(languages, source, target) {
    const within = (codes, code) => !codes || codes.includes(code);
    const sources = languages.pairs?.[target];
    return within(languages.source, source)
        && within(languages.target, target)
        && within(sources, source);
}

export function supportsPair(id, source, target) {
    const languages = translator(id)?.languages;
    return !languages || allows(languages, source, target);
}
