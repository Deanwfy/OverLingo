import { Channel, invoke as invokeNative, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
    disable as disableAutostart,
    enable as enableAutostart,
    isEnabled as isAutostartEnabled,
} from '@tauri-apps/plugin-autostart';
import * as mock from './runtime.mock.js';

const native = isTauri();

export function invoke(command, args = {}) {
    return native ? invokeNative(command, args) : mock.invoke(command, args);
}

export async function subscribeController(surface, handler) {
    if (!native) return mock.subscribeController(surface, handler);
    const channel = new Channel();
    channel.onmessage = handler;
    await invokeNative('subscribe_controller', { surface, onEvent: channel });
    return () => {
        channel.onmessage = null;
    };
}

export function sendControllerAction(request) {
    return native
        ? invokeNative('controller_action', { request })
        : mock.sendControllerAction(request);
}

export async function showSettingsWindow() {
    if (native) await invokeNative('show_settings_window');
}

export async function onOverlayOutsideClick(handler) {
    if (!native) return () => {};
    return listen('overlay://outside-click', () => handler());
}

export async function onOverlayPointerHover(handler) {
    // The browser preview has no native cursor polling; pretend the pointer rests on the
    // controls so the toolbar cannot disappear for good.
    if (!native) {
        handler(true);
        return () => {};
    }
    return listen('overlay://pointer-hover', event => handler(Boolean(event.payload)));
}

export async function setOverlayInteractiveHeight(height) {
    if (native) await invokeNative('set_overlay_interactive_height', { height: Math.ceil(height) });
}

// Routed through the opener plugin rather than `window.open`, which a webview with no
// browser chrome cannot service. Allowed destinations are pinned in capabilities.
export async function openExternal(url) {
    if (native) await invokeNative('plugin:opener|open_url', { url });
    else window.open(url, '_blank', 'noopener');
}

export function getAutostartEnabled() {
    return native ? isAutostartEnabled() : mock.getAutostartEnabled();
}

export async function setAutostartEnabled(enabled) {
    if (!native) return mock.setAutostartEnabled(enabled);
    if (enabled) await enableAutostart();
    else await disableAutostart();
}
