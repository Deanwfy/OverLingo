import { DEFAULT_CONFIG } from '../../app/defaults';
import { translator } from '../../engines/registry.js';
import { locale } from '../i18n.js';
import { saveSession } from './commands.js';
import { readJson, writeJson } from './storage.js';

const subscribers = new Map();
let controllerState;
let timers = [];

export async function subscribeController(surface, handler) {
    subscribers.set(surface, handler);
    handler(structuredClone(snapshot()));
    // `?autostart` lets scripts/record-demo.mjs open the overlay straight into the demo.
    if (!autostarted && demoParams().has('autostart')) {
        autostarted = true;
        setTranslation('start');
    }
    return () => subscribers.delete(surface);
}

const demoParams = () => new URLSearchParams(location.search);
// `?scene=video` plays a single-route scene; anything else is the two-way meeting.
const demoScene = () => (demoParams().get('scene') === 'video' ? 'video' : 'meeting');
// `?layout=merged` shows both routes on one timeline for this load only.
const demoLayout = () => (demoParams().get('layout') === 'merged' ? 'merged' : null);
// `?speed=0.25` slows the demo so every frame can be captured; timing is scaled back on encode.
const demoSpeed = () => Number(demoParams().get('speed')) || 1;
let autostarted = false;

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
            Object.assign(state.capture, structuredClone(CAPTURE));
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

const CAPTURE = {
    applications: [
        { bundleId: 'us.zoom.xos', name: 'Zoom' },
        { bundleId: 'com.apple.Safari', name: 'Safari' },
    ],
    microphones: ['MacBook Pro Microphone', 'AirPods Pro'],
};

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
    if (demoScene() === 'video') config.routes.microphone.enabled = false;
    if (demoLayout()) config.overlay.layout = demoLayout();
    const demo = demoParams().has('autostart');
    controllerState = {
        locale: config.locale === 'auto' ? locale() : config.locale,
        preferredLocale: config.locale,
        translationState: 'stopped',
        elapsedSeconds: 0,
        overlayVisible: true,
        config: structuredClone(config.overlay),
        // The demo listens to Zoom through AirPods, so the settings panel has something to show.
        audio: demo
            ? { system: { scope: 'application', application: CAPTURE.applications[0] }, microphone: { device: CAPTURE.microphones[1] } }
            : structuredClone(config.audio),
        qwen: structuredClone(config.qwen),
        capture: { capabilities: { applicationCapture: true }, applications: [], microphones: [], loading: false },
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
        demoRun += 1;
        state.translationState = command === 'pause' ? 'paused' : 'stopped';
        if (command === 'stop') {
            saveSession(state, demoParams().has('autostart') ? SCENE_TITLES[demoScene()] : null);
            for (const route of Object.values(state.routes)) {
                route.turns = [];
                route.draft = { original: '', translation: '' };
            }
        }
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
        state.startedAt = new Date().toISOString();
        timers.push(setTimeout(() => {
            state.translationState = 'running';
            broadcast();
        }, 180));
        tickElapsed(state);
        playDemo(state);
    }
    broadcast();
}

function tickElapsed(state) {
    timers.push(setInterval(() => {
        state.elapsedSeconds += 1;
        broadcast();
    }, 1000 / demoSpeed()));
}

// Sessions the demo saves get a name instead of the timestamp the app would use.
const SCENE_TITLES = { meeting: 'Launch timeline sync', video: 'Lip-sync video' };

// Each route picks its original from its source language and its translation from its target.
const DEMO_SCENES = {
    // A two-way meeting: the other side over system audio, the user on the microphone.
    meeting: [
        ['system', {
            en: "Thanks for joining. Let's go over the launch timeline.",
            zh: '感谢参加。我们过一下发布时间线。',
        }],
        ['system', {
            en: "We're aiming for the beta next Friday, if QA signs off.",
            zh: '如果 QA 通过，我们计划下周五发 beta。',
        }],
        ['microphone', {
            en: 'Sure, the translations will be done this week.',
            zh: '可以，翻译这周就能交。',
        }],
        ['system', {
            en: 'Can your side have the localized strings ready by then?',
            zh: '你们那边能在那之前把本地化文案准备好吗？',
        }],
        ['microphone', {
            en: "The strings are fine. But the font license isn't confirmed yet, it may take another two days.",
            zh: '文案没问题。但字体授权还没确认，可能要再等两天。',
        }],
        ['system', {
            en: "Okay, let's lock the date and revisit the font on Wednesday.",
            zh: '好，那先定下日期，字体的事周三再看。',
        }],
    ],
    // A video with no subtitles, system audio only.
    video: [
        ['system', {
            en: "You've probably seen this online: why does it always feel like the voice doesn't match the mouth when someone on screen is talking?",
            zh: '在互联网上你应该多多少少看过这样的，为什么总感觉屏幕里这个人说话的时候他的声音和嘴对不上？',
        }],
        ['system', {
            en: 'Everyone has wondered about this. So in this video we looked into it properly.',
            zh: '所有人都有这个疑惑。所以这个视频我们认真地调研了一下。',
        }],
    ],
};

let demoRun = 0;
const wait = ms => new Promise(resolve => timers.push(setTimeout(resolve, ms / demoSpeed())));
const isCjk = text => /[㐀-鿿]/.test(text);
// Roughly how the providers stream: Latin text by word, CJK by short runs of characters.
const pieces = text => (isCjk(text) ? text.match(/.{1,3}/g) : text.split(/(?<=\s)/));

async function playDemo(state) {
    const run = ++demoRun;
    await wait(1500);
    for (const [routeId, text] of DEMO_SCENES[demoScene()]) {
        if (run !== demoRun) return;
        const route = state.routes[routeId];
        const speaks = language => text[language] ?? text.en;
        const original = speaks(route.config.sourceLanguage);
        const translation = speaks(route.config.targetLanguage);
        const latin = !isCjk(original);
        for (const piece of pieces(original)) {
            route.draft.original += piece;
            broadcast();
            await wait(latin ? 110 : 140);
        }
        await wait(350);
        for (const piece of pieces(translation)) {
            route.draft.translation += piece;
            broadcast();
            await wait(latin ? 90 : 60);
        }
        if (run !== demoRun) return;
        route.turns.push({
            type: 'turn',
            routeId,
            original,
            translation,
            timestamp: new Date().toISOString(),
            elapsed: state.elapsedSeconds,
            ...route.config,
        });
        route.draft = { original: '', translation: '' };
        broadcast();
        await wait(900);
    }
    await wait(4000);
    if (run === demoRun) document.documentElement.dataset.demoFinished = 'true';
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
