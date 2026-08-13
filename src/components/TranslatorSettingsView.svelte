<script lang="ts">
    import type { AppState } from '../app/state.svelte';
    import { docsUrl, providers } from '../engines/registry.js';
    import { openExternal } from '../core/runtime.js';
    import Icon from './Icon.svelte';

    let { state: app }: { state: AppState } = $props();
    const providerOptions: string[] = providers();
    let selectedProvider = $state(providerOptions[0]);
    let keys = $state<Record<string, string>>({});

    const mark = (provider: string) => provider.charAt(0).toUpperCase();
    const entered = (provider: string) => keys[provider]?.trim() ?? '';

    async function saveKey(provider: string) {
        if (!await app.saveCredential(provider, entered(provider))) return;
        keys[provider] = '';
    }

    async function clearKey(provider: string) {
        if (!await app.clearCredential(provider)) return;
        keys[provider] = '';
    }

    function help(topic: string) {
        const url = docsUrl(selectedProvider, topic, app.resolvedLocale);
        if (url) openExternal(url);
    }
</script>

<div class="preference-view translator-view">
    <header class="preference-heading">
        <h1>{app.text('translators')}</h1>
    </header>

    <section class="translator-workspace">
        <aside class="translator-sidebar">
            <nav aria-label={app.text('translatorList')}>
                {#each providerOptions as provider}
                    <button
                        class:active={selectedProvider === provider}
                        aria-current={selectedProvider === provider ? 'page' : undefined}
                        onclick={() => selectedProvider = provider}
                    >
                        <span class="provider-mark" class:qwen={provider === 'qwen'}>{mark(provider)}</span>
                        <span>
                            <strong>{app.text(provider)}</strong>
                            <small class:ready={app.credentials[provider]}>
                                <i></i>{app.text(app.credentials[provider] ? 'configured' : 'notConfigured')}
                            </small>
                        </span>
                        <span class="provider-chevron">›</span>
                    </button>
                {/each}
            </nav>
        </aside>

        <div class="translator-detail">
            <header class="provider-heading">
                <span class="provider-mark large" class:qwen={selectedProvider === 'qwen'}>{mark(selectedProvider)}</span>
                <div>
                    <h2>{app.text(selectedProvider)}</h2>
                    <p>{app.text(`${selectedProvider}Description`)}</p>
                    {#if docsUrl(selectedProvider, 'languages', app.resolvedLocale)}
                        <button class="doc-link" onclick={() => help('languages')}>
                            {app.text('supportedLanguages')}<Icon name="external" size={11} />
                        </button>
                    {/if}
                </div>
            </header>

            <section class="provider-form">
                {#if selectedProvider === 'qwen'}
                    <label class="provider-row">
                        <span><strong>{app.text('qwenRegion')}</strong></span>
                        <select
                            value={app.config.qwen.region}
                            disabled={app.translationActive}
                            onchange={(event) => app.updateQwen('region', event.currentTarget.value as typeof app.config.qwen.region)}
                        >
                            <option value="beijing">{app.text('beijing')}</option>
                            <option value="singapore">{app.text('singapore')}</option>
                        </select>
                    </label>
                    <div class="provider-row input-row">
                        <span>
                            <strong>{app.text('workspaceId')}</strong>
                            <button class="doc-link" onclick={() => help('workspaceId')}>
                                {app.text('whereToFind')}<Icon name="external" size={11} />
                            </button>
                        </span>
                        <input
                            value={app.config.qwen.workspaceId}
                            disabled={app.translationActive}
                            autocomplete="off"
                            spellcheck="false"
                            placeholder="llm-…"
                            oninput={(event) => app.updateQwen('workspaceId', event.currentTarget.value)}
                        />
                    </div>
                {/if}

                <div class="provider-row credential-setting">
                    <span>
                        <strong>API Key</strong>
                        <button class="doc-link" onclick={() => help('apiKey')}>
                            {app.text('getApiKey')}<Icon name="external" size={11} />
                        </button>
                    </span>
                    <div>
                        <input
                            type="text"
                            bind:value={keys[selectedProvider]}
                            disabled={app.translationActive}
                            autocomplete="off"
                            spellcheck="false"
                            placeholder="sk-…"
                            aria-label={`${app.text(selectedProvider)} API Key`}
                        />
                        <button
                            class="secondary-button"
                            disabled={app.translationActive || !entered(selectedProvider)}
                            onclick={() => saveKey(selectedProvider)}
                        >{app.text('save')}</button>
                        {#if app.credentials[selectedProvider]}
                            <button
                                class="secondary-button destructive"
                                disabled={app.translationActive}
                                onclick={() => clearKey(selectedProvider)}
                            >{app.text('clearKey')}</button>
                        {/if}
                    </div>
                </div>
            </section>
        </div>
    </section>
</div>
