<script lang="ts">
    import { formatDuration } from '../app/format';
    import type { ControllerSnapshot, OverlayAction, OverlaySettingsPatch } from '../app/types';
    import { t } from '../core/locale.svelte';
    import { dragOverlay, sendControllerAction } from '../core/runtime.js';
    import Icon from './Icon.svelte';

    let { state, settingsOpen, onToggleSettings, send, update }: {
        state: ControllerSnapshot;
        settingsOpen: boolean;
        onToggleSettings: () => void;
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

    // Pressing between the buttons moves the subtitle window, not this one: the backend
    // follows the cursor until release and lets this window ride along.
    let dragging = false;

    function beginDrag(event: PointerEvent) {
        if (event.button !== 0 || (event.target as Element).closest('button')) return;
        event.preventDefault();
        (event.currentTarget as Element).setPointerCapture(event.pointerId);
        dragging = true;
        void dragOverlay(true);
    }

    function endDrag() {
        if (!dragging) return;
        dragging = false;
        void dragOverlay(false);
    }
</script>

<header
    class="overlay-chrome"
    role="group"
    aria-label={t('moveOverlay')}
    onpointerdown={beginDrag}
    onpointerup={endDrag}
    onpointercancel={endDrag}
>
    <nav aria-label={t('overlayControls')}>
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
            onclick={onToggleSettings}
        ><Icon name="settings" size={15} /></button>
        <button
            class="close-overlay"
            title={t('hideOverlay')}
            aria-label={t('hideOverlay')}
            onclick={() => void hideOverlayWindow()}
        ><Icon name="close" size={15} /></button>
    </nav>
</header>
