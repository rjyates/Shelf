# Atelier & InkFlow vs native Notes writing speed (Nomad)

Investigation date: 2026-11-16. Complements `docs/research/nomad-stylus.md`
(Shelf's SPEED+A2 Surface path). Nomad was **not** USB-attached this session, so
APK dumps of Atelier/InkFlow were not possible; conclusions below are from Ratta
docs, community reports, and open reimplementations of the same HandWrite binder
Notes/Atelier use.

## Verdict

| App | What "real-time" means | How it hits Notes-class tip speed |
| --- | --- | --- |
| **Notes** (OEM) | Wet ink on the e-ink panel | Firmware HandWrite daemon paints the EPDC overlay (`service_myservice` → panel). Not a normal Android Canvas refresh. |
| **Atelier** (Ratta store drawing app) | Same: wet ink on panel | Same OEM stack as Notes/Document — first-party HandWrite client + brush tooling. That is why it feels as fast as Notes. |
| **InkFlow** (Ratta store) | Lag felt on the **PC** display | Device becomes a USB graphics tablet; EMR streams to Windows/macOS. "Virtually no lag" is LCD-side, not e-ink mid-stroke. |
| **Shelf today** | Wet ink on Shelf's Surface | `InkHostView` + SPEED + A2 + tip lead ≈ **0.15–0.2 s** — framework approximation, not the HandWrite overlay. |
| **Sideloaded Concepts / plain Flutter** | App paints + panel GC | Often **500 ms+**; no firmware ink claim. |

**Implication for Shelf:** mimicking Atelier/Notes on-device means **claiming the
firmware ink overlay and not painting mid-stroke ourselves**. InkFlow is the wrong
reference for in-reader annotation — it deliberately leaves the e-ink wet-ink
problem.

---

## Atelier

### Product role

Atelier is Ratta's dedicated sketching app (layers, many pencils/inks/markers,
PSD export). It is distributed from the Supernote App Store, not Play Store.
Reviews treat its writing/drawing responsiveness as native-class: marks appear
where expected with no perceptible latency, same family of feel as Notes.

### Interaction model (why it is fast)

Atelier is not a clever third-party that figured out e-ink. It is a **Ratta
first-party** app on the same Chauvet stack as Notes. Native Document/Notes code
uses `com.ratta.supernote.eventlibrary.HandWriteClient` (and related Draw/PW
paths). The open clean-room clients that reimplement that binder document the
contract as:

| Piece | Value |
| --- | --- |
| Binder names | `service_myservice` (current), `service.myservice` (legacy) |
| Interface token | `android.demo.IMyService` |
| App role | Claim ownership, set pen/areas, clear overlay — **never draw live strokes** |
| Paint plane | Firmware → EPDC overlay / framebuffer at sub-frame latency |
| Known Nomad pen codes | Needle `10`, Ink `16`, Mark `11`, Calligraphy `15` |

So Atelier "mimics Notes" by **being the same class of client**: firmware owns
nib-to-ink; the app owns tools, layers, export, and stroke persistence after the
fact.

Community confirmation: sideloaded drawing apps stay laggy unless they adopt how
the built-in apps handle stylus events; Ratta's answer for "I need Procreate" is
Atelier on-device or InkFlow + a PC — not a hidden global "fast stylus" switch.

### What we did **not** verify this session

- Pull Atelier APK from device and confirm shared `HandWriteClient` / `.so`
  linkage (needs Nomad USB + installed Atelier).
- Brush/layer code beyond the binder pen codes above.

---

## InkFlow

### Product role

InkFlow turns Nomad/Manta into a **pressure-sensitive graphics tablet** for a
computer. Official manual: connect USB → operations sync in real time with
desktop drawing software. Ratta marketing: "virtually no lag" into Photoshop /
Illustrator / Clip Studio / browser PDF annotation. Sideloading must be off for
the USB tablet mode. Assistive dot matrix and aspect-ratio mapping are on-device
aids; the primary ink feedback is on the PC.

### Interaction model (why it feels fast)

InkFlow solves a different latency budget:

