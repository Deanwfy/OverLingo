<script lang="ts">
    import { onMount } from 'svelte';
    import type { ControllerSnapshot, OverlayAction, OverlaySettingsPatch } from './app/types';
    import OverlaySettingsPanel from './components/OverlaySettingsPanel.svelte';
    import OverlayToolbar from './components/OverlayToolbar.svelte';
    import { setLocale } from './core/locale.svelte';
    import {
        onOverlayOutsideClick,
        onOverlayPointerAt,
        onOverlayPointerHover,
        onOverlaySettingsOpen,
        sendControllerAction,
        setOverlayChromeSize,
        setOverlaySettingsOpen,
        showSettingsWindow,
        subscribeController,
    } from './core/runtime.js';

    // One page serves both control windows; the backend opens the panel's with ?view=panel.
    // The browser preview mounts both in one page and says which is which instead.
    let { view = new URLSearchParams(location.search).get('view') === 'panel' ? 'panel' : 'toolbar' }: {
        view?: 'toolbar' | 'panel';
    } = $props();

    let chromeState = $state<ControllerSnapshot | null>(null);
    let settingsOpen = $state(false);
    let pointerOverControls = $state(false);
    let shell = $state<HTMLElement | null>(null);

    onMount(() => {
        const stoppers: Array<() => void> = [];
        void subscribeController(view, (payload: ControllerSnapshot) => {
            chromeState = payload;
            setLocale(payload.locale);
        }).then((stop: () => void) => stoppers.push(stop));
        void onOverlaySettingsOpen((open: boolean) => {
            settingsOpen = open;
        }).then((stop: () => void) => stoppers.push(stop));
        if (view === 'panel') {
            void onOverlayOutsideClick(closeSettings).then((stop: () => void) => stoppers.push(stop));
            window.addEventListener('keydown', dismissOnEscape);
        } else {
            void onOverlayPointerHover((hovering: boolean) => {
                pointerOverControls = hovering;
            }).then((stop: () => void) => stoppers.push(stop));
            void onOverlayPointerAt(markHovered).then((stop: () => void) => stoppers.push(stop));
        }
        return () => {
            for (const stop of stoppers) stop();
            window.removeEventListener('keydown', dismissOnEscape);
        };
    });

    // The window is sized to the content, so every change in it is reported; the
    // toolbar also says where its settings button is, for the panel to hang under it.
    $effect(() => {
        if (!shell) return;
        const observer = new ResizeObserver(() => {
            const button = shell!.querySelector<HTMLElement>('button[aria-expanded]');
            void setOverlayChromeSize({
                width: shell!.offsetWidth,
                height: shell!.offsetHeight,
                anchor: button ? button.offsetLeft + button.offsetWidth / 2 : undefined,
            });
        });
        observer.observe(shell);
        return () => observer.disconnect();
    });

    // :hover drawn by hand from the cursor position the backend reports.
    let hovered: Element | null = null;

    function markHovered(point: [number, number] | null) {
        const next = point ? document.elementFromPoint(point[0], point[1])?.closest('button') ?? null : null;
        if (next === hovered) return;
        hovered?.classList.remove('hover');
        next?.classList.add('hover');
        hovered = next;
    }

    function toggleSettings() {
        if (!settingsOpen) send({ type: 'requestCaptureOptions' });
        void setOverlaySettingsOpen(!settingsOpen);
    }

    function closeSettings() {
        if (settingsOpen) void setOverlaySettingsOpen(false);
    }

    function dismissOnEscape(event: KeyboardEvent) {
        if (event.key === 'Escape') closeSettings();
    }

    function send(action: OverlayAction) {
        void sendControllerAction(action);
    }

    function update(patch: OverlaySettingsPatch) {
        if (!chromeState) return;
        if (patch.showOriginal === false && !chromeState.config.showTranslation) {
            patch.showTranslation = true;
        }
        if (patch.showTranslation === false && !chromeState.config.showOriginal) {
            patch.showOriginal = true;
        }
        chromeState = {
            ...chromeState,
            config: { ...chromeState.config, ...patch },
        };
        send({ type: 'settings', patch });
    }

    async function openMoreSettings() {
        closeSettings();
        await showSettingsWindow();
    }
</script>

{#if chromeState}
    <main
        class="overlay-chrome-shell"
        class:revealed={view === 'panel' || pointerOverControls || settingsOpen}
        data-view={view}
        bind:this={shell}
    >
        {#if view === 'panel'}
            <OverlaySettingsPanel state={chromeState} {send} {update} onMoreSettings={openMoreSettings} />
        {:else}
            <OverlayToolbar state={chromeState} {settingsOpen} onToggleSettings={toggleSettings} {send} {update} />
        {/if}
    </main>
{/if}
