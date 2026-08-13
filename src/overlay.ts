import { mount } from 'svelte';
import OverlayApp from './OverlayApp.svelte';
import './styles/overlay.css';

mount(OverlayApp, {
    target: document.getElementById('overlay')!,
});
