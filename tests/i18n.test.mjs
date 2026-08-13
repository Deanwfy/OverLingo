import assert from 'node:assert/strict';
import test from 'node:test';

import { messages, setLocale, t } from '../src/core/i18n.js';
import {
    languageName,
    nativeLanguageName,
    translationDirection,
} from '../src/core/languages.js';

test('all interface languages provide the complete message catalog', () => {
    const expected = Object.keys(messages.en).sort();
    for (const locale of ['es', 'zh-Hans', 'ja', 'ko', 'vi']) {
        assert.deepEqual(Object.keys(messages[locale]).sort(), expected, locale);
    }
});

test('Spanish, Japanese and Korean can be selected directly', () => {
    setLocale('es');
    assert.equal(t('launchAtLogin'), 'Abrir al iniciar sesión');
    setLocale('ja');
    assert.equal(t('launchAtLogin'), 'ログイン時に起動');
    setLocale('ko');
    assert.equal(t('launchAtLogin'), '로그인 시 실행');
});

test('language names support interface and native labels', () => {
    assert.equal(languageName('en', 'ja'), '英語');
    assert.equal(languageName('es', 'es'), 'español');
    assert.equal(languageName('ko', 'ko'), '한국어');
    assert.equal(nativeLanguageName('ja'), '日本語');
});

test('subtitle directions use the target language', () => {
    assert.equal(translationDirection('en', 'zh'), '英语 → 中文');
    assert.equal(translationDirection('zh', 'en'), 'Chinese → English');
});

test('audio and subtitle labels use product terminology', () => {
    assert.deepEqual(
        ['en', 'es', 'zh-Hans', 'ja', 'ko', 'vi'].map(locale => [
            messages[locale].systemAudio,
            messages[locale].sourceLanguage,
            messages[locale].targetLanguage,
        ]),
        [
            ['System audio', 'Original', 'Translation'],
            ['Audio del sistema', 'Original', 'Traducción'],
            ['系统音频', '原文', '译文'],
            ['システムオーディオ', '原文', '訳文'],
            ['시스템 오디오', '원문', '번역문'],
            ['Âm thanh hệ thống', 'Bản gốc', 'Bản dịch'],
        ],
    );
    for (const catalog of Object.values(messages)) {
        assert.equal('translatorsHint' in catalog, false);
        assert.equal('modelHint' in catalog, false);
    }
});
