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

test('classifies provider and device failures', () => {
    assert.equal(readableRuntimeError(new Error('HTTP 401 unauthorized'), text), 'credentialRejected');
    assert.equal(readableRuntimeError(new Error('Workspace access denied'), text), 'workspaceRejected');
    assert.equal(readableRuntimeError(new Error('No microphone input device'), text), 'microphoneUnavailable');
    assert.equal(
        readableRuntimeError(new Error('Selected application is not running'), text),
        'captureApplicationUnavailable',
    );
    assert.equal(readableRuntimeError(new Error('websocket connect timed out'), text), 'networkUnavailable');
});
