import { mount } from 'svelte';
import OverlayChromeApp from './OverlayChromeApp.svelte';
import './styles/tokens.css';
import './styles/base.css';
import './styles/controls.css';
import './styles/chrome.css';

mount(OverlayChromeApp, {
    target: document.getElementById('chrome')!,
});
