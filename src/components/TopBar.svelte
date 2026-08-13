<script lang="ts">
    import type { AppState } from '../app/state.svelte';
    import type { MainView } from '../app/types';

    let { state }: { state: AppState } = $props();
    const tabs: Array<{ id: MainView; label: string }> = [
        { id: 'translation', label: 'translators' },
        { id: 'history', label: 'history' },
        { id: 'general', label: 'general' },
    ];
</script>

<header class="app-toolbar" data-tauri-drag-region>
    <div class="traffic-light-space" data-tauri-drag-region></div>

    <nav class="segmented-control primary-navigation" aria-label={state.text('navigation')}>
        {#each tabs as tab}
            <button
                class:active={state.view === tab.id}
                aria-current={state.view === tab.id ? 'page' : undefined}
                onclick={() => state.setView(tab.id)}
            >{state.text(tab.label)}</button>
        {/each}
    </nav>
    <div data-tauri-drag-region></div>
</header>
