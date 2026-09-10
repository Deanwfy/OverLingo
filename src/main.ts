import { mount } from 'svelte';
import App from './App.svelte';
import './styles/tokens.css';
import './styles/base.css';
import './styles/controls.css';
import './styles/shell.css';
import './styles/translators.css';
import './styles/history.css';

mount(App, {
    target: document.getElementById('app')!,
});
