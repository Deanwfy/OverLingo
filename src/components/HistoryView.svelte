<script lang="ts">
    import type { AppState } from '../app/state.svelte';
    import type { SessionExportMode } from '../app/types';
    import { formatDate, formatDuration } from '../app/format';
    import Icon from './Icon.svelte';

    const EXPORT_MODES: Array<{ mode: SessionExportMode; label: string }> = [
        { mode: 'source', label: 'exportSource' },
        { mode: 'target', label: 'exportTarget' },
        { mode: 'both', label: 'exportBoth' },
    ];

    let { state: app }: { state: AppState } = $props();
    let exportOpen = $state(false);
    let renaming = $state(false);
    let draftTitle = $state('');

    $effect(() => {
        void app.selectedHistory?.id;
        exportOpen = false;
        renaming = false;
    });

    function startRename() {
        draftTitle = app.selectedHistory?.title ?? '';
        renaming = true;
    }

    function commitRename() {
        if (!renaming) return;
        renaming = false;
        void app.renameHistory(draftTitle);
    }

    function onRenameKey(event: KeyboardEvent) {
        if (event.key === 'Enter') commitRename();
        else if (event.key === 'Escape') renaming = false;
    }

    function exportSession(mode: SessionExportMode) {
        exportOpen = false;
        void app.exportHistory(mode);
    }

    function focusField(node: HTMLInputElement) {
        node.focus();
        node.select();
    }
</script>

<svelte:document onclick={() => { exportOpen = false; }} />

<div class="history-view">
    <header class="page-heading">
        <h1>{app.text('history')}</h1>
    </header>

    <div class="history-layout">
        <aside class="history-sidebar">
            {#if app.historyLoading}
                <div class="history-placeholder">{app.text('loading')}</div>
            {:else if app.history.length === 0}
                <div class="history-placeholder">
                    <Icon name="history" size={24} />
                    <span>{app.text('noHistory')}</span>
                </div>
            {:else}
                {#each app.history as session}
                    <button
                        class:active={app.selectedHistory?.id === session.id}
                        class="history-item"
                        onclick={() => app.selectHistory(session)}
                    >
                        <strong>{session.title}</strong>
                        <span>{formatDate(session.created_at)} · {formatDuration(session.duration_sec)}</span>
                    </button>
                {/each}
            {/if}
        </aside>

        <article class="history-detail">
            {#if !app.selectedHistory}
                <div class="history-placeholder detail-placeholder">{app.text('selectSession')}</div>
            {:else}
                <header>
                    <div class="history-title">
                        {#if renaming}
                            <input
                                use:focusField
                                aria-label={app.text('rename')}
                                bind:value={draftTitle}
                                maxlength="120"
                                onblur={commitRename}
                                onkeydown={onRenameKey}
                            />
                        {:else}
                            <h2>{app.selectedHistory.title}</h2>
                        {/if}
                        <p>{formatDate(app.selectedHistory.created_at)} · {formatDuration(app.selectedHistory.duration_sec)}</p>
                    </div>
                    <div class="history-actions">
                        <div class="menu-anchor">
                            <button
                                class="icon-button"
                                aria-expanded={exportOpen}
                                aria-haspopup="menu"
                                aria-label={app.text('export')}
                                onclick={(event) => { event.stopPropagation(); exportOpen = !exportOpen; }}
                            >
                                <Icon name="export" size={18} />
                            </button>
                            {#if exportOpen}
                                <div class="popup-menu" role="menu">
                                    {#each EXPORT_MODES as option}
                                        <button role="menuitem" onclick={() => exportSession(option.mode)}>
                                            {app.text(option.label)}
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                        <button class="icon-button" aria-label={app.text('rename')} onclick={startRename}>
                            <Icon name="rename" size={18} />
                        </button>
                        <button class="icon-button destructive" aria-label={app.text('delete')} onclick={() => app.deleteHistory()}>
                            <Icon name="trash" size={18} />
                        </button>
                    </div>
                </header>
                <div class="history-turns">
                    {#each app.historySegments as segment}
                        <section class="history-turn">
                            <small>{segment.ts} · {app.routeName(segment.route_id)}</small>
                            {#if segment.src}<p>{segment.src}</p>{/if}
                            <strong>{segment.tgt}</strong>
                        </section>
                    {/each}
                </div>
            {/if}
        </article>
    </div>
</div>
