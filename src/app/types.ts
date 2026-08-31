export type LocaleSetting = 'auto' | 'en' | 'es' | 'ja' | 'ko' | 'vi' | 'zh-Hans';
export type ResolvedLocale = Exclude<LocaleSetting, 'auto'>;
export type RouteId = 'system' | 'microphone';
export type TranslationState = 'stopped' | 'starting' | 'running' | 'paused' | 'failed';
export type RouteState = 'stopped' | 'connecting' | 'live' | 'reconfiguring' | 'reconnecting' | 'failed';
export type MainView = 'translation' | 'history' | 'general';
export type OverlayLayout = 'split' | 'merged';

export interface RouteConfig {
    enabled: boolean;
    input: 'system' | 'microphone';
    engine: 'qwen' | 'openai';
    model: string;
    sourceLanguage: string;
    targetLanguage: string;
}

export interface ApplicationReference {
    bundleId: string;
    name: string;
}

export interface CaptureCapabilities {
    applicationCapture: boolean;
}

export interface AppConfig {
    schemaVersion: number;
    locale: LocaleSetting;
    audio: {
        system: {
            scope: 'all' | 'application';
            application: ApplicationReference | null;
        };
    };
    routes: Record<RouteId, RouteConfig>;
    overlay: {
        enabled: boolean;
        opacity: number;
        fontScale: number;
        alwaysOnTop: boolean;
        clickThrough: boolean;
        showOriginal: boolean;
        showTranslation: boolean;
        layout: OverlayLayout;
    };
    qwen: {
        region: 'beijing' | 'singapore';
        workspaceId: string;
    };
}

export interface OverlaySettingsPatch {
    opacity?: number;
    fontScale?: number;
    alwaysOnTop?: boolean;
    clickThrough?: boolean;
    showOriginal?: boolean;
    showTranslation?: boolean;
    layout?: OverlayLayout;
}

export type ControllerAction =
    | { type: 'settings'; patch: OverlaySettingsPatch }
    | { type: 'route'; routeId: RouteId; enabled: boolean }
    | { type: 'routeSettings'; routeId: RouteId; patch: Partial<Pick<RouteConfig, 'model' | 'sourceLanguage' | 'targetLanguage'>> }
    | { type: 'retryRoute'; routeId: RouteId }
    | { type: 'qwenSettings'; patch: Partial<AppConfig['qwen']> }
    | { type: 'locale'; locale: LocaleSetting }
    | { type: 'capture'; bundleId: string }
    | { type: 'requestCaptureOptions' }
    | { type: 'translation'; command: 'start' | 'pause' | 'resume' | 'stop' }
    | { type: 'toggleTranslation' }
    | { type: 'toggleOverlay' }
    | { type: 'showOverlay'; visible: boolean }
    | { type: 'hide' }
    | { type: 'exit' };

export type OverlayAction = Extract<ControllerAction, {
    type: 'settings' | 'route' | 'routeSettings' | 'retryRoute' | 'capture' | 'requestCaptureOptions' | 'translation' | 'hide';
}>;

/// Keyed by provider id, so adding a translator needs no change here.
export type Credentials = Record<string, boolean>;

export interface TranscriptTurn {
    type?: 'draft' | 'turn';
    routeId: RouteId;
    original: string;
    translation: string;
    timestamp: string | number;
    engine?: string;
    model?: string;
    sourceLanguage?: string;
    targetLanguage?: string;
}

export interface TranscriptDraft {
    original: string;
    translation: string;
}

export interface TranscriptUpdate extends TranscriptTurn {
    type: 'draft' | 'turn';
}

export interface RouteRuntimeState {
    state: RouteState;
    error: string;
}

export interface ControllerSnapshot {
    locale: ResolvedLocale;
    preferredLocale: LocaleSetting;
    translationState: TranslationState;
    elapsedSeconds: number;
    overlayVisible: boolean;
    config: AppConfig['overlay'];
    audio: AppConfig['audio'];
    qwen: AppConfig['qwen'];
    capture: {
        capabilities: CaptureCapabilities;
        applications: ApplicationReference[];
        loading: boolean;
    };
    credentials: Credentials;
    routes: Record<RouteId, {
        config: RouteConfig;
        state: RouteState;
        error: string;
        turns: TranscriptTurn[];
        draft: TranscriptDraft;
    }>;
    notice: { id: number; code: string | null; message: string } | null;
}

export type SessionExportMode = 'source' | 'target' | 'both';

export interface SessionSummary {
    id: string;
    title: string;
    created_at: string;
    duration_sec: number;
}

export interface SessionSegment {
    ts: string;
    route_id?: RouteId;
    src: string;
    tgt: string;
}

export interface SessionDetail {
    json: {
        chunks: Array<{ segments: SessionSegment[] }>;
    };
}
