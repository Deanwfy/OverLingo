<script lang="ts">
    import type { AppState } from '../app/state.svelte';
    import Icon from './Icon.svelte';

    let { state }: { state: AppState } = $props();

    const update = $derived(state.update);

    const detail = $derived.by(() => {
        switch (update.stage) {
            case 'checking':
                return state.text('checkingForUpdates');
            case 'upToDate':
                return state.text('upToDate');
            case 'available':
                return state.text('updateAvailable', { version: update.version ?? '' });
            case 'downloading':
                return update.progress === null
                    ? state.text('downloadingUpdate')
                    : state.text('downloadingUpdatePercent', { percent: update.progress });
            case 'ready':
                return state.text('updateReady');
            default:
                return '';
        }
    });

</script>

<footer class="about-footer">
    <span class="about-text">
        <span class="about-version">
            OverLingo {update.currentVersion}
            <button
                class="glyph-button"
                aria-label={state.text('checkForUpdatesNow')}
                title={state.text('checkForUpdatesNow')}
                disabled={state.updateBusy}
                onclick={() => state.checkForUpdates()}
            >
                <span class="glyph" class:spinning={update.stage === 'checking'}><Icon name="retry" size={11} /></span>
            </button>
        </span>
        {#if detail}
            <small class="about-detail">
                {detail}
                {#if state.updatePending && update.version}
                    <button class="doc-link" onclick={() => state.openUpdatePage(`/releases/tag/v${update.version}`)}>
                        {state.text('releaseNotes')}
                    </button>
                {/if}
                {#if update.stage === 'available'}
                    <!-- An update the running copy cannot apply in place leaves the download
                         page as the only route, so the link changes what it does rather than going. -->
                    {#if update.installable}
                        <button class="doc-link" onclick={() => state.downloadUpdate()}>
                            {state.text('updateNow')}
                        </button>
                    {:else}
                        <button class="doc-link" onclick={() => state.openUpdatePage('/releases/latest')}>
                            {state.text('downloadUpdate')}<Icon name="external" size={10} />
                        </button>
                    {/if}
                {:else if update.stage === 'ready'}
                    <button class="doc-link" onclick={() => state.installUpdate()}>
                        {state.text('installAndRestart')}
                    </button>
                {/if}
            </small>
        {/if}
        {#if update.stage === 'failed'}
            <small class="about-alert">{state.text('updateFailed')}</small>
        {:else if update.stage === 'ready' && update.error}
            <small class="about-alert">{state.text('updateInstallFailed')}</small>
        {/if}
        {#if update.stage === 'ready' && state.translationActive}
            <small class="about-alert">{state.text('updateEndsTranslation')}</small>
        {/if}
    </span>
    <button
        class="glyph-button about-repository"
        aria-label={state.text('viewOnGitHub')}
        title={state.text('viewOnGitHub')}
        onclick={() => state.openUpdatePage()}
    >
        <Icon name="github" size={24} />
    </button>
</footer>
