import { t } from '../core/locale.svelte';
import { invoke } from '../core/runtime.js';
import type { RouteId, SessionDetail, SessionExportMode, SessionSummary } from './types';

export interface HistoryDeps {
    notify: (message: string, error?: boolean) => void;
    readableError: (error: unknown) => string;
    routeName: (routeId: RouteId) => string;
}

export class HistoryState {
    items = $state<SessionSummary[]>([]);
    loading = $state(false);
    selected = $state<SessionSummary | null>(null);
    detail = $state<SessionDetail | null>(null);

    private readonly deps: HistoryDeps;

    constructor(deps: HistoryDeps) {
        this.deps = deps;
    }

    get segments() {
        return this.detail?.json.chunks.flatMap(chunk => chunk.segments) ?? [];
    }

    async load() {
        this.loading = true;
        try {
            this.items = await invoke('list_sessions') as SessionSummary[];
            if (
                this.selected
                && !this.items.some(session => session.id === this.selected?.id)
            ) {
                this.selected = null;
                this.detail = null;
            }
        } catch (error) {
            this.deps.notify(this.deps.readableError(error), true);
        } finally {
            this.loading = false;
        }
    }

    async select(summary: SessionSummary) {
        this.selected = summary;
        this.detail = null;
        try {
            this.detail = await invoke('read_session', {
                id: summary.id,
            }) as SessionDetail;
        } catch (error) {
            this.deps.notify(this.deps.readableError(error), true);
        }
    }

    async rename(title: string) {
        const target = this.selected;
        const next = title.trim();
        if (!target || !next || next === target.title) return;
        try {
            await invoke('rename_session', { id: target.id, title: next });
            const renamed = { ...target, title: next };
            this.items = this.items.map(
                session => session.id === target.id ? renamed : session,
            );
            this.selected = renamed;
        } catch {
            this.deps.notify(t('renameFailed'), true);
        }
    }

    async export(mode: SessionExportMode) {
        if (!this.selected) return;
        try {
            const saved = await invoke('export_session', {
                id: this.selected.id,
                mode,
                labels: {
                    system: this.deps.routeName('system'),
                    microphone: this.deps.routeName('microphone'),
                },
            }) as boolean;
            if (saved) this.deps.notify(t('exportSaved'));
        } catch {
            this.deps.notify(t('exportFailed'), true);
        }
    }

    async delete() {
        if (!this.selected || !window.confirm(t('confirmDelete'))) return;
        try {
            await invoke('delete_session', { id: this.selected.id });
            this.selected = null;
            this.detail = null;
            await this.load();
        } catch {
            this.deps.notify(t('deleteFailed'), true);
        }
    }
}
