import en from './locales/en.js';
import es from './locales/es.js';
import zhHans from './locales/zh-Hans.js';
import ja from './locales/ja.js';
import ko from './locales/ko.js';
import vi from './locales/vi.js';

export const messages = { en, es, 'zh-Hans': zhHans, ja, ko, vi };

let currentLocale = detectLocale();

export function setLocale(value) {
    currentLocale = resolveLocale(value);
    if (typeof document !== 'undefined') document.documentElement.lang = currentLocale;
}

export function locale() {
    return currentLocale;
}

export function t(key) {
    return messages[currentLocale]?.[key] || messages.en[key] || key;
}

function detectLocale() {
    const preferred = typeof navigator === 'undefined'
        ? 'en'
        : navigator.languages?.[0] || navigator.language || 'en';
    if (preferred.toLowerCase().startsWith('zh')) return 'zh-Hans';
    if (preferred.toLowerCase().startsWith('es')) return 'es';
    if (preferred.toLowerCase().startsWith('ja')) return 'ja';
    if (preferred.toLowerCase().startsWith('ko')) return 'ko';
    if (preferred.toLowerCase().startsWith('vi')) return 'vi';
    return 'en';
}

function resolveLocale(value) {
    if (value === 'auto') return detectLocale();
    return messages[value] ? value : 'en';
}
