<script lang="ts">
    import { formatDuration } from '../app/format';
    import type { ControllerSnapshot, OverlayAction, OverlaySettingsPatch } from '../app/types';
    import { t } from '../core/locale.svelte';
    import { sendControllerAction } from '../core/runtime.js';
    import Icon from './Icon.svelte';

    let {
        state,
        settingsOpen = $bindable(false),
        chromeElement = $bindable(null),
        toolbarElement = $bindable(null),
        send,
        update,
    }: {
        state: ControllerSnapshot;
        settingsOpen?: boolean;
        chromeElement?: HTMLElement | null;
        toolbarElement?: HTMLElement | null;
        send: (action: OverlayAction) => void;
        update: (patch: OverlaySettingsPatch) => void;
    } = $props();

    let controlLabel = $derived(
        state.translationState === 'paused' ? 'resume'
            : state.translationState === 'running' ? 'pause'
                : state.translationState === 'starting' ? 'connecting'
                    : 'start',
    );

    function toggleAlwaysOnTop() {
        const enabled = !state.config.alwaysOnTop;
        update({ alwaysOnTop: enabled });
    }

    function toggleSettings() {
        settingsOpen = !settingsOpen;
        if (settingsOpen) send({ type: 'requestCaptureOptions' });
    }

    function controlTranslation() {
        const command = state.translationState === 'paused'
            ? 'resume'
            : state.translationState === 'running'
                ? 'pause'
                : 'start';
        send({ type: 'translation', command });
    }

    async function hideOverlayWindow() {
        await sendControllerAction({ type: 'hide' });
    }
</script>

<header class="overlay-chrome overlay-reveal" bind:this={chromeElement}>
    <div class="drag-surface" data-tauri-drag-region></div>
    <nav aria-label={t('overlayControls')} bind:this={toolbarElement}>
        <button
            class="translation-control"
            class:running={state.translationState === 'running'}
            disabled={state.translationState === 'starting'}
            title={t(controlLabel)}
            onclick={controlTranslation}
        >
            <Icon name={controlLabel === 'resume' || controlLabel === 'pause' ? controlLabel : 'play'} size={14} />
            <span>{t(controlLabel)}</span>
            {#if state.translationState !== 'stopped' && state.translationState !== 'failed'}
                <time>{formatDuration(state.elapsedSeconds)}</time>
            {/if}
        </button>
        {#if ['running', 'paused', 'starting'].includes(state.translationState)}
            <button
                class="stop-translation"
                title={t('end')}
                aria-label={t('end')}
                onclick={() => send({ type: 'translation', command: 'stop' })}
            ><Icon name="stop" size={13} /></button>
        {/if}
        <button
            class:active={state.config.alwaysOnTop}
            title={t(state.config.alwaysOnTop ? 'unpinOverlay' : 'pinOverlay')}
            aria-label={t(state.config.alwaysOnTop ? 'unpinOverlay' : 'pinOverlay')}
            aria-pressed={state.config.alwaysOnTop}
            onclick={toggleAlwaysOnTop}
        ><Icon name="pin" size={15} /></button>
        <button
            class:active={settingsOpen}
            title={t('overlaySettings')}
            aria-label={t('overlaySettings')}
            aria-expanded={settingsOpen}
            onclick={toggleSettings}
        ><Icon name="settings" size={15} /></button>
        <button
            class="close-overlay"
            title={t('hideOverlay')}
            aria-label={t('hideOverlay')}
            onclick={() => void hideOverlayWindow()}
        ><Icon name="close" size={15} /></button>
    </nav>
</header>
