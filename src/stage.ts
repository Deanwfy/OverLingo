// The overlay as the app shows it: the subtitle box with the toolbar and the settings
// panel floating over it. Those are windows of their own in the app, placed by
// overlay_chrome.rs; here one page stands in for the window system. Browser preview
// and scripts/record-demo.mjs only, never built.
import { mount } from 'svelte';
import OverlayApp from './OverlayApp.svelte';
import OverlayChromeApp from './OverlayChromeApp.svelte';
import { onOverlaySettingsOpen, setOverlaySettingsOpen } from './core/runtime.js';
import './styles/tokens.css';
import './styles/base.css';
import './styles/controls.css';
import './styles/overlay.css';
import './styles/chrome.css';
import './styles/stage.css';

const stage = document.getElementById('stage')!;
const toolbar = document.getElementById('toolbar')!;
const panel = document.getElementById('panel')!;

mount(OverlayApp, { target: document.getElementById('overlay')! });
mount(OverlayChromeApp, { target: toolbar, props: { view: 'toolbar' } });
mount(OverlayChromeApp, { target: panel, props: { view: 'panel' } });

// The horizontal rule of panel_frame in overlay_chrome.rs: hung with a third of its width
// to the right of the settings button, pushed back inside the viewport at either side.
// The panel always fits below the toolbar here, so its vertical rule is not needed.
function placePanel() {
    const button = toolbar.querySelector<HTMLElement>('button[aria-expanded]');
    if (!button || panel.hidden) return;
    const origin = stage.getBoundingClientRect().left;
    const buttonBox = button.getBoundingClientRect();
    const anchor = buttonBox.left + buttonBox.width / 2 - origin;
    const width = panel.offsetWidth;
    const left = Math.min(anchor - width * 2 / 3, document.documentElement.clientWidth - origin - width);
    panel.style.left = `${Math.max(left, -origin)}px`;
}

const observer = new ResizeObserver(placePanel);
observer.observe(toolbar);
observer.observe(panel);

void onOverlaySettingsOpen((open: boolean) => {
    panel.hidden = !open;
});

// The global click monitor's part: a press anywhere but on the controls closes the panel.
document.addEventListener('pointerdown', (event) => {
    const target = event.target as Node;
    if (!toolbar.contains(target) && !panel.contains(target)) void setOverlaySettingsOpen(false);
});
