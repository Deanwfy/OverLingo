<script lang="ts">
    import { onMount } from 'svelte';
    import type {
        ControllerSnapshot,
        OverlayAction,
        OverlaySettingsPatch,
        RouteId,
    } from './app/types';
    import OverlaySettingsPanel from './components/OverlaySettingsPanel.svelte';
    import OverlaySubtitles from './components/OverlaySubtitles.svelte';
    import OverlayToolbar from './components/OverlayToolbar.svelte';
    import { setLocale } from './core/locale.svelte';
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
    let toolbarElement = $state<HTMLElement | null>(null);
    // The toolbar grows when the timer appears, and the rightmost route's labels have to
    // stop short of it.
    let toolbarWidth = $state(0);
    let reportedHeight = 0;
    const routeIds: RouteId[] = ['system', 'microphone'];
    let activeRouteCount = $derived(
        routeIds.filter(routeId => overlayState?.routes[routeId]?.config.enabled !== false).length,
    );
    // A single route already occupies the whole overlay, so merging only means anything
    // with two of them.
    let merged = $derived(activeRouteCount > 1 && overlayState?.config.layout === 'merged');

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

    $effect(() => {
        if (!toolbarElement) return;
        const observer = new ResizeObserver(([entry]) => {
            toolbarWidth = Math.ceil(entry.borderBoxSize[0]?.inlineSize ?? entry.contentRect.width);
        });
        observer.observe(toolbarElement);
        return () => observer.disconnect();
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

    async function openMoreSettings() {
        settingsOpen = false;
        await showSettingsWindow();
    }
</script>

{#if overlayState}
    <main
        class:settings-open={settingsOpen}
        class:click-through={overlayState.config.clickThrough}
        class:pointer-over-controls={pointerOverControls}
        class="overlay-shell"
        style:--active-route-count={merged ? 1 : Math.max(activeRouteCount, 1)}
        style:--overlay-opacity={overlayState.config.opacity}
        style:--font-scale={overlayState.config.fontScale}
        style:--toolbar-width="{toolbarWidth}px"
    >
        <OverlayToolbar
            state={overlayState}
            bind:settingsOpen
            bind:chromeElement
            bind:toolbarElement
            {send}
            {update}
        />

        {#if settingsOpen}
            <OverlaySettingsPanel
                state={overlayState}
                {send}
                {update}
                onMoreSettings={openMoreSettings}
            />
        {/if}

        <OverlaySubtitles
            state={overlayState}
            {merged}
            {send}
            onOpenSettings={openMoreSettings}
        />
    </main>
{/if}
