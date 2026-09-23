import { locale } from '../core/i18n.js';
import type { AppConfig, UpdateStatus } from './types';

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
        microphone: {
            device: null,
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
        frame: null,
    },
    qwen: {
        region: 'beijing',
        workspaceId: '',
    },
};

// Nothing known yet; the backend replaces it as soon as it announces its own state.
export const DEFAULT_UPDATE_STATUS: UpdateStatus = {
    stage: 'idle',
    currentVersion: '',
    version: null,
    progress: null,
    error: null,
    autoCheck: true,
    installable: true,
};
