import { DEFAULT_CONFIG } from '../../app/defaults';
import { translator } from '../../engines/registry.js';
import { locale } from '../i18n.js';
import { readJson, writeJson } from './storage.js';

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
        case 'microphoneDevice':
            state.audio.microphone.device = request.device === 'default' ? null : request.device;
            break;
        case 'requestCaptureOptions':
            state.capture.microphones = ['MacBook Pro Microphone', 'AirPods Pro'];
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
            microphones: [],
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
        timers.forEach(clearInterval);
        timers = [];
        state.translationState = command === 'pause' ? 'paused' : 'stopped';
        broadcast();
        return;
    }
    if (command === 'resume') {
        state.translationState = 'running';
        tickElapsed(state);
    } else if (command === 'start') {
        state.translationState = 'starting';
        state.overlayVisible = true;
        state.config.enabled = true;
        state.elapsedSeconds = 0;
        timers.push(setTimeout(() => {
            state.translationState = 'running';
            broadcast();
        }, 180));
        tickElapsed(state);
        for (const turn of DEMO_TURNS) queueTurn(...turn);
    }
    broadcast();
}

function tickElapsed(state) {
    timers.push(setInterval(() => {
        state.elapsedSeconds += 1;
        broadcast();
    }, 1000));
}

// A two-way meeting: the other side over system audio, the user on the microphone.
const DEMO_TURNS = [
    ['system', 700, {
        en: 'Thanks for joining. Let us review the launch plan.',
        zh: '感谢大家参加。我们来确认一下发布计划。',
    }],
    ['microphone', 1500, {
        en: 'Okay, I will send the final timeline shortly.',
        zh: '好的，我稍后发送最终时间表。',
    }],
    ['system', 2300, {
        en: 'Great. Can we ship the beta next Friday?',
        zh: '很好。测试版下周五能发布吗？',
    }],
    ['microphone', 3100, {
        en: 'Yes, once external testing is done.',
        zh: '可以，等外部测试完成就发。',
    }],
];

function queueTurn(routeId, delay, text) {
    timers.push(setTimeout(() => {
        const state = snapshot();
        const route = state.routes[routeId];
        const speaks = language => text[language] ?? text.en;
        route.turns.push({
            type: 'turn',
            routeId,
            original: speaks(route.config.sourceLanguage),
            translation: speaks(route.config.targetLanguage),
            timestamp: new Date().toISOString(),
            ...route.config,
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

function broadcast() {
    const state = structuredClone(snapshot());
    for (const handler of subscribers.values()) handler(state);
}
