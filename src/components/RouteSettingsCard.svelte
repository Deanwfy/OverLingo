<script lang="ts">
    import type { ControllerSnapshot, OverlayAction, RouteConfig, RouteId } from '../app/types';
    import { t } from '../core/locale.svelte';
    import { LANGUAGES, languageName } from '../core/languages.js';
    import { supportsLanguage, translator, translators } from '../core/translators.js';
    import Icon from './Icon.svelte';

    let { routeId, state, send, starting, enabledCount }: {
        routeId: RouteId;
        state: ControllerSnapshot;
        send: (action: OverlayAction) => void;
        starting: boolean;
        enabledCount: number;
    } = $props();

    let route = $derived(state.routes[routeId].config);
    let selectedApplication = $derived(state.audio.system.application);
    let selectedMicrophone = $derived(state.audio.microphone.device);

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

    function patchRoute(patch: Partial<RouteConfig>) {
        Object.assign(state.routes[routeId].config, patch);
        send({ type: 'routeSettings', routeId, patch });
    }

    function toggleRoute() {
        const enabled = state.routes[routeId].config.enabled !== false;
        if (starting || (enabled && enabledCount === 1)) return;
        state.routes[routeId].config.enabled = !enabled;
        send({ type: 'route', routeId, enabled: !enabled });
    }

    function updateLanguage(field: 'sourceLanguage' | 'targetLanguage', value: string) {
        const route = state.routes[routeId].config;
        const other = field === 'sourceLanguage' ? 'targetLanguage' : 'sourceLanguage';
        patchRoute(
            value === route[other] ? { [field]: value, [other]: route[field] } : { [field]: value },
        );
    }

    function swapLanguages() {
        const route = state.routes[routeId].config;
        patchRoute({
            sourceLanguage: route.targetLanguage,
            targetLanguage: route.sourceLanguage,
        });
    }

    // The language pair is what the user actually asked for, so switching translator never
    // rewrites it. An unsupported combination is left visible until the user resolves it.
    function updateTranslator(model: string) {
        if (model !== state.routes[routeId].config.model) patchRoute({ model });
    }
</script>

<section
    class="source-settings-card"
    class:disabled={!route.enabled}
    data-route={routeId}
>
    <header>
        <span>
            <Icon name={routeId === 'system' ? 'speaker' : 'mic'} size={14} />
            {t(routeId === 'system' ? 'systemAudio' : 'microphone')}
        </span>
        <label class="switch">
            <input
                type="checkbox"
                checked={route.enabled}
                disabled={starting || (route.enabled && enabledCount === 1)}
                aria-label={t(routeId === 'system' ? 'systemAudio' : 'microphone')}
                onchange={toggleRoute}
            />
            <i></i>
        </label>
    </header>

    {#if routeId === 'system' && state.capture.capabilities.applicationCapture}
        <label class="compact-select full-width">
            <span>{t('audioFrom')}</span>
            <select
                class="compact"
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
                class="compact"
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
                class="compact"
                value={route.sourceLanguage}
                disabled={!route.enabled || starting}
                onchange={(event) => updateLanguage('sourceLanguage', event.currentTarget.value)}
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
            onclick={swapLanguages}
        ><Icon name="swap" size={14} /></button>
        <label class="compact-select">
            <span>{t('targetLanguage')}</span>
            <select
                class="compact"
                value={route.targetLanguage}
                disabled={!route.enabled || starting}
                onchange={(event) => updateLanguage('targetLanguage', event.currentTarget.value)}
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
            class="compact"
            value={route.model}
            disabled={!route.enabled || starting || !configured.length}
            onchange={(event) => updateTranslator(event.currentTarget.value)}
        >
            {#each translatorOptions(route) as entry}
                <option value={entry.id}>{entry.name}</option>
            {/each}
        </select>
    </label>
</section>
