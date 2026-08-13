<script lang="ts">
    import { onMount } from 'svelte';
    import { AppState } from './app/state.svelte';
    import GeneralView from './components/GeneralView.svelte';
    import HistoryView from './components/HistoryView.svelte';
    import TopBar from './components/TopBar.svelte';
    import TranslatorSettingsView from './components/TranslatorSettingsView.svelte';

    const state = new AppState();

    onMount(() => {
        void state.init();
        return () => state.destroy();
    });
</script>

<svelte:head>
    <title>OverLingo</title>
</svelte:head>

{#if state.loading}
    <div class="launch-screen" aria-label={state.text('loading')}>
        <span></span>
    </div>
{:else}
    <div class="app-shell">
        <TopBar {state} />
        <main>
            {#if state.view === 'translation'}
                <TranslatorSettingsView {state} />
            {:else if state.view === 'history'}
                <HistoryView {state} />
            {:else}
                <GeneralView {state} />
            {/if}
        </main>
    </div>

    <div class:visible={Boolean(state.toastMessage)} class:error={state.toastIsError} class="toast" role="status" aria-live="polite">
        {state.toastMessage}
    </div>
{/if}
