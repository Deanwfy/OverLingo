<script lang="ts">
    import type {
        ControllerSnapshot,
        OverlayAction,
        RouteConfig,
        RouteId,
        RouteState,
    } from '../app/types';
    import { mergeTimeline } from '../app/timeline';
    import { t } from '../core/locale.svelte';
    import { translationDirection } from '../core/languages.js';
    import { readableRuntimeError } from '../core/errors.js';
    import Icon from './Icon.svelte';

    let { state, merged, send, onOpenSettings }: {
        state: ControllerSnapshot;
        merged: boolean;
        send: (action: OverlayAction) => void;
        onOpenSettings: () => void;
    } = $props();

    const routeIds: RouteId[] = ['system', 'microphone'];
    let timeline = $derived(merged ? mergeTimeline(state.routes, routeIds) : []);

    const readableError = (message: string) => readableRuntimeError(message, t);

    function direction(route: RouteConfig) {
        return translationDirection(route.sourceLanguage, route.targetLanguage, state.locale);
    }

    function routeStatus(state: RouteState) {
        if (state === 'connecting') return 'connecting';
        if (state === 'reconnecting') return 'reconnecting';
        if (state === 'reconfiguring') return 'applyingSettings';
        return '';
    }

    function routeIsEmpty(routeId: RouteId) {
        const route = state.routes[routeId];
        return !route || (route.turns.length === 0 && !route.draft.original && !route.draft.translation);
    }
</script>

{#snippet routeHeader(snapshot: ControllerSnapshot, routeId: RouteId)}
    {@const routeState = snapshot.routes[routeId]}
    <span class="route-direction overlay-reveal" data-route={routeId} lang={routeState.config.targetLanguage}>{direction(routeState.config)}</span>
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
                onclick={onOpenSettings}
            ><Icon name="settings" size={13} /></button>
        </span>
    {:else if routeStatus(routeState.state)}
        <small class="route-status">{t(routeStatus(routeState.state))}</small>
    {/if}
{/snippet}

{#snippet turn(snapshot: ControllerSnapshot, routeId: RouteId, original: string, translation: string, draft: boolean)}
    <article class="overlay-turn" class:draft data-route={routeId}>
        {#if snapshot.config.showOriginal && original}<p>{original}</p>{/if}
        {#if snapshot.config.showTranslation && translation}<strong>{translation}</strong>{/if}
    </article>
{/snippet}

<div class="overlay-routes" class:merged>
    {#if merged}
        <section class="overlay-route merged">
            <header>
                {#each routeIds as routeId}
                    {@render routeHeader(state, routeId)}
                {/each}
            </header>
            <div class="overlay-turns">
                {#each timeline as entry}
                    {@render turn(state, entry.routeId, entry.original, entry.translation, entry.draft)}
                {/each}
                {#if timeline.length === 0}
                    <p class="overlay-empty">{t('emptyOverlay')}</p>
                {/if}
            </div>
        </section>
    {:else}
        {#each routeIds as routeId}
            {@const routeState = state.routes[routeId]}
            {#if routeState?.config.enabled !== false}
                <section class="overlay-route">
                    <header>{@render routeHeader(state, routeId)}</header>
                    <div class="overlay-turns">
                        {#each routeState.turns as item}
                            {@render turn(state, routeId, item.original, item.translation, false)}
                        {/each}
                        {#if routeState.draft.original || routeState.draft.translation}
                            {@render turn(state, routeId, routeState.draft.original, routeState.draft.translation, true)}
                        {/if}
                        {#if routeIsEmpty(routeId)}
                            <p class="overlay-empty">{t('emptyOverlay')}</p>
                        {/if}
                    </div>
                </section>
            {/if}
        {/each}
    {/if}
</div>
