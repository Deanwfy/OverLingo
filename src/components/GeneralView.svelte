<script lang="ts">
    import type { AppState } from '../app/state.svelte';

    let { state }: { state: AppState } = $props();
</script>

<div class="preference-view">
    <header class="preference-heading">
        <h1>{state.text('general')}</h1>
    </header>

    <section class="preference-card">
        <label class="settings-row">
            <span><strong>{state.text('language')}</strong></span>
            <select value={state.config.locale} onchange={(event) => state.updateLocale(event.currentTarget.value as typeof state.config.locale)}>
                <option value="auto">{state.text('automatic')}</option>
                <option value="en">English</option>
                <option value="es">Español</option>
                <option value="ja">日本語</option>
                <option value="ko">한국어</option>
                <option value="vi">Tiếng Việt</option>
                <option value="zh-Hans">简体中文</option>
            </select>
        </label>
        <label class="settings-row">
            <span>
                <strong>{state.text('launchAtLogin')}</strong>
                <small>{state.text('launchAtLoginHint')}</small>
            </span>
            <input
                class="native-switch"
                type="checkbox"
                checked={state.autostartEnabled}
                disabled={state.autostartLoading}
                onchange={(event) => state.updateAutostart(event.currentTarget.checked)}
            />
        </label>
    </section>
</div>
