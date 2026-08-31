import type { RouteId, TranscriptDraft, TranscriptTurn } from './types';

export interface TimelineEntry {
    routeId: RouteId;
    original: string;
    translation: string;
    draft: boolean;
}

type RouteTranscript = { turns: TranscriptTurn[]; draft: TranscriptDraft };

function epoch(timestamp: TranscriptTurn['timestamp']) {
    const value = typeof timestamp === 'number' ? timestamp : Date.parse(timestamp);
    return Number.isFinite(value) ? value : 0;
}

// Interleaves every route's finished turns by time, then appends the live drafts so the
// text still being spoken always sits at the bottom. Ties keep `order` so the result is
// stable between snapshots.
export function mergeTimeline(
    routes: Partial<Record<RouteId, RouteTranscript>>,
    order: RouteId[],
): TimelineEntry[] {
    const turns = order.flatMap((routeId, rank) =>
        (routes[routeId]?.turns ?? []).map((turn, index) => ({
            key: [epoch(turn.timestamp), rank, index],
            entry: { routeId, original: turn.original, translation: turn.translation, draft: false },
        })));
    turns.sort((a, b) => a.key[0] - b.key[0] || a.key[1] - b.key[1] || a.key[2] - b.key[2]);
    const drafts = order.flatMap((routeId): TimelineEntry[] => {
        const draft = routes[routeId]?.draft;
        return draft && (draft.original || draft.translation)
            ? [{ routeId, original: draft.original, translation: draft.translation, draft: true }]
            : [];
    });
    return [...turns.map(item => item.entry), ...drafts];
}
