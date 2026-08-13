<script lang="ts">
    import { onMount } from 'svelte';
    import { formatDuration } from './app/format';
    import type {
        ControllerSnapshot,
        OverlayAction,
        OverlaySettingsPatch,
        RouteConfig,
        RouteId,
        RouteState,
    } from './app/types';
    import Icon from './components/Icon.svelte';
    import OverlaySettingsPanel from './components/OverlaySettingsPanel.svelte';
    import { setLocale, t } from './core/locale.svelte';
    import { translationDirection } from './core/languages.js';
    import { readableRuntimeError } from './core/errors.js';
    import {
        onOverlayOutsideClick,
        onOverlayPointerHover,
        sendControllerAction,
        setOverlayInteractiveHeight,
        showSettingsWindow,
        subscribeController,
    } from './core/runtime.js';

    // A little slack above the toolbar so nearing the top edge already reveals the controls.
    const CONTROL_STRIP_MARGIN = 8;

    let overlayState = $state<ControllerSnapshot | null>(null);
    let settingsOpen = $state(false);
    let pointerOverControls = $state(false);
    let chromeElement = $state<HTMLElement | null>(null);
    let reportedHeight = 0;
    const routeIds: RouteId[] = ['system', 'microphone'];
    let activeRouteCount = $derived(
        routeIds.filter(routeId => overlayState?.routes[routeId]?.config.enabled !== false).length,
    );
    let controlLabel = $derived(
        overlayState?.translationState === 'paused' ? 'resume'
            : overlayState?.translationState === 'running' ? 'pause'
                : overlayState?.translationState === 'starting' ? 'connecting'
                    : 'start',
    );

    onMount(() => {
        const stoppers: Array<() => void> = [];
        void subscribeController('overlay', (payload: ControllerSnapshot) => {
            overlayState = payload;
            setLocale(payload.locale);
        }).then((stop: () => void) => stoppers.push(stop));
        void onOverlayOutsideClick(closeSettings).then((stop: () => void) => stoppers.push(stop));
        void onOverlayPointerHover((hovering: boolean) => {
            pointerOverControls = hovering;
        }).then((stop: () => void) => stoppers.push(stop));
        document.addEventListener('pointerdown', dismissOnOutsidePointer, true);
        window.addEventListener('blur', dismissOnBlur);
        window.addEventListener('resize', reportInteractiveHeight);
        return () => {
            for (const stop of stoppers) stop();
            document.removeEventListener('pointerdown', dismissOnOutsidePointer, true);
            window.removeEventListener('blur', dismissOnBlur);
            window.removeEventListener('resize', reportInteractiveHeight);
        };
    });

    let failedRouteCount = $derived(
        routeIds.filter(routeId => overlayState?.routes[routeId]?.state === 'failed').length,
    );

    $effect(() => {
        void settingsOpen;
        void chromeElement;
        void failedRouteCount;
        reportInteractiveHeight();
    });

    function reportInteractiveHeight() {
        const controlStrip = chromeElement
            ? chromeElement.getBoundingClientRect().bottom + CONTROL_STRIP_MARGIN
            : 0;
        // Clicks pass through everything below the reported strip, so a failure notice with
        // buttons in it has to extend the strip or the user cannot reach its own actions.
        const reachable = [...document.querySelectorAll('.route-failure')]
            .map(element => element.getBoundingClientRect().bottom + CONTROL_STRIP_MARGIN);
        const height = Math.ceil(
            settingsOpen ? window.innerHeight : Math.max(controlStrip, ...reachable),
        );
        if (height === reportedHeight) return;
        reportedHeight = height;
        void setOverlayInteractiveHeight(height);
    }

    function closeSettings() {
        settingsOpen = false;
    }

    function dismissOnOutsidePointer(event: PointerEvent) {
        if (!settingsOpen) return;
        const target = event.target as HTMLElement | null;
        if (target?.closest('.overlay-settings-panel, .overlay-chrome nav')) return;
        closeSettings();
    }

    function dismissOnBlur() {
        if (document.activeElement instanceof HTMLSelectElement) return;
        closeSettings();
    }

    const readableError = (message: string) => readableRuntimeError(message, t);

    function direction(route: RouteConfig) {
        return translationDirection(
            route.sourceLanguage,
            route.targetLanguage,
            overlayState?.locale ?? 'en',
        );
    }

    function routeStatus(state: RouteState) {
        if (state === 'connecting') return 'connecting';
        if (state === 'reconnecting') return 'reconnecting';
        if (state === 'reconfiguring') return 'applyingSettings';
        return '';
    }

    function send(action: OverlayAction) {
        void sendControllerAction(action);
    }

    function update(patch: OverlaySettingsPatch) {
        if (!overlayState) return;
        if (patch.showOriginal === false && !overlayState.config.showTranslation) {
            patch.showTranslation = true;
        }
        if (patch.showTranslation === false && !overlayState.config.showOriginal) {
            patch.showOriginal = true;
        }
        overlayState = {
            ...overlayState,
            config: { ...overlayState.config, ...patch },
        };
        send({ type: 'settings', patch });
    }

    function toggleAlwaysOnTop() {
        const enabled = !overlayState?.config.alwaysOnTop;
        update({ alwaysOnTop: enabled });
    }

    function toggleClickThrough() {
        update({ clickThrough: !overlayState?.config.clickThrough });
    }

    function toggleSettings() {
        settingsOpen = !settingsOpen;
        if (settingsOpen) send({ type: 'requestCaptureOptions' });
    }

    function controlTranslation() {
        if (!overlayState) return;
        const command = overlayState.translationState === 'paused'
            ? 'resume'
            : overlayState.translationState === 'running'
                ? 'pause'
                : 'start';
        send({ type: 'translation', command });
    }

    async function openMoreSettings() {
        settingsOpen = false;
        await showSettingsWindow();
    }

    async function hideOverlayWindow() {
        await sendControllerAction({ type: 'hide' });
    }

    function fitKey(routeId: RouteId) {
        if (!overlayState) return '';
        const route = overlayState.routes[routeId];
        return [
            overlayState.config.fontScale,
            overlayState.config.showOriginal,
            overlayState.config.showTranslation,
            route.turns.length,
            route.draft.original,
            route.draft.translation,
        ].join(':');
    }

    function fitSubtitleTurns(node: HTMLElement, _dependency: string) {
        let frame = 0;
        const fit = () => {
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(() => {
                const turns = Array.from(node.querySelectorAll<HTMLElement>(':scope > .overlay-turn'));
                for (const turn of turns) turn.hidden = false;
                if (!turns.length) return;
                const gap = Number.parseFloat(getComputedStyle(node).rowGap) || 0;
                let used = 0;
                let visible = 0;
                for (let index = turns.length - 1; index >= 0; index -= 1) {
                    const turn = turns[index];
                    const required = turn.offsetHeight + (visible ? gap : 0);
                    if (visible && used + required > node.clientHeight) {
                        turn.hidden = true;
                    } else {
                        used += required;
                        visible += 1;
                    }
                }
            });
        };
        const resizeObserver = new ResizeObserver(fit);
        const mutationObserver = new MutationObserver(fit);
        resizeObserver.observe(node);
        mutationObserver.observe(node, { childList: true, subtree: true, characterData: true });
        fit();
        return {
            update() {
                fit();
            },
            destroy() {
                cancelAnimationFrame(frame);
                resizeObserver.disconnect();
                mutationObserver.disconnect();
            },
        };
    }
