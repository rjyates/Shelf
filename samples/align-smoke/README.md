# Align / Combine smoke fixture (CTC)

Short **owned** EPUB + matching TTS MP3 for proving Shelf’s Storyteller-parity
align path without waiting on a multi-hour audiobook.

## Files

| File | Notes |
| --- | --- |
| `Lamp_and_Rain.epub` / `.mp3` | Short (~18s) single-chapter packaging smoke |
| `Four_Lamps.epub` / `.mp3` | Multi-chapter (~6 min, 4 spine XHTML) for page-turn / desync QA |
| `Four_Lamps_script.txt` | Narration text used to regenerate the longer MP3 |

Rights: original Shelf wording + synthetic speech. Safe to keep in-tree.
Prefer **Four Lamps** for highlighting across chapter boundaries; **Lamp and Rain** for a fast SMIL/OPF check.

**CI gate:** `Four_Lamps.{epub,mp3}` is the required PR Combine regression
(`four-lamps-combine` / `scripts/four_lamps_combine_gate.py`) — full CTC prod
path via `compose.yaml` alone (ghost-story baked into the Shelf image), then
`readaloud-verify` asserts (manifest MO, `media:active-class`, `media:duration`,
SMIL timing, sentence spans). Keep both Lamp and Four Lamps fixtures in-tree;
do not delete either.

## How this maps to Storyteller’s testing

Storyteller ships **synthetic / public-domain unit tests** with the repo. Their
full regression **battery** (`tests/battery-*.json`) is **gitignored** — it pins
verdicts against copyrighted books the maintainers own and cannot redistribute.
The harness shape is documented so anyone can build a private battery from books
they own.

Shelf mirrors that split:

1. **This smoke** — redistributable, minutes on CTC CPU emit, used for
   `readaloud-verify` after Compose rebuilds.
2. **Private / long books** (e.g. RotK) — optional overnight Measure; not required
   to claim algorithm parity or packaging correctness.

## Run (Compose)

```sh
docker compose up -d
# Import both files via UI or API, then Manage → Merge & Combine / POST /api/v1/works
```

Then probe with the shelf skill’s `references/readaloud-verify.md`.

## Regenerate MP3

If the EPUB text changes, rebuild narration (host needs Docker):

```sh
# write script.txt from the EPUB body, then:
docker run --rm -v "$PWD/samples/align-smoke:/out" -v "$PWD:/in:ro" debian:bookworm-slim \
  bash -lc 'apt-get update -qq && apt-get install -y -qq espeak-ng ffmpeg >/dev/null \
  && espeak-ng -v en-us -s 140 -f /in/script.txt -w /tmp/o.wav \
  && ffmpeg -y -i /tmp/o.wav -codec:a libmp3lame -q:a 4 /out/Lamp_and_Rain.mp3'
```
