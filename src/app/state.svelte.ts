import { DEFAULT_CONFIG } from './defaults';
import { readableRuntimeError } from '../core/errors.js';
import { currentLocale, setLocale, t } from '../core/locale.svelte';
import { languageName } from '../core/languages.js';
import {
    getAutostartEnabled,
    invoke,
    sendControllerAction,
    setAutostartEnabled,
    subscribeController,
} from '../core/runtime.js';
import type {
    AppConfig,
    ControllerSnapshot,
    Credentials,
    MainView,
    RouteConfig,
    RouteId,
    RouteRuntimeState,
    SessionDetail,
    SessionExportMode,
    SessionSummary,
    TranscriptDraft,
    TranscriptTurn,
    TranslationState,
} from './types';

const emptyDrafts = (): Record<RouteId, TranscriptDraft> => ({
    system: { original: '', translation: '' },
    microphone: { original: '', translation: '' },
});

const emptyRouteStates = (): Record<RouteId, RouteRuntimeState> => ({
    system: { state: 'stopped', error: '' },
    microphone: { state: 'stopped', error: '' },
});

export class AppState {
    config = $state<AppConfig>(structuredClone(DEFAULT_CONFIG) as AppConfig);
    credentials = $state<Credentials>({});
    loading = $state(true);
    autostartEnabled = $state(false);
    autostartLoading = $state(false);
    view = $state<MainView>('translation');
    translationState = $state<TranslationState>('stopped');
    routeStates = $state<Record<RouteId, RouteRuntimeState>>(emptyRouteStates());
    turns = $state<Record<RouteId, TranscriptTurn[]>>({ system: [], microphone: [] });
    drafts = $state<Record<RouteId, TranscriptDraft>>(emptyDrafts());
    overlayVisible = $state(false);
    elapsedSeconds = $state(0);
    toastMessage = $state('');
    toastIsError = $state(false);
    history = $state<SessionSummary[]>([]);
    historyLoading = $state(false);
    selectedHistory = $state<SessionSummary | null>(null);
    selectedHistoryDetail = $state<SessionDetail | null>(null);

    private toastTimer: ReturnType<typeof setTimeout> | null = null;
    private qwenTimer: ReturnType<typeof setTimeout> | null = null;
    private unsubscribeController: (() => void) | null = null;
    private lastNoticeId = 0;

    get translationActive() {
        return ['starting', 'running', 'paused'].includes(this.translationState);
    }

    get resolvedLocale() {
        return currentLocale();
    }

    get historySegments() {
        return this.selectedHistoryDetail?.json.chunks.flatMap(chunk => chunk.segments) ?? [];
    }

    async init() {
        try {
            const [credentials, autostart, unsubscribe] = await Promise.all([
                invoke('get_credential_status') as Promise<Credentials>,
                getAutostartEnabled().catch(() => false),
                subscribeController(
                    'main',
                    (snapshot: ControllerSnapshot) => this.applyControllerSnapshot(snapshot),
                ),
            ]);
            this.credentials = credentials;
            this.autostartEnabled = autostart;
            this.unsubscribeController = unsubscribe;
        } catch (error) {
            this.notify(this.readableError(error), true);
        } finally {
            this.loading = false;
        }
    }

    destroy() {
        this.unsubscribeController?.();
        if (this.toastTimer) clearTimeout(this.toastTimer);
        if (this.qwenTimer) clearTimeout(this.qwenTimer);
    }

    text(key: string) {
        return t(key);
    }

    language(code: string) {
        return languageName(code, this.resolvedLocale);
    }

    routeName(routeId?: string) {
        return this.text(routeId === 'microphone' ? 'microphone' : 'systemAudio');
    }

    async setView(view: MainView) {
        this.view = view;
        if (view === 'history') await this.loadHistory();
    }

    async startTranslation() {
        await sendControllerAction({ type: 'translation', command: 'start' });
    }

    async togglePause() {
        const command = this.translationState === 'paused' ? 'resume' : 'pause';
        await sendControllerAction({ type: 'translation', command });
    }

    async stopTranslation() {
        await sendControllerAction({ type: 'translation', command: 'stop' });
    }

    async toggleOverlay() {
        await sendControllerAction({ type: 'toggleOverlay' });
    }

    updateLocale(value: AppConfig['locale']) {
        this.config.locale = value;
        setLocale(value);
        document.documentElement.lang = this.resolvedLocale;
        this.dispatch({ type: 'locale', locale: value });
    }

    async updateAutostart(enabled: boolean) {
        if (this.autostartLoading) return;
        const previous = this.autostartEnabled;
        this.autostartEnabled = enabled;
        this.autostartLoading = true;
        try {
            await setAutostartEnabled(enabled);
        } catch (error) {
            this.autostartEnabled = previous;
            this.notify(this.readableError(error), true);
        } finally {
            this.autostartLoading = false;
        }
    }

