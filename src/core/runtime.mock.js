import { DEFAULT_CONFIG } from '../app/defaults';
import { formatDuration } from '../app/format';
import { translator } from '../engines/registry.js';
import { locale } from './i18n.js';

const subscribers = new Map();
let controllerState;
let timers = [];

export async function subscribeController(surface, handler) {
    subscribers.set(surface, handler);
    handler(structuredClone(snapshot()));
    return () => subscribers.delete(surface);
}

export async function sendControllerAction(request) {
    const state = snapshot();
    switch (request.type) {
        case 'toggleOverlay':
            state.overlayVisible = !state.overlayVisible;
            state.config.enabled = state.overlayVisible;
            break;
        case 'showOverlay':
            state.overlayVisible = Boolean(request.visible);
            state.config.enabled = state.overlayVisible;
            break;
        case 'hide':
            state.overlayVisible = false;
            state.config.enabled = false;
            break;
        case 'settings':
            Object.assign(state.config, request.patch);
            break;
        case 'route':
            state.routes[request.routeId].config.enabled = request.enabled;
            break;
        case 'retryRoute': {
            const route = state.routes[request.routeId];
            route.state = 'connecting';
            route.error = '';
            break;
        }
        case 'routeSettings': {
            const route = state.routes[request.routeId].config;
            Object.assign(route, request.patch);
            route.engine = translator(route.model)?.provider ?? route.engine;
            break;
        }
        case 'qwenSettings':
            Object.assign(state.qwen, request.patch);
            break;
        case 'locale':
            state.preferredLocale = request.locale;
            state.locale = request.locale === 'auto' ? locale() : request.locale;
            break;
        case 'capture':
            updateCapture(state, request.bundleId);
            break;
        case 'requestCaptureOptions':
            state.capture.applications = [
                { bundleId: 'us.zoom.xos', name: 'Zoom' },
                { bundleId: 'com.apple.Safari', name: 'Safari' },
            ];
            break;
        case 'toggleTranslation':
            setTranslation(['starting', 'running', 'paused'].includes(state.translationState) ? 'stop' : 'start');
            return;
        case 'translation':
            setTranslation(request.command);
            return;
        default:
            break;
    }
    persistConfig(state);
    broadcast();
}

export async function invoke(command, args = {}) {
    switch (command) {
        case 'get_credential_status':
            return readJson('overlingo-credentials') || { qwen: true, openai: false };
        case 'set_provider_credential': {
            const status = readJson('overlingo-credentials') || { qwen: false, openai: false };
            status[args.provider] = Boolean(args.secret?.trim());
            writeJson('overlingo-credentials', status);
            return status;
        }
        case 'list_sessions':
            return sessionList();
        case 'read_session': {
            const session = (readJson('overlingo-sessions') || {})[args.id];
            if (!session) throw new Error('Session not found');
            return session;
        }
        case 'rename_session': {
            const sessions = readJson('overlingo-sessions') || {};
            const session = sessions[args.id];
            if (!session) throw new Error('Session not found');
            session.json.title = args.title;
            session.md = renderMarkdown(session.json, 'both');
            writeJson('overlingo-sessions', sessions);
            return null;
        }
        case 'export_session': {
            const session = (readJson('overlingo-sessions') || {})[args.id];
            if (!session) throw new Error('Session not found');
            downloadFile(
                `${session.json.title}.txt`,
                renderTranscript(session.json, args.mode, args.labels),
            );
            return true;
        }
        case 'delete_session': {
            const sessions = readJson('overlingo-sessions') || {};
            delete sessions[args.id];
            writeJson('overlingo-sessions', sessions);
            return null;
        }
        default:
            throw new Error(`Unsupported browser command: ${command}`);
    }
}

export async function getAutostartEnabled() {
    return localStorage.getItem('overlingo-autostart') === 'true';
}

export async function setAutostartEnabled(enabled) {
    localStorage.setItem('overlingo-autostart', String(enabled));
}

