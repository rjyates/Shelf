# Device ideas

Ideas for the reader/notes session on the device: home screen, reading, highlights, notes, settings. Nothing here is decided. [product.md](../product.md) is the plan and wins on every conflict. An idea moves into product.md, or into an issue, only after we agree on it. Agents do not implement from this file.

Each idea names the phase it would land in and how it fits the locked choices. "No phase" means product.md has no slot for it yet. Adding one is a product.md change.

## Make it ours

We look at other readers (Kindle, Kobo, reMarkable, KOReader) to learn what people need from a reading device. We do not copy how they answered it. No borrowed feature names, screen layouts, icons, or wording. Each idea here is written as our own answer to a need, and the design work starts from that need.

What makes oseink different, and what every idea should lean on:

- Books and notes are equals. Notes are not a sub-menu of a book.
- Buttons alone do everything. Touch and stylus only add gestures.
- One UI for every panel. It branches on the capability profile, never on a device model.
- It works fully offline. Shelf is yours, on your server.
- The panel is e-ink, and the UI is designed for it: few redraws, calm screens, no animation.

## Home: Books and Notes

The idea we are building around. Home is a clean, quiet screen with two choices: **Books** and **Notes**. Pick one to open that library. Settings is a small third row at the bottom.

```text
┌──────────────────────────┐
│                          │
│   Books                  │
│   12 books · 2 in progress
│                          │
│   Notes                  │
│   3 notebooks · 48 marks │
│                          │
│                          │
│   Settings               │
└──────────────────────────┘
```

- Up and Down choose. Enter opens. Left goes back to home from either library. Touch and stylus tap the same rows.
- Each choice carries one quiet line of status, not a feed. No covers on home, so home redraws rarely and stays calm.
- Opening home is a `quality` frame. Moving between the two choices is `fast`.
- Phase 3 home has only Settings. Books arrives with phase 4 (local reader). Notes starts in phase 4 with highlights and typed notes, and gains notebooks in phase 12.
- Open question: should Books open straight to the last book, or always to the library first? One option is a single "Resume" row inside Books, not on home.

### The Books library

- A list by default, with a cover grid as a choice. On a small button-only panel, a list is quicker to move through and moving the highlight stays `fast`.
- Sort by recent, title, or author. Filter by unread, in progress, or finished.
- Progress shown as a thin bar on each row. The percentage is derived from the CFI and the current layout. It is not stored.
- Shelves: groups the user makes. A book can sit on more than one. Local in SQLite first. Syncing them needs a protocol type in phase 7 or later.
- Phase: 4 for a local folder. 6 and 7 for books from shelf.

### The Notes library

Everything you wrote or marked, in one place, whichever book it came from.

- **Notebooks**: standalone pen notebooks. Shown only when the profile has a stylus (phase 12).
- **Marks**: every highlight and note, grouped by book or listed by date. Enter on a mark opens the book at that spot (its CFI range).
- **Words**: words you looked up, with the sentence each came from. Depends on the dictionary idea below.
- On a button-only gray device, Notes still works: marks and words, no notebooks, no ink tools.
- A mark links both ways. From the book you can jump to the mark in Notes, and from Notes back into the book.
- Phase: 4 for marks. 7 when they sync. 12 for notebooks.

## Reading screen

### Footer you choose

The user picks what the footer shows: page in chapter, percent of book, time left in chapter, time left in book, or nothing. A button press cycles through them.

- Phase: 4. The plan already lists chapter page, position in book, title, and battery.
- Fit: "time left" needs a reading-speed estimate from page-turn timing, kept per user on the device. No server needed.

### Clock

An optional clock in the reading header.

- Phase: 4.
- Fit: a minute tick redraws a small rect. It must stay `fast` and must not count toward the clearing-refresh cadence, or the clock alone triggers flashes.

### Jump menu

Contents, start of book, a percentage or page, and your marks in one menu.

- Phase: 4.
- Fit: Readium gives the contents. Every entry is a row Up, Down, and Enter can reach.

### Glance

Hold a button to look at nearby pages, and let go to snap back. For maps, tables, and "who was that again".

- Phase: no phase. After phase 4.
- Fit: reads pages from the layout cache without moving the stored CFI. Holding a key needs key-repeat or key-release handling in the reader.

### Saved layouts

Save a font, size, margins, and spacing as a named layout, and switch with one row.

- Phase: 4.
- Fit: a layout is a typography spec, which is the page cache key. Switching drops the cache. The CFI keeps the place.

### Search in book

