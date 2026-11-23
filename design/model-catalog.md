# Model catalog: several models per provider, real language rules

Status: implemented, 2026-11-23.

## Why

Today each provider effectively has one model, and the catalog records one language list
per model. Three things break that:

- Alibaba ships `qwen3.8-livetranslate-flash-realtime` next to 3.5. It speaks a different
  wire protocol, and the next release offers both.
- Language support is not one list. OpenAI understands 70+ languages but translates into
  13. Speechmatics only pairs English with another language. Today all of these collapse
  into a single list, so OpenAI offers 13 source languages instead of 70+.
- The interface's language list (`src/core/languages.js`) is a hand-kept copy of Qwen's 60
  languages, and every model is cut down to it. Soniox supports languages the picker cannot
  show. Qwen's `null` ("any language") is only right because the two lists happen to match.

The goal is one general mechanism. Qwen 3.8 is its first user, not a special case.

## Out of scope

These were considered and set aside on 2026-11-23:

- **Cascades** (recognition from one service and text translation from another). They need
  either two API keys or local recognition, and local recognition means a separate stack
  per platform. The providers that offer both halves under one key (OpenAI, Alibaba)
  already sell end-to-end translation.
- **Soft language gating** (warn, but let a session start). Live subtitles fail mid-session
  rather than up front, so an unsupported pair stays blocked before connecting, as today.
- **Runtime discovery** of models or languages from provider APIs. Only Soniox exposes
  languages (DeepL and Speechmatics would later). It would cost a cache, a catalog pushed
  through the snapshot, and rules to stop a refresh from rewriting stored routes or ending
  sessions. Language lists rarely change: Soniox v5 to v6 changed none. Changes are tracked
  by hand and shipped in a release, so every install of one version behaves the same.
- **Automated watching** of provider docs and APIs in CI. The maintainer tracks providers
  through their own channels.
- **New providers.** DeepL Voice, Gemini Live Translate, Speechmatics, Gladia and
  Volcengine all fit the existing `providers::Event` contract. Each becomes one adapter
  plus catalog entries once this lands.

## What stays

- `src/core/translators.json` stays the single catalog, read by Rust (`include_str!`) and
  the interface (JSON import). It changes only in a release.
- `RouteConfig` keeps `{ engine, model, source_language, target_language }`. Stored config
  needs no migration.
- The `providers::Event` stream (`Ready`, `Fragment{Original|Translation}`, `Error`,
  `Closed`) stays the contract between providers and the controller. A provider that
  streams deltas joins them itself, as the contract already requires.
- Credentials stay one key per provider.
- The picker stays a flat list of models, filtered to providers that have a key.
- Retirement stays as it is: a release drops the entry, and `normalize_route` moves routes
  on it to the provider's default.

## 1. Catalog schema

Each entry gains a language rule, an optional protocol, and an explicit default flag. Lists
that several models share are written once, as named sets.

```json
{
    "languageSets": { "alibaba": ["af", "ar", "ast", "..."] },
    "models": [
        {
            "id": "qwen3.8-livetranslate-flash-realtime",
            "provider": "qwen",
            "name": "Qwen3.8 LiveTranslate Flash Realtime",
            "protocol": "qwen-livetranslate-3.8",
            "default": true,
            "languages": { "source": "alibaba", "target": "alibaba" }
        }
    ]
}
```

`languages` fields:

| Field | Meaning | `null` / absent |
|---|---|---|
| `source` | Codes the model can recognise: a list, or the name of a set | Any code |
| `target` | Codes the model can translate into: a list, or the name of a set | Any code |
| `pairs` | Map from target code to the source codes allowed for it | Any `source` may go to any `target` |

Sets are resolved once, where each side loads the catalog. An unknown set name fails the
load, so every test that touches the catalog catches it.

This covers every shape seen so far:

| Model | `source` | `target` | `pairs` |
|---|---|---|---|
| Qwen 3.5 | set `alibaba` (60) | set `alibaba` | — |
| Qwen 3.8 | set `alibaba` (60) | set `alibaba` | — |
| OpenAI translate | 73 inline | 13 inline | — |
| Soniox v6 | set `soniox` (60) | set `soniox` | — |
| Speechmatics (future) | list | list | `en`→others, others→`en` |

Both Qwen models share Alibaba's one table of 60 languages. 3.8 recognises all 60, although
it cannot be told which one to expect. Its docs say "recognises 60 source languages", and
the same 60 are valid targets for text output.

`null` is reserved for a model with no restriction of its own. No current model has one.

