import assert from 'node:assert/strict';
import test from 'node:test';

import { mergeTimeline } from '../src/app/timeline.ts';

const turn = (routeId, original, timestamp) => ({ routeId, original, translation: `${original}!`, timestamp });
const empty = { original: '', translation: '' };

test('turns from both routes interleave by timestamp', () => {
    const merged = mergeTimeline({
        system: { turns: [turn('system', 'a', '2026-01-01T00:00:01Z'), turn('system', 'c', '2026-01-01T00:00:03Z')], draft: empty },
        microphone: { turns: [turn('microphone', 'b', '2026-01-01T00:00:02Z')], draft: empty },
    }, ['system', 'microphone']);
    assert.deepEqual(merged.map(entry => entry.original), ['a', 'b', 'c']);
    assert.deepEqual(merged.map(entry => entry.routeId), ['system', 'microphone', 'system']);
});

test('equal timestamps keep route order, numeric timestamps are accepted', () => {
    const merged = mergeTimeline({
        system: { turns: [turn('system', 's', 1000)], draft: empty },
        microphone: { turns: [turn('microphone', 'm', 1000)], draft: empty },
    }, ['microphone', 'system']);
    assert.deepEqual(merged.map(entry => entry.original), ['m', 's']);
});

test('drafts follow every finished turn regardless of time', () => {
    const merged = mergeTimeline({
        system: { turns: [turn('system', 'late', '2026-01-01T00:00:09Z')], draft: { original: 'typing', translation: '' } },
        microphone: { turns: [], draft: { original: '', translation: 'nur Übersetzung' } },
    }, ['system', 'microphone']);
    assert.deepEqual(merged.map(entry => [entry.original || entry.translation, entry.draft]), [
        ['late', false],
        ['typing', true],
        ['nur Übersetzung', true],
    ]);
});

test('a missing route contributes nothing', () => {
    assert.deepEqual(mergeTimeline({ system: { turns: [], draft: empty } }, ['system', 'microphone']), []);
});
