import { locale } from '../core/i18n.js';
import type { AppConfig } from './types';

// Mirrors interface_language() in app_config.rs. Only the placeholder shown before the
// first controller snapshot and the browser preview's fake backend read this; the running
// app takes its defaults from Rust.
const interfaceLanguage = () => (locale() === 'zh-Hans' ? 'zh' : locale());
const counterpartLanguage = () => (interfaceLanguage() === 'en' ? 'zh' : 'en');

export const DEFAULT_CONFIG: AppConfig = {
    schemaVersion: 1,
    locale: 'auto',
    audio: {
        system: {
            scope: 'all',
            application: null,
        },
    },
    routes: {
        system: {
            enabled: true,
            input: 'system',
            engine: 'qwen',
            model: 'qwen3.5-livetranslate-flash-realtime',
            sourceLanguage: counterpartLanguage(),
            targetLanguage: interfaceLanguage(),
        },
        microphone: {
            enabled: true,
            input: 'microphone',
            engine: 'qwen',
            model: 'qwen3.5-livetranslate-flash-realtime',
            sourceLanguage: interfaceLanguage(),
            targetLanguage: counterpartLanguage(),
        },
    },
    overlay: {
        enabled: true,
        opacity: 0.75,
        fontScale: 1,
        alwaysOnTop: true,
        clickThrough: true,
        showOriginal: true,
        showTranslation: true,
        layout: 'split',
    },
    qwen: {
        region: 'beijing',
        workspaceId: '',
    },
};