`pairs` mirrors the shape of Soniox's `translation_targets[].source_languages`, which is
general enough for pivot-language providers.

`protocol` is opaque to everything except the provider's adapter. A provider with one
protocol leaves it out. Each `Engine` says which tags it speaks (`speaks`), `open_session`
refuses a model whose tag its engine does not speak, and a test checks every catalog entry
against its engine.

`default` replaces "first entry of a provider", which made JSON order load-bearing. Exactly
one entry per provider carries it, and a test checks that.

## 2. Language codes and the interface's language list

**One code system.** The catalog uses standard BCP 47 codes, which means the ISO 639-1 code
where one exists (`nb` for Norwegian Bokmål, `fil` for Filipino, `yue` for Cantonese). A
provider that spells a language differently is translated in its adapter, on the way out
only. Soniox, for example, sends `nb` as `no` and `fil` as `tl`. The rest of the app never
sees provider spellings.

**The list is derived, not kept.** The interface's language list becomes the union of every
`source` and `target` list in the catalog. `languages.js` stops holding codes. A model added
to the catalog brings its languages into the picker with it, and each select still filters
to what the chosen model accepts.

**Names come from the platform.** `languageName` asks `Intl.DisplayNames`, cached per
locale. Every catalog code was measured to have a name in all six interface languages on
JavaScriptCore (the WKWebView engine) and V8, so no fallback table is kept. WebView2 runs
Chromium's V8 and ICU.

A test fails when any catalog code has no name, which catches a mistyped code.

The derived list lives in `translators.js`, the one JS module that reads the catalog.
`languages.js` re-exports it and only deals with names.

Effects:

- Soniox's languages the picker could not show become available for Soniox routes: `cy`,
  `eu`, `lt`, `sq`, `sr`, `ta` and `te`, plus `nb` and `fil` through the code mapping. This
  supersedes the decision of 2026-11-05 to leave them out.
- OpenAI's source list can grow past the 60 (section 3).

## 3. OpenAI source languages

OpenAI translates into 13 languages but recognises 70+, and it detects the source itself.
Today its source list is cut to the 13. With `source` and `target` split, `source` takes
the full set.

The cookbook lists the 70+ by English name only, so mapping them to codes is a one-off
task. A few names need a judgement call, for example "Norwegian" as `nb` and "Chinese" as
`zh`. These judgements are recorded next to the list in `translators.rs`, where the notes
on catalog data already live.

## 4. One language check, used everywhere

`supports_language(model, code)` is replaced by:

- `Languages::allows` in Rust and `allows` in JS: are both codes in their sets, and does
  `pairs` allow the combination? `route_config_error` uses the Rust one, and the translator
  option label (`· unsupportedLanguages`) uses the JS one through `supportsPair`. Both run
  the same cases from `tests/fixtures/language-rules.json`, so they cannot drift apart
  without a test failing.
- `accepts(model, field, code)` in JS only: is `code` in that end's set? The pickers use
  it to filter each select.

The picker behaviour stays as it is. Each select filters to what the model accepts and
keeps the current value marked unsupported. A pair the model cannot serve is blocked with
`unsupportedLanguage` before connecting.

The source language works the same way for every model, whatever the model does with it.
It is the pair the user declares, and it is checked against the model's lists. Whether it
also goes over the wire is the adapter's business, decided by its protocol:

- Qwen 3.5 and Soniox send it as a hint.
- Qwen 3.8 and OpenAI detect the language themselves and have no field for it.

The interface does not show this difference.

Switching model while translating needs nothing new. `reconfigure_if_active` already
leaves the current session running when the new combination fails `route_config_error`,
and swaps make-before-break when it passes. The per-model rules go through that same
check, so they apply here too.

## 5. Several models per provider: protocol per model

`Engine.open` receives the model's protocol tag in `StartRequest`, so an adapter can
branch on it. Qwen parses it into a `Protocol` enum.

For Qwen, the two versions differ in two places, both established from the docs on
2026-11-23:

**`session.update`**

| | 3.5 | 3.8 |
|---|---|---|
| Output | `modalities: ["text"]` | `output_modalities: ["text"]` |
| Input format | `input_audio_format`, `sample_rate` | `audio.input.format.{type, sample_rate}` |
| Source language | `input_audio_transcription.language` | not available |
| Turn detection | `turn_detection` (server_vad) | `audio.input.turn_detection`; left on its default, `speaker_detection` (see below) |
| Target language | `translation.language` | same |

**Server events**

