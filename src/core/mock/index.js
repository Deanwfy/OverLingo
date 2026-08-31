// Fake backend for the browser preview (`npm run dev:web`), where there is no Tauri.
// runtime.js loads it only in dev builds when isTauri() is false.

export { getAutostartEnabled, invoke, setAutostartEnabled } from './commands.js';
export { sendControllerAction, subscribeController } from './controller.js';
