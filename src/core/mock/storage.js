// The browser preview keeps everything the Rust side would persist in localStorage.

export function readJson(key) {
    try {
        return JSON.parse(localStorage.getItem(key) || 'null');
    } catch {
        return null;
    }
}

export function writeJson(key, value) {
    localStorage.setItem(key, JSON.stringify(value));
}