</script>

{#if overlayState}
    <main
        class:settings-open={settingsOpen}
        class:click-through={overlayState.config.clickThrough}
        class:pointer-over-controls={pointerOverControls}
        class="overlay-shell"
        style:--active-route-count={Math.max(activeRouteCount, 1)}
        style:--overlay-opacity={overlayState.config.opacity}
        style:--font-scale={overlayState.config.fontScale}
    >
        <header class="overlay-chrome overlay-reveal" bind:this={chromeElement}>
            <div class="drag-surface" data-tauri-drag-region></div>
            <nav aria-label={t('overlayControls')}>
                <button
                    class="translation-control"
                    class:running={overlayState.translationState === 'running'}
                    disabled={overlayState.translationState === 'starting'}
                    title={t(controlLabel)}
                    onclick={controlTranslation}
                >
                    <Icon name={controlLabel === 'resume' || controlLabel === 'pause' ? controlLabel : 'play'} size={14} />
                    <span>{t(controlLabel)}</span>
                    {#if overlayState.translationState !== 'stopped' && overlayState.translationState !== 'failed'}
                        <time>{formatDuration(overlayState.elapsedSeconds)}</time>
                    {/if}
                </button>
                {#if ['running', 'paused', 'starting'].includes(overlayState.translationState)}
                    <button
                        class="stop-translation"
                        title={t('end')}
                        aria-label={t('end')}
                        onclick={() => send({ type: 'translation', command: 'stop' })}
                    ><Icon name="stop" size={13} /></button>
                {/if}
                <button
                    class:active={overlayState.config.alwaysOnTop}
                    title={t(overlayState.config.alwaysOnTop ? 'unpinOverlay' : 'pinOverlay')}
                    aria-label={t(overlayState.config.alwaysOnTop ? 'unpinOverlay' : 'pinOverlay')}
                    aria-pressed={overlayState.config.alwaysOnTop}
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

        {#if settingsOpen}
            <OverlaySettingsPanel
                state={overlayState}
                {send}
                {update}
                onMoreSettings={openMoreSettings}
            />
        {/if}

        <div class="overlay-routes">
            {#each routeIds as routeId}
                {@const routeState = overlayState.routes[routeId]}
                {#if routeState?.config.enabled !== false}
                    <section class="overlay-route">
                        <header>
                            <span class="route-direction overlay-reveal" class:system={routeId === 'system'} lang={routeState.config.targetLanguage}>{direction(routeState.config)}</span>
                            {#if routeState.state === 'failed'}
                                {@const message = readableError(routeState.error)}
                                <span class="route-failure" role="alert">
                                    <span title={message}>{message}</span>
                                    <button
                                        title={t('retryRoute')}
                                        aria-label={t('retryRoute')}
                                        onclick={() => send({ type: 'retryRoute', routeId })}
                                    ><Icon name="retry" size={13} /></button>
                                    <button
                                        title={t('openSettings')}
                                        aria-label={t('openSettings')}
                                        onclick={openMoreSettings}
                                    ><Icon name="settings" size={13} /></button>
                                </span>
                            {:else if routeStatus(routeState.state)}
                                <small class="route-status">{t(routeStatus(routeState.state))}</small>
                            {/if}
                        </header>
                        <div class="overlay-turns" use:fitSubtitleTurns={fitKey(routeId)}>
                            {#each routeState.turns as turn}
                                <article class="overlay-turn">
                                    {#if overlayState.config.showOriginal && turn.original}<p>{turn.original}</p>{/if}
                                    {#if overlayState.config.showTranslation && turn.translation}<strong>{turn.translation}</strong>{/if}
                                </article>
                            {/each}
                            {#if routeState.draft.original || routeState.draft.translation}
                                <article class="overlay-turn draft">
                                    {#if overlayState.config.showOriginal && routeState.draft.original}<p>{routeState.draft.original}</p>{/if}
                                    {#if overlayState.config.showTranslation && routeState.draft.translation}<strong>{routeState.draft.translation}</strong>{/if}
                                </article>
                            {/if}
                            {#if routeState.turns.length === 0 && !routeState.draft.original && !routeState.draft.translation}
                                <p class="overlay-empty">{t('emptyOverlay')}</p>
                            {/if}
                        </div>
                    </section>
                {/if}
            {/each}
        </div>
    </main>
{/if}
