<script lang="ts">
    import type {
        ControllerSnapshot,
        OverlayAction,
        OverlayLayout,
        OverlaySettingsPatch,
        RouteId,
    } from '../app/types';
    import { t } from '../core/locale.svelte';
    import Icon from './Icon.svelte';
    import RouteSettingsCard from './RouteSettingsCard.svelte';

    let { state, send, update, onMoreSettings }: {
        state: ControllerSnapshot;
        send: (action: OverlayAction) => void;
        update: (patch: OverlaySettingsPatch) => void;
        onMoreSettings: () => void;
    } = $props();

    const routeIds: RouteId[] = ['system', 'microphone'];
    let starting = $derived(state.translationState === 'starting');
    let enabledCount = $derived(
        routeIds.filter(routeId => state.routes[routeId].config.enabled !== false).length,
    );

    function toggleContent(field: 'showOriginal' | 'showTranslation') {
        update({ [field]: !state.config[field] });
    }

    const layouts: OverlayLayout[] = ['split', 'merged'];
    const layoutLabel: Record<OverlayLayout, 'layoutSplit' | 'layoutMerged'> = {
        split: 'layoutSplit',
        merged: 'layoutMerged',
    };
</script>

<section class="overlay-settings-panel" aria-label={t('overlaySettings')}>
    <div class="source-settings-grid">
        {#each routeIds as routeId}
            <RouteSettingsCard {routeId} {state} {send} {starting} {enabledCount} />
        {/each}
    </div>

    <div class="overlay-display-settings">
        <div class="content-toggle">
            <span>{t('subtitleContent')}</span>
            <div class="segmented compact fill">
                <button
                    class:active={state.config.showOriginal}
                    aria-pressed={state.config.showOriginal}
                    onclick={() => toggleContent('showOriginal')}
                >{t('showOriginal')}</button>
                <button
                    class:active={state.config.showTranslation}
                    aria-pressed={state.config.showTranslation}
                    onclick={() => toggleContent('showTranslation')}
                >{t('showTranslation')}</button>
            </div>
        </div>
        <div class="content-toggle">
            <span>{t('subtitleLayout')}</span>
            <div class="segmented compact fill" role="radiogroup" aria-label={t('subtitleLayout')}>
                {#each layouts as layout}
                    <button
                        role="radio"
                        class:active={state.config.layout === layout}
                        aria-checked={state.config.layout === layout}
                        onclick={() => update({ layout })}
                    >{t(layoutLabel[layout])}</button>
                {/each}
            </div>
        </div>
        <label class="overlay-range-row">
            <span>{t('opacity')} <output>{Math.round(state.config.opacity * 100)}%</output></span>
            <input
                type="range"
                min="0"
                max="100"
                step="1"
                value={state.config.opacity * 100}
                style:--fill={state.config.opacity}
                oninput={(event) => update({ opacity: Number(event.currentTarget.value) / 100 })}
            />
        </label>
        <label class="overlay-range-row">
            <span>{t('subtitleSize')} <output>{Math.round(state.config.fontScale * 100)}%</output></span>
            <input
                type="range"
                min="75"
                max="180"
                step="5"
                value={state.config.fontScale * 100}
                style:--fill={(state.config.fontScale * 100 - 75) / 105}
                oninput={(event) => update({ fontScale: Number(event.currentTarget.value) / 100 })}
            />
        </label>
        <label class="overlay-switch-row">
            <span>{t('clickThrough')}</span>
            <span class="switch">
                <input
                    type="checkbox"
                    checked={state.config.clickThrough}
                    onchange={() => update({ clickThrough: !state.config.clickThrough })}
                />
                <i></i>
            </span>
        </label>
    </div>

    <footer>
        <button class="more-settings" onclick={onMoreSettings}>
            <Icon name="settings" size={14} />{t('moreSettings')}<span>›</span>
        </button>
    </footer>
</section>
