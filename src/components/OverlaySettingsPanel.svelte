<script lang="ts">
    import type {
        ControllerSnapshot,
        OverlayAction,
        OverlayLayout,
        OverlaySettingsPatch,
        RouteConfig,
        RouteId,
    } from '../app/types';
    import { t } from '../core/locale.svelte';
    import { LANGUAGES, languageName } from '../core/languages.js';
    import { supportsLanguage, translator, translators } from '../engines/registry.js';
    import Icon from './Icon.svelte';

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

    let configured = $derived(
        translators().filter(entry => state.credentials[entry.provider]),
    );

    // Holding a key is not part of the route, so hiding a translator without one costs no
    // reachability — the user adds the key first either way. Lacking the route's languages
    // is a relation between route fields, so those stay selectable and merely say so;
    // forbidding them would strand any pair the current translator cannot serve.
    function translatorOptions(route: RouteConfig) {
        if (!configured.length) {
            return [{ id: route.model, name: t('noTranslator') }];
        }
        const options = configured.map(entry => ({
            id: entry.id,
            name: entry.name + (supportsPair(entry.id, route) ? '' : ` · ${t('unsupportedLanguages')}`),
        }));
        // Clearing a key must not leave the select blank on the route still using it.
        if (!options.some(entry => entry.id === route.model)) {
            options.push({
                id: route.model,
                name: `${translator(route.model)?.name ?? route.model} · ${t('notConfigured')}`,
            });
        }
        return options;
    }

    function supportsPair(model: string, route: RouteConfig) {
        return supportsLanguage(model, route.sourceLanguage)
            && supportsLanguage(model, route.targetLanguage);
    }

    // Filtered to what the translator accepts, except for the value the route already
    // holds: switching translator must not blank the select, and seeing the offending
    // language is how the user knows what to fix. Reachability is preserved by the
    // translator picker, which is never restricted — switch first, then fix the language.
    function languageOptions(route: RouteConfig, current: string) {
        return LANGUAGES
            .filter(language => language.code === current
                || supportsLanguage(route.model, language.code))
            .map(language => ({
                code: language.code,
                name: languageName(language.code, state.locale)
                    + (supportsLanguage(route.model, language.code)
                        ? ''
                        : ` · ${t('unsupportedByTranslator')}`),
            }))
            .sort((left, right) => left.name.localeCompare(right.name, state.locale));
    }

    function patchRoute(routeId: RouteId, patch: Partial<RouteConfig>) {
        Object.assign(state.routes[routeId].config, patch);
        send({ type: 'routeSettings', routeId, patch });
    }

    function toggleRoute(routeId: RouteId) {
        const enabled = state.routes[routeId].config.enabled !== false;
        if (starting || (enabled && enabledCount === 1)) return;
        state.routes[routeId].config.enabled = !enabled;
        send({ type: 'route', routeId, enabled: !enabled });
    }

    function updateLanguage(
        routeId: RouteId,
        field: 'sourceLanguage' | 'targetLanguage',
        value: string,
    ) {
        const route = state.routes[routeId].config;
        const other = field === 'sourceLanguage' ? 'targetLanguage' : 'sourceLanguage';
        patchRoute(
            routeId,
            value === route[other] ? { [field]: value, [other]: route[field] } : { [field]: value },
        );
    }

    function swapLanguages(routeId: RouteId) {
        const route = state.routes[routeId].config;
        patchRoute(routeId, {
            sourceLanguage: route.targetLanguage,
            targetLanguage: route.sourceLanguage,
        });
    }

    // The language pair is what the user actually asked for, so switching translator never
    // rewrites it. An unsupported combination is left visible until the user resolves it.
    function updateTranslator(routeId: RouteId, model: string) {
        if (model !== state.routes[routeId].config.model) patchRoute(routeId, { model });
    }

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
            {@const route = state.routes[routeId].config}
            {@const selectedApplication = state.audio.system.application}
            {@const selectedMicrophone = state.audio.microphone.device}
            <section
                class="source-settings-card"
                class:disabled={!route.enabled}
                class:system={routeId === 'system'}
            >
                <header>
                    <span>
                        <Icon name={routeId === 'system' ? 'speaker' : 'mic'} size={14} />
                        {t(routeId === 'system' ? 'systemAudio' : 'microphone')}
                    </span>
                    <label class="mini-switch">
                        <input
                            type="checkbox"
                            checked={route.enabled}
                            disabled={starting || (route.enabled && enabledCount === 1)}
                            aria-label={t(routeId === 'system' ? 'systemAudio' : 'microphone')}
                            onchange={() => toggleRoute(routeId)}
                        />
                        <i></i>
                    </label>
                </header>

                {#if routeId === 'system' && state.capture.capabilities.applicationCapture}
                    <label class="compact-select full-width">
                        <span>{t('audioFrom')}</span>
                        <select
                            value={state.audio.system.scope === 'application' ? selectedApplication?.bundleId : 'all'}
                            disabled={!route.enabled || state.capture.loading || starting}
                            onchange={(event) => send({ type: 'capture', bundleId: event.currentTarget.value })}
                        >
                            <option value="all">{t('allComputerAudio')}</option>
                            {#if state.audio.system.scope === 'application' && selectedApplication && !state.capture.applications.some(app => app.bundleId === selectedApplication.bundleId)}
                                <option value={selectedApplication.bundleId}>{selectedApplication.name} · {t('unavailable')}</option>
                            {/if}
                            {#each state.capture.applications as application}
                                <option value={application.bundleId}>{application.name}</option>
                            {/each}
                        </select>
                    </label>
                {/if}

                {#if routeId === 'microphone'}
                    <label class="compact-select full-width">
                        <span>{t('microphoneFrom')}</span>
                        <select
                            value={selectedMicrophone ?? 'default'}
                            disabled={!route.enabled || state.capture.loading || starting}
                            onchange={(event) => send({ type: 'microphoneDevice', device: event.currentTarget.value })}
                        >
                            <option value="default">{t('defaultMicrophone')}</option>
                            {#if selectedMicrophone && !state.capture.microphones.includes(selectedMicrophone)}
                                <option value={selectedMicrophone}>{selectedMicrophone} · {t('unavailable')}</option>
                            {/if}
                            {#each state.capture.microphones as microphone}
                                <option value={microphone}>{microphone}</option>
                            {/each}
                        </select>
                    </label>
                {/if}

                <div class="language-pair-mini">
                    <label class="compact-select">
                        <span>{t('sourceLanguage')}</span>
                        <select
                            value={route.sourceLanguage}
                            disabled={!route.enabled || starting}
                            onchange={(event) => updateLanguage(routeId, 'sourceLanguage', event.currentTarget.value)}
                        >
                            {#each languageOptions(route, route.sourceLanguage) as language}
                                <option value={language.code}>{language.name}</option>
                            {/each}
                        </select>
                    </label>
                    <button
                        class="swap-mini"
                        disabled={!route.enabled || starting}
                        title={t('swapLanguages')}
                        aria-label={t('swapLanguages')}
                        onclick={() => swapLanguages(routeId)}
                    ><Icon name="swap" size={14} /></button>
                    <label class="compact-select">
                        <span>{t('targetLanguage')}</span>
                        <select
                            value={route.targetLanguage}
                            disabled={!route.enabled || starting}
                            onchange={(event) => updateLanguage(routeId, 'targetLanguage', event.currentTarget.value)}
                        >
                            {#each languageOptions(route, route.targetLanguage) as language}
                                <option value={language.code}>{language.name}</option>
                            {/each}
                        </select>
                    </label>
                </div>

                <label class="compact-select translator-select">
                    <span>{t('translator')}</span>
                    <select
                        value={route.model}
                        disabled={!route.enabled || starting || !configured.length}
                        onchange={(event) => updateTranslator(routeId, event.currentTarget.value)}
                    >
                        {#each translatorOptions(route) as entry}
                            <option value={entry.id}>{entry.name}</option>
                        {/each}
                    </select>
                </label>
            </section>
        {/each}
    </div>

    <div class="overlay-display-settings">
        <div class="content-toggle">
            <span>{t('subtitleContent')}</span>
            <div>
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
            <div role="radiogroup" aria-label={t('subtitleLayout')}>
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
                oninput={(event) => update({ fontScale: Number(event.currentTarget.value) / 100 })}
            />
        </label>
        <label class="overlay-switch-row">
            <span>{t('clickThrough')}</span>
            <span class="mini-switch">
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
