import assert from 'node:assert/strict';
import test from 'node:test';
import { readableRuntimeError } from '../src/core/errors.js';

const text = key => key;

test('a refused audio capture reads as a permission problem, not a raw status code', () => {
    const result = readableRuntimeError(
        new Error('Failed to create audio tap: Core Audio error 560947818'),
        text,
    );
    assert.equal(result, 'audioPermissionRequired');
});

test('classifies device and network failures', () => {
    assert.equal(readableRuntimeError(new Error('No microphone input device'), text), 'microphoneUnavailable');
    assert.equal(
        readableRuntimeError(new Error('Selected application is not running'), text),
        'captureApplicationUnavailable',
    );
    assert.equal(readableRuntimeError(new Error('websocket connect timed out'), text), 'networkUnavailable');
});

test('what the translator said is shown as sent', () => {
    const sent = '401 Unauthorized InvalidApiKey: Invalid API-key provided.';
    assert.equal(readableRuntimeError(new Error(sent), text), sent);
    assert.equal(readableRuntimeError(undefined, text), 'unknownError');
});
