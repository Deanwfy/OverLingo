import { mount } from 'svelte';
import OverlayApp from './OverlayApp.svelte';
import './styles/tokens.css';
import './styles/base.css';
import './styles/controls.css';
import './styles/overlay.css';

mount(OverlayApp, {
    target: document.getElementById('overlay')!,
});
