export { LANGUAGES } from './translators.js';

const names = new Map();

export function languageName(code, locale = 'en') {
    try {
        if (!names.has(locale)) names.set(locale, new Intl.DisplayNames([locale], { type: 'language' }));
        return names.get(locale).of(code);
    } catch {
        return code;
    }
}

export function translationDirection(source, target, fallbackLocale = 'en') {
    const locale = target === 'zh'
        ? 'zh-Hans'
        : ['en', 'es', 'ja', 'ko', 'vi'].includes(target)
            ? target
            : fallbackLocale;
    return `${languageName(source, locale)} → ${languageName(target, locale)}`;
}
