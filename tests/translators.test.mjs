import assert from 'node:assert/strict';
import test from 'node:test';

import RULES from './fixtures/language-rules.json' with { type: 'json' };
import { languageName } from '../src/core/languages.js';
import { LANGUAGES, allows, defaultModel, supportsPair } from '../src/core/translators.js';

test('the language list gathers every model without repeats', () => {
    assert.equal(new Set(LANGUAGES).size, LANGUAGES.length);
    // Only Soniox has Welsh and only OpenAI has Esperanto.
    assert.ok(LANGUAGES.includes('cy'));
    assert.ok(LANGUAGES.includes('eo'));
});

// A code the platform cannot name is almost always a typo in the catalog.
test('every catalog language has a name in every interface language', () => {
    for (const locale of ['en', 'es', 'zh-Hans', 'ja', 'ko', 'vi']) {
        for (const code of LANGUAGES) {
            assert.notEqual(languageName(code, locale), code, `${locale}:${code}`);
        }
    }
});

test('language rules match the shared cases', () => {
    for (const rule of RULES) {
        for (const [source, target, allowed] of rule.cases) {
            assert.equal(allows(rule.languages, source, target), allowed, `${rule.about}: ${source} -> ${target}`);
        }
    }
});

test('named sets resolve, and a route without a model restricts nothing', () => {
    assert.equal(supportsPair(defaultModel('qwen'), 'yue', 'en'), true);
    assert.equal(supportsPair(defaultModel('soniox'), 'yue', 'en'), false);
    assert.equal(supportsPair('', 'yue', 'en'), true);
});

test('Qwen defaults to 3.8', () => {
    assert.equal(defaultModel('qwen'), 'qwen3.8-livetranslate-flash-realtime');
});