    updateQwen<Field extends keyof AppConfig['qwen']>(
        field: Field,
        value: AppConfig['qwen'][Field],
    ) {
        if (this.translationActive) return;
        this.config.qwen[field] = value;
        if (this.qwenTimer) clearTimeout(this.qwenTimer);
        this.qwenTimer = setTimeout(() => {
            this.dispatch({
                type: 'qwenSettings',
                patch: { ...this.config.qwen },
            });
        }, 250);
    }

    async saveCredential(provider: string, secret: string) {
        if (this.translationActive || !secret.trim()) return false;
        try {
            this.credentials = await invoke('set_provider_credential', {
                provider,
                secret,
            }) as Credentials;
            this.notify(this.text('keySaved'));
            return true;
        } catch (error) {
            this.notify(this.readableError(error), true);
            return false;
        }
    }

    async clearCredential(provider: string) {
        if (this.translationActive) return false;
        try {
            this.credentials = await invoke('set_provider_credential', {
                provider,
                secret: '',
            }) as Credentials;
            return true;
        } catch (error) {
            this.notify(this.readableError(error), true);
            return false;
        }
    }

    async loadHistory() {
        this.historyLoading = true;
        try {
            this.history = await invoke('list_sessions') as SessionSummary[];
            if (
                this.selectedHistory
                && !this.history.some(session => session.id === this.selectedHistory?.id)
            ) {
                this.selectedHistory = null;
                this.selectedHistoryDetail = null;
            }
        } catch (error) {
            this.notify(this.readableError(error), true);
        } finally {
            this.historyLoading = false;
        }
    }

    async selectHistory(summary: SessionSummary) {
        this.selectedHistory = summary;
        this.selectedHistoryDetail = null;
        try {
            this.selectedHistoryDetail = await invoke('read_session', {
                id: summary.id,
            }) as SessionDetail;
        } catch (error) {
            this.notify(this.readableError(error), true);
        }
    }

    async renameHistory(title: string) {
        const target = this.selectedHistory;
        const next = title.trim();
        if (!target || !next || next === target.title) return;
        try {
            await invoke('rename_session', { id: target.id, title: next });
            const renamed = { ...target, title: next };
            this.history = this.history.map(
                session => session.id === target.id ? renamed : session,
            );
            this.selectedHistory = renamed;
        } catch {
            this.notify(this.text('renameFailed'), true);
        }
    }

    async exportHistory(mode: SessionExportMode) {
        if (!this.selectedHistory) return;
        try {
            const saved = await invoke('export_session', {
                id: this.selectedHistory.id,
                mode,
                labels: {
                    system: this.routeName('system'),
                    microphone: this.routeName('microphone'),
                },
            }) as boolean;
            if (saved) this.notify(this.text('exportSaved'));
        } catch {
            this.notify(this.text('exportFailed'), true);
        }
    }

    async deleteHistory() {
        if (!this.selectedHistory || !window.confirm(this.text('confirmDelete'))) return;
        try {
            await invoke('delete_session', { id: this.selectedHistory.id });
            this.selectedHistory = null;
            this.selectedHistoryDetail = null;
            await this.loadHistory();
        } catch {
            this.notify(this.text('deleteFailed'), true);
        }
    }

    notify(message: string, error = false) {
        if (!message) return;
        if (this.toastTimer) clearTimeout(this.toastTimer);
        this.toastMessage = message;
        this.toastIsError = error;
        this.toastTimer = setTimeout(() => {
            this.toastMessage = '';
        }, 3200);
    }

    private applyControllerSnapshot(snapshot: ControllerSnapshot) {
        this.translationState = snapshot.translationState;
        this.elapsedSeconds = snapshot.elapsedSeconds;
        this.overlayVisible = snapshot.overlayVisible;
        this.config.overlay = structuredClone(snapshot.config);
        this.config.audio = structuredClone(snapshot.audio);
        this.config.locale = snapshot.preferredLocale;
        this.config.qwen = structuredClone(snapshot.qwen);
        for (const routeId of ['system', 'microphone'] as RouteId[]) {
            const route = snapshot.routes[routeId];
            this.config.routes[routeId] = structuredClone(route.config);
            this.routeStates[routeId] = { state: route.state, error: route.error };
            this.turns[routeId] = route.turns.map(turn => ({ ...turn }));
            this.drafts[routeId] = { ...route.draft };
        }
        setLocale(snapshot.locale);
        const notice = snapshot.notice;
        if (notice && notice.id !== this.lastNoticeId) {
            this.lastNoticeId = notice.id;
            const message = notice.code ? this.text(notice.code) : this.readableError(notice.message);
            this.notify(message, true);
        }
    }

    private dispatch(action: Parameters<typeof sendControllerAction>[0]) {
        void sendControllerAction(action)
            .catch(error => this.notify(this.readableError(error), true));
    }

    private readableError(error: unknown) {
        return readableRuntimeError(error, (key: string) => this.text(key));
    }
}