1. Sample EMR (Wacom layer) on the Nomad.
2. Stream tip/pressure/hover to the host as a tablet HID-like input.
3. Host GPU/LCD paints at 60 Hz+.

There is no requirement for A2/SPEED or HandWrite overlay quality on the e-ink
panel for the stroke the user is judging. Demo reviewers measure "instant" on
the Mac/PC cursor and brush, not panel settle. That is why community advice for
"Concepts is 500 ms on Nomad" points at InkFlow + second device rather than a
system toggle.

### Shelf takeaway

Do **not** chase InkFlow's architecture for EPUB annotation. It does not teach
on-panel wet ink. At most it is a future "use Nomad as a desk tablet" product
idea, unrelated to reader overlays.

---

## How this maps to Shelf's prior RE

`nomad-stylus.md` already correctly identified Notes tip feel as
`librecgnition` / `ThreadUpdateEpdc` / `HTEINK_IOC_DISPAREA` plus HandWrite /
PWCore, and correctly rejected shipping Notes `.so`s and using HandWrite as a
**pixel source for Shelf's Surface** (`realTimeHandWriting` ≠ our pixels).

What changed with open evidence (inkRead + plateaukao `supernote_draw`):

| Old Shelf reading | Updated reading |
| --- | --- |
| HandWrite binder as live paint → dead end | Dead end **only if** we expect HandWrite to fill *our* bitmap. |
| Sideload cannot get Notes latency | Sideload **can** claim `service_myservice` and let **firmware** paint wet ink (inkRead production path; penspike Route 5). |
| SPEED+A2 is the product ceiling | SPEED+A2 is the ceiling of the **Surface** strategy (~0.15–0.2 s). Firmware overlay is a separate lane (Notes/Atelier/inkRead pen). |
| `enableFullUiAuto` freezes UI without EMR | Still fragile; inkRead notes SELinux often no-ops it for untrusted apps — writable-area tx on the HandWrite binder is the lever that actually gates EMR ink for sideloads. |

inkRead's published limit table: waveform selection and dirty-rect refresh stay
platform-capped for sideloads; **live pen ink uses the same firmware overlay as
Notes**. Their highlighter lags because they disable that trajectory and paint
the band themselves — matching user reports of thick→thin handoff when baking
firmware ink into the app buffer.

---

## Recommended Shelf path (investigation → next spike)

1. **Keep** current SPEED+A2 host as fallback (`ratta_pw`) for when HandWrite is
   unavailable or ink mode must share Flutter UI badly.
2. **Spike** (device required): clean-room HandWrite client — claim app name,
   set Needle/Ink pen, clear disable areas, confirm wet ink under nib with
   **zero** `InkHostView` presents; on UP, persist from `MotionEvent` history into
   existing `InkStroke` / `nativeStrokes`, then `clearAll` / page-turn clear.
3. **Do not** reopen mid-stroke Flutter Canvas, FastPW, or Soft-DU-every-UP
   (still do-not-retread).
4. **Do not** treat InkFlow USB tablet mode as the reader ink solution.
5. License: reproduce **binder contract only** from AGPL clean-room sources
   (plateaukao / inkRead); do not copy their files wholesale into Shelf without
   a license decision.

### Device checklist when Nomad is plugged in

```bash
adb devices -l
adb shell pm list packages | rg -i 'atelier|inkflow|ratta|supernote'
# optional APK pull for linkage confirm:
# adb shell pm path <atelier.pkg> && adb pull …
adb shell service list | rg -i 'myservice|handwrite|eink'
```

Then run a minimal claim/teardown Activity (or inkRead's `:spike` APK) and film
nib→ink vs Shelf SPEED+A2 side by side.

---

## Sources (investigation)

See citation ledger for this task; primary open technical sources:

- inkRead `docs/EINK-LIMITS.md` — firmware overlay vs sideload refresh caps
- inkRead `SupernoteInk.kt` / penspike Route 5 — HandWrite binder client
- plateaukao `supernote_draw` `SupernoteInk.kt` — HandWriteClient clean-room
- Ratta InkFlow product post + Nomad manual ch.11 — graphics-tablet architecture
- r/Supernote Concepts lag thread — first-party vs sideload latency; InkFlow as
  Ratta's alternative
