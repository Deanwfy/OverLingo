import { Channel, invoke as invokeNative, isTauri } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
const native = isTauri();
// The browser preview's fake backend only exists in dev builds; production builds drop it.
const mock = !native && import.meta.env.DEV ? import('./mock/index.js') : null;

export async function invoke(command, args = {}) {
    return native ? invokeNative(command, args) : (await mock).invoke(command, args);
}

export async function subscribeController(surface, handler) {
    if (!native) return (await mock).subscribeController(surface, handler);
    const channel = new Channel();
    channel.onmessage = handler;
    await invokeNative('subscribe_controller', { surface, onEvent: channel });
    return () => {
        channel.onmessage = null;
    };
}

export async function sendControllerAction(request) {
    return native
        ? invokeNative('controller_action', { request })
        : (await mock).sendControllerAction(request);
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

// Cursor position inside the toolbar window, null once it leaves; the toolbar never
// holds focus, and WebKit only tracks the mouse for :hover in a focused window.
export async function onOverlayPointerAt(handler) {
    if (!native) return () => {};
    return listen('overlay://pointer-at', event => handler(event.payload));
}

export async function setOverlayInteractiveHeight(height) {
    if (native) await invokeNative('set_overlay_interactive_height', { height: Math.ceil(height) });
}

export async function setOverlaySettingsOpen(open) {
    if (native) await invokeNative('set_overlay_settings_open', { open });
}

export async function setOverlayChromeSize({ width, height, anchor }) {
    if (native) await invokeNative('set_overlay_chrome_size', { width, height, anchor });
}

// Moves the subtitle window, or resizes it from a corner (`ne`, ...); the backend
// follows the cursor itself between begin and end.
export async function dragOverlay(begin, edge = null) {
    if (native) await invokeNative('drag_overlay', { begin, edge });
}

// The panel is a window of its own, so its open state is kept by the backend and
// announced to both control windows.
export async function onOverlaySettingsOpen(handler) {
    if (!native) return () => {};
    return listen('overlay://settings-open', event => handler(Boolean(event.payload)));
}

// The global click monitor only sees other apps' windows; a click on the subtitles has
// to tell the panel window itself.
export async function dismissOverlayChrome() {
    if (native) await emitTo('overlay-panel', 'overlay://outside-click');
}

// Routed through the opener plugin rather than `window.open`, which a webview with no
// browser chrome cannot service. Allowed destinations are pinned in capabilities.
export async function openExternal(url) {
    if (native) await invokeNative('plugin:opener|open_url', { url });
    else window.open(url, '_blank', 'noopener');
}

export async function getAutostartStatus() {
    return native ? invokeNative('get_autostart_status') : (await mock).getAutostartStatus();
}

export async function setAutostartEnabled(enabled) {
    return native
        ? invokeNative('set_autostart_enabled', { enabled })
        : (await mock).setAutostartEnabled(enabled);
}

export async function openAutostartSettings() {
    if (native) await invokeNative('open_autostart_settings');
}