function snapshot() {
    if (controllerState) return controllerState;
    const config = readJson('overlingo-config') || structuredClone(DEFAULT_CONFIG);
    const route = routeId => ({
        config: structuredClone(config.routes[routeId]),
        state: 'stopped',
        error: '',
        turns: [],
        draft: { original: '', translation: '' },
    });
    controllerState = {
        locale: config.locale === 'auto' ? locale() : config.locale,
        preferredLocale: config.locale,
        translationState: 'stopped',
        elapsedSeconds: 0,
        overlayVisible: true,
        config: structuredClone(config.overlay),
        audio: structuredClone(config.audio),
        qwen: structuredClone(config.qwen),
        capture: {
            capabilities: { applicationCapture: true },
            applications: [],
            loading: false,
        },
        credentials: { qwen: true, openai: true },
        routes: {
            system: route('system'),
            microphone: route('microphone'),
        },
        notice: null,
    };
    return controllerState;
}

function setTranslation(command) {
    const state = snapshot();
    if (command === 'stop' || command === 'pause') {
        timers.forEach(clearTimeout);
        timers = [];
        state.translationState = command === 'pause' ? 'paused' : 'stopped';
        broadcast();
        return;
    }
    if (command === 'resume') {
        state.translationState = 'running';
    } else if (command === 'start') {
        state.translationState = 'starting';
        state.overlayVisible = true;
        state.config.enabled = true;
        timers.push(setTimeout(() => {
            state.translationState = 'running';
            broadcast();
        }, 180));
        queueTurn('system', 700,
            'Thanks for joining. Let us review the launch plan.',
            '感谢大家参加。我们来确认一下发布计划。');
        queueTurn('microphone', 1100,
            '好的，我稍后发送最终时间表。',
            'Okay, I will send the final timeline shortly.');
    }
    broadcast();
}

function queueTurn(routeId, delay, original, translation) {
    timers.push(setTimeout(() => {
        const state = snapshot();
        state.routes[routeId].turns.push({
            type: 'turn',
            routeId,
            original,
            translation,
            timestamp: new Date().toISOString(),
            ...state.routes[routeId].config,
        });
        broadcast();
    }, delay));
}

function updateCapture(state, bundleId) {
    const application = state.capture.applications.find(item => item.bundleId === bundleId);
    state.audio.system = bundleId === 'all'
        ? { scope: 'all', application: null }
        : { scope: 'application', application };
}

function persistConfig(state) {
    writeJson('overlingo-config', {
        schemaVersion: 1,
        locale: state.preferredLocale,
        audio: state.audio,
        routes: Object.fromEntries(
            Object.entries(state.routes).map(([id, route]) => [id, route.config]),
        ),
        overlay: state.config,
        qwen: state.qwen,
    });
}

function sessionList() {
    const sessions = Object.values(readJson('overlingo-sessions') || {});
    return sessions.map(({ json }) => ({
        id: json.id,
        title: json.title,
        created_at: json.created_at,
        duration_sec: json.duration_sec,
    })).sort((a, b) => b.created_at.localeCompare(a.created_at));
}

function renderMarkdown(data, mode) {
    const lines = [`# ${data.title}`, '', data.created_at, ''];
    for (const segment of data.chunks.flatMap(chunk => chunk.segments)) {
        const source = mode === 'target' ? '' : segment.src;
        const target = mode === 'source' ? '' : segment.tgt;
        if (!source && !target) continue;
        lines.push(`## ${segment.ts} · ${segment.route_id}`, '');
        if (source) lines.push(`**${segment.source_lang}**  ${source}`);
        if (target) lines.push(`**${segment.target_lang}**  ${target}`);
        lines.push('');
    }
    return lines.join('\n');
}

function renderTranscript(data, mode, labels = {}) {
    const lines = [data.title, `${data.created_at} · ${formatDuration(data.duration_sec)}`, ''];
    for (const segment of data.chunks.flatMap(chunk => chunk.segments)) {
        const source = mode === 'target' ? '' : segment.src;
        const target = mode === 'source' ? '' : segment.tgt;
        if (!source && !target) continue;
        lines.push(`[${segment.ts}] ${labels[segment.route_id] || segment.route_id}`);
        if (source) lines.push(source);
        if (target) lines.push(target);
        lines.push('');
    }
    return lines.join('\n');
}

function downloadFile(name, content) {
    const url = URL.createObjectURL(new Blob([content], { type: 'text/markdown' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    link.click();
    URL.revokeObjectURL(url);
}

function broadcast() {
    const state = structuredClone(snapshot());
    for (const handler of subscribers.values()) handler(state);
}

function readJson(key) {
    try {
        return JSON.parse(localStorage.getItem(key) || 'null');
    } catch {
        return null;
    }
}

function writeJson(key, value) {
    localStorage.setItem(key, JSON.stringify(value));
}
