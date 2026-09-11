<script lang="ts">
    import { onMount } from 'svelte';
    import type { ControllerSnapshot, OverlayAction, RouteId } from './app/types';
    import OverlaySubtitles from './components/OverlaySubtitles.svelte';
    import { setLocale, t } from './core/locale.svelte';
    import {
        dismissOverlayChrome,
        dragOverlay,
        onOverlayPointerHover,
        sendControllerAction,
        setOverlayInteractiveHeight,
        showSettingsWindow,
        subscribeController,
    } from './core/runtime.js';

    // A little slack below a notice so its buttons stay reachable near the edge.
    const NOTICE_MARGIN = 8;
    // The window system no longer moves or resizes this window: with clicks passing
    // through, its edges are only reachable through the pointer watcher's band, so the
    // handles are drawn here and the backend follows the cursor. Edges move, corners resize.
    // The window system's own drag was tried and dropped: across displays it leaves the
    // transparent box smeared in stair-steps until release.
    const EDGES = ['n', 's', 'e', 'w'];
    const CORNERS = ['ne', 'nw', 'se', 'sw'];
    let dragging = false;

    let overlayState = $state<ControllerSnapshot | null>(null);
    let pointerOverControls = $state(false);
    let reportedHeight = 0;
    const routeIds: RouteId[] = ['system', 'microphone'];
    let activeRouteCount = $derived(
        routeIds.filter(routeId => overlayState?.routes[routeId]?.config.enabled !== false).length,
    );
    // A single route already occupies the whole overlay, so merging only means anything
    // with two of them.
    let merged = $derived(activeRouteCount > 1 && overlayState?.config.layout === 'merged');
    let failedRouteCount = $derived(
        routeIds.filter(routeId => overlayState?.routes[routeId]?.state === 'failed').length,
    );

    onMount(() => {
        const stoppers: Array<() => void> = [];
        void subscribeController('overlay', (payload: ControllerSnapshot) => {
            overlayState = payload;
            setLocale(payload.locale);
        }).then((stop: () => void) => stoppers.push(stop));
        void onOverlayPointerHover((hovering: boolean) => {
            pointerOverControls = hovering;
        }).then((stop: () => void) => stoppers.push(stop));
        window.addEventListener('resize', reportInteractiveHeight);
        return () => {
            for (const stop of stoppers) stop();
            window.removeEventListener('resize', reportInteractiveHeight);
        };
    });

    $effect(() => {
        void failedRouteCount;
        reportInteractiveHeight();
    });

    // Clicks pass through the box, so a failure notice with buttons in it has to claim
    // a strip below the top edge or the user cannot reach its own actions.
    function reportInteractiveHeight() {
        const box = document.querySelector('.overlay-shell')?.getBoundingClientRect();
        const height = Math.ceil(Math.max(0, ...[...document.querySelectorAll('.route-failure')]
            .map(element => element.getBoundingClientRect().bottom - (box?.top ?? 0) + NOTICE_MARGIN)));
        if (height === reportedHeight) return;
        reportedHeight = height;
        void setOverlayInteractiveHeight(height);
    }

    function onPointerDown() {
        void dismissOverlayChrome();
    }

    function beginHandle(event: PointerEvent, corner?: string) {
        if (event.button !== 0) return;
        event.preventDefault();
        (event.currentTarget as Element).setPointerCapture(event.pointerId);
        dragging = true;
        void dragOverlay(true, corner ?? null);
    }

    function endHandle() {
        if (!dragging) return;
        dragging = false;
        void dragOverlay(false);
    }

    function send(action: OverlayAction) {
        void sendControllerAction(action);
    }
</script>

<svelte:document onpointerdown={onPointerDown} />

{#if overlayState}
    <main
        class="overlay-shell"
        class:pointer-over-controls={pointerOverControls}
        style:--active-route-count={merged ? 1 : Math.max(activeRouteCount, 1)}
        style:--overlay-opacity={overlayState.config.opacity}
        style:--font-scale={overlayState.config.fontScale}
    >
        <OverlaySubtitles
            state={overlayState}
            {merged}
            {send}
            onOpenSettings={() => void showSettingsWindow()}
        />
    </main>
    {#each EDGES as edge}
        <div
            class="overlay-handle"
            role="separator"
            aria-label={t('moveOverlay')}
            data-edge={edge}
            onpointerdown={(event) => beginHandle(event)}
            onpointerup={endHandle}
            onpointercancel={endHandle}
        ></div>
    {/each}
    {#each CORNERS as corner}
        <div
            class="overlay-handle"
            role="separator"
            aria-label={t('resizeOverlay')}
            data-edge={corner}
            onpointerdown={(event) => beginHandle(event, corner)}
            onpointerup={endHandle}
            onpointercancel={endHandle}
        ></div>
    {/each}
{/if}
