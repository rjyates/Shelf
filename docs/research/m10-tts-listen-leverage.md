# M10 TTS & listen — leverage and slice order

Reference: Book Orbit TTS UI under `web/orbit-reader/vendor/bookorbit/features/tts/`, Foliate `tts.js`, Storyteller for later audiobook sync. Shelf does not provision paid cloud TTS accounts (AGENTS.md).

## What already exists

| Piece | State |
| --- | --- |
| BO headphones / mini-player / resume / prefs UI | Vendored; Shelf mounts `TtsMiniPlayer` (+ MO mini-player) in `web/orbit-reader/src/App.vue` |
| Reader gate | Headphones for synthesize-TTS only when `GET /api/v1/tts/providers` returns an enabled OpenAI-compatible provider; media overlays still show listen when the EPUB has them |
| Journal Settings | `#settings` — add/enable/disable/remove providers (Kokoro voice preset checkbox) |
| Server providers + synthesize | SQLite `tts_providers`; proxy `POST /api/v1/tts/synthesize` to `{baseUrl}/audio/speech` |
| Chapter text | `GET /api/v1/tts/text/{book}/{chapter}` |
| Prefs / position | Browser localStorage stubs until SQLite TTS prefs |
| EPUB media overlays | **Web shipped** — Foliate plays embedded SMIL/audio from the whole-file EPUB; Shelf serves `GET /api/v1/epub/{id}/media-overlay` for resume timing. Streaming package files still deferred. |
| Mobile listen | Not started (Readium later) |

## Recommended M10 slice order (fly path)

1. ~~Web browser SpeechSynthesis~~ — **dropped**; BO does not use system TTS for the web player.
2. **Admin-configured OpenAI-compatible / Kokoro** — **shipped**: Settings configures providers; reader synthesizes via AudioContext.
3. **EPUB media overlays** — **shipped (web)**: playlist API + Foliate MO from the EPUB blob; BO UI prefers MO over TTS when present. **← current**
4. **Mobile / e-ink listen** — Readium TTS and/or media-overlay playback on the shared Flutter app; e-ink-friendly chrome.
5. **Storyteller-shaped extras** — sleep timer (BO already has UI), sync offset, PiP; separate audiobook + EPUB alignment when audiobook ingest exists.

## Do not reinvent

- A second reader chrome for listen — keep BO’s TTS / MO UI.
- Shipping Kokoro inside the Shelf image by default (opt-in / user-run only).
- Full Storyteller alignment before media overlays + mobile.

## Operator note

Run something OpenAI-compatible (e.g. `ghcr.io/remsky/kokoro-fastapi-cpu`) and register it under Settings with base URL like `http://127.0.0.1:8880/v1`. Until at least one provider is **enabled**, the EPUB reader hides the synthesize listen control.

Read-along EPUBs (Storyteller-style media overlays) play through Foliate once imported; no separate TTS provider is required for that path.

## References

- Upstream BO TTS: `client/src/features/tts/`
- Foliate TTS segmentation: `web/orbit-reader/public/assets/foliate/tts.js`
- Shelf API gate: `web/orbit-reader/vendor/bookorbit/lib/api.ts` (prefs/position local; providers + media-overlay hit server)