| | 3.5 | 3.8 |
|---|---|---|
| Original, partial | `conversation.item.input_audio_transcription.text` (`text` + `stash`) | `...input_audio_transcription.delta` (`delta`, appended) |
| Original, final | `...input_audio_transcription.completed` | same |
| Translation, partial | `response.text.text` | `response.text.delta` |
| Translation, final | `response.text.done` | `response.text.done` if sent; otherwise the accumulated text at `response.done` |

Only `session.update` branches on the protocol. The event names of the two versions do not
overlap, so one handler takes both. 3.8's deltas are joined by `providers::Joined`, which
the OpenAI adapter now uses too.

The endpoint, regions, Workspace ID, client events (`input_audio_buffer.append`,
`session.finish`) and the error shape are identical. They stay shared.

Found in live tests on 2026-11-23:

- 3.8 ends a turn on silence in the audio stream, in both turn-detection modes. The app
  streams silence too, so turns close as they do on 3.5.
- With synthetic speech, `server_vad` kept Chinese punctuation that `speaker_detection`
  dropped. On a real podcast over music, though, `server_vad` detected one 5 ms utterance
  and then nothing for 30 s. 3.8 stays on `speaker_detection`.
- 3.8 releases text a translation unit at a time, every 2 to 5 s, where 3.5 streams word
  by word and revises its tail. The output arrives in bursts by design.
- Audio that never falls silent can keep a segment open for half a minute, on 3.5 too.
  Switching model used to drop that open segment; the controller now keeps it as a turn
  when a session ends (`TurnAssembler::finish`).
- 3.8 also sends an undocumented `response.inference_unit_finished` within a turn, and a
  `speaker_id`. Both are ignored for now.

Region availability per model was a suspected gap. The docs list 3.8 in both Beijing and
Singapore, so regions stay per provider.

## 6. Changes by file

| File | Change |
|---|---|
| `src/core/translators.json` | New schema with named sets; Qwen 3.8 as the Qwen default; Soniox codes in BCP 47; OpenAI's full source list |
| `src-tauri/src/translators.rs` | Load and resolve the catalog; `Languages::allows`; `default` flag; `Engine.speaks`; protocol tag passed to `open` |
| `src-tauri/src/providers/mod.rs` | `Joined` for delta streams; a shared test collector |
| `src-tauri/src/providers/qwen_realtime.rs` | `Protocol`; per-protocol `session.update`; 3.8 events |
| `src-tauri/src/providers/openai_realtime.rs` | Uses `Joined` |
| `src-tauri/src/providers/soniox_realtime.rs` | Map `nb`→`no` and `fil`→`tl` when sending |
| `src/core/translators.js` | Resolves sets; derives `LANGUAGES`; `accepts`, `allows`, `supportsPair` |
| `src/core/languages.js` | Names from `Intl.DisplayNames` only |
| `src/components/RouteSettingsCard.svelte` | Per-field filtering; pair check for labels |
| `src/core/locales/*.js` | The Workspace ID notice no longer names Qwen 3.5 |
| `tests/fixtures/language-rules.json` | Pair-rule cases shared by Rust and JS |
| `README.md`, `README.zh-CN.md` | Model list |

`app_config.rs` and the controller need no change.

## 7. Verification

Done on 2026-11-23:

- Unit tests:
  - The shared pair-rule cases, in Rust and JS.
  - Exactly one `default` per provider, and every model's protocol spoken by its engine.
  - Every catalog code has a name in every interface language; the derived list has no
    repeats.
  - Soniox code mapping on the way out.
  - Qwen 3.5 and 3.8 `session.update` shapes; 3.8 deltas joined into whole segments,
    including closing at `response.done` without `response.text.done`.
- Live sessions with real keys, through the app's own `open_session`:
  - Qwen 3.5 and 3.8, en→zh and zh→en.
  - Soniox en→`nb` and en→`fil`, accepted after the mapping.
- The browser preview: the picker offers both Qwen models with 3.8 selected by default.
  OpenAI shows 73 sources and 13 targets, and Qwen shows 60 and 60.

Not done: the picker inside the real app on Windows.

## Decided

- **Default Qwen model: 3.8** (2026-11-23). New installs start on it. Existing routes keep
  the model they store, so nobody is moved off 3.5.
- **The source select is the same for every model** (2026-11-23). Whether a model takes
  the source language as a parameter is internal, and the user is not shown it.
- **Models and languages change only through releases** (2026-11-23). There is no runtime
  discovery and no automated watching.
- **The interface language list is derived from the catalog, and OpenAI's source list is
  widened** (2026-11-23).
