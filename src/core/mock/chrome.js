// The panel's open state, which the Rust side keeps and announces to both control
// windows; in the browser preview every control lives in one page.

const listeners = new Set();
let open = false;

export async function setOverlaySettingsOpen(next) {
    open = Boolean(next);
    for (const handler of listeners) handler(open);
}

export async function onOverlaySettingsOpen(handler) {
    listeners.add(handler);
    return () => listeners.delete(handler);
}