- Phase: no phase. After phase 4.
- Fit: Readium has search. Typing with buttons needs an on-screen keyboard driven by arrows. Search, typed notes, and Wi-Fi passwords all need that keyboard, so it may deserve its own issue.

### Invert

Light text on dark.

- Phase: 4.
- Fit: an inverted page ghosts more on `fast`. It may need a shorter clearing cadence while inverted.

### Rotate

Read sideways, with buttons remapped to match.

- Phase: no phase.
- Fit: the profile has width and height but no rotation.

## Words and context

### Define

Pick a word and see its meaning at the bottom of the page. Buttons move a word cursor. Touch presses and holds a word.

- Phase: no phase.
- Fit: needs an offline dictionary file and a license that lets us ship it, or lets the user add their own. An open format would avoid writing a parser. Open question.
- Every lookup is saved to **Words** in the Notes library.

### People and places

A list of the people, places, and terms in the book, with where each appears. Spoiler-safe by default: it shows only what comes before your current place.

- Phase: no phase.
- Fit: building the list is a server job, like alignment in phase 9. Shelf computes it once per book, and the device only displays it.

## Marks and ink

### Mark styles

A few mark styles in gray, which gain color on a color profile in phase 11.

- Phase: 4 for gray. 11 for color.
- Fit: the anchor is a CFI range plus a stable id, already locked. On a gray panel, styles are told apart by pattern (solid, underline, dotted), not by gray level alone.

### Note on a mark

Attach typed or handwritten text to a mark.

- Phase: typed after phase 4, once the on-screen keyboard exists. Handwritten in phase 12.
- Fit: stylus present: write. Stylus absent: type. A button-only device still needs a way to type.

### Margin ink that follows the text

With a stylus, write on the page. The ink stays with the words it sits next to, so it survives a font change.

- Phase: 12.
- Fit: the hard part is the anchor. A stroke stored only in screen position breaks on relayout, the same reason a mark is a CFI range. A stroke group could anchor to a CFI and store its offset from that text.

### Notebook pages

Blank, lined, grid, and dotted pages for standalone notebooks.

- Phase: 12.
- Fit: page backgrounds are static gray8. Ink is vector strokes, already in the plan.

### Pen tools

Pen, marker, eraser, and a select tool to move or delete strokes.

- Phase: 12.
- Fit: stylus only, hidden otherwise. Live ink needs the lowest-latency mode, so this tests the `fast` path hard.

### Export marks

A book's marks and notes as a plain Markdown file, from shelf's admin UI.

- Phase: no phase.
- Fit: product.md defers Readwise export. A plain file is smaller than that. Open question.

## Settings

### Reading

Clearing-refresh cadence (already planned: 1, 5, 10, 15, 30, never), default layout, footer, tap zones on or off, invert.

- Phase: 4.
- Fit: every row works with Up, Down, Enter, Left. Same screen code as phase 3 settings.

### Light warmth

Warmer or cooler light, maybe warmer at night.

- Phase: 12, if at all.
- Fit: the profile has `frontlight: yes | no`. Warmth would need a new profile value, and the profile is locked. Open question.

### Sleep picture

When the device sleeps, the panel keeps the cover of the book you are reading.

- Phase: deferred. Sleep waits for a board port.
- Fit: matches the deferred rule that sleep keeps the last finished frame. We would draw the cover as that frame.

## Ruled out

Listed so they are not proposed again by accident.

| Idea | Why it is out | Rule |
| --- | --- | --- |
| Other readers' popular highlights | Social reading is deferred. | product.md, Deferred |
| Streaks and badges | Achievements are deferred. | product.md, Deferred |
| Text-to-speech | Deferred. Read-along uses aligned narration (phase 8). | product.md, Deferred |
| Sync to outside reading services | Deferred. | product.md, Deferred |
| A store, recommendations, ads | Shelf does not ship books. | product.md, Locked choices |
| Online lookups (encyclopedia, translation) | The reader works with the radio off and no server. | product.md, Locked choices (Radio) |
| AI summaries | Same radio rule. Paid APIs are also out of budget. | product.md, Locked choices (Radio) |

## Open questions

- Does Books open to the library, or straight back into the last book?
- Is Settings a third row on home, or reached another way so home has only two choices?
- Where do Words live if the dictionary never ships: hide the row, or leave it out of Notes?
- Is Wi-Fi a toggle the user controls, or only "sync now"?
- Which offline dictionary format and source, and who supplies the file?
- Does light warmth justify a new profile value?
- One on-screen keyboard for search, notes, and Wi-Fi: who owns it, and in which phase?
