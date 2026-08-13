import { locale, setLocale as applyLocale, t as translate } from './i18n.js';

let revision = $state(0);

export function setLocale(value: string) {
    applyLocale(value);
    revision += 1;
}

export function currentLocale() {
    void revision;
    return locale();
}

export function t(key: string) {
    void revision;
    return translate(key);
}
