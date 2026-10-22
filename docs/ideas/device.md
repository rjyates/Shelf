# Device ideas

Ideas for the reader/notes session on the device: home screen, reading, highlights, notes, settings. Nothing here is decided. [product.md](../product.md) is the plan and wins on every conflict. An idea moves into product.md, or into an issue, only after we agree on it. Agents do not implement from this file.

Each idea lists where it came from, the phase it would land in, and how it fits the locked choices. "No phase" means product.md has no slot for it yet. Adding one is a product.md change.

Every idea has to pass the same rules the plan already sets: buttons alone can do it, touch and stylus only add gestures, body text stays gray, color waits for phase 11, and the UI branches on the capability profile, never on a device model.

## Home screen

### Continue reading

The book you were last in, large, at the top of home, with its cover and progress. One button press (Enter) opens it at the saved position.

- From: Kindle home "Jump back in" row.
- Phase: 4 (local reader). Phase 3 home has only a Settings row.
- Fit: needs the phase 4 position store. The cover is a thumbnail made at draw size, dithered once (Crosspoint reference, item 6).

### Recent books row

The next few recently opened books in a row under Continue reading. Left and Right move along it.

- From: Kindle home.
- Phase: 4.
- Fit: Left already means "back to home" on the settings screen. On home, Left and Right moving within a row needs a clear rule for leaving the row. Open question.

### Library: grid or list

A full library screen as a cover grid or a text list, with sort (recent, title, author) and a filter (all, unread, in progress, finished).

- From: Kindle library view options.
- Phase: 4 for a local folder. 6 and 7 for books from shelf.
- Fit: a list view works better than a grid on a small button-only panel. A grid redraw is a `quality` frame, while moving a list highlight can stay `fast`. Default may depend on panel size from the profile, not on a device model.

### Progress under each book

A thin bar or a percentage under each cover or list row. "New" for unopened books.

- From: Kindle library.
- Phase: 4.
- Fit: the percentage is derived from the CFI and the current layout, like the page number. It is a rendering, not stored state.

### Collections

User-made groups of books ("To read", "Programming", "Book club"). A book can be in more than one.

- From: Kindle collections.
- Phase: no phase. Local storage could arrive with phase 4. Syncing collections needs a protocol type in phase 7 or later.
- Fit: SQLite on the device. Field-level `updated_at` merge works for membership.

### Quick settings pull-down

One screen, reached by a button (or a top-edge swipe on touch), with Wi-Fi on/off, sync now, frontlight, and switch to desktop.

- From: Kindle "quick actions" pull-down.
- Phase: 4 for the menu. Frontlight is 12. Sync now is 7.
- Fit: the Crosspoint reference already reserves top-edge swipe down for Menu. Wi-Fi as a toggle sits awkwardly with "radio comes up to sync, then goes down". It may become "sync now" only. Open question.

## Reading screen

### Footer progress choices

The user picks what the footer shows: page in chapter, percent of book, time left in chapter, time left in book, or nothing. A button press cycles through them.

- From: Kindle reading progress options.
- Phase: 4. The plan already lists chapter page, position in book, title, and battery.
- Fit: "time left" needs a reading-speed estimate from page-turn timing, stored per user on the device. No server needed.

### Clock in the header

An optional clock in the reading header.

- From: Kindle.
- Phase: 4.
- Fit: a minute tick redraws a small rect. That must stay `fast` and must not count toward the clearing-refresh cadence, or the clock alone triggers flashes.

### Go To menu

Table of contents, beginning, a percentage or page, and your bookmarks, all in one menu.

- From: Kindle "Go To".
- Phase: 4.
- Fit: Readium gives the TOC. Every entry is a row Up, Down, and Enter can reach.

### Page Flip (peek without losing your place)

Look ahead or back at other pages, or a thumbnail strip of pages, then snap back to where you were. Useful for maps, tables, and "who was that character".

- From: Kindle Page Flip.
- Phase: no phase. Fits after phase 4.
- Fit: needs pages from the layout cache without moving the stored CFI. Thumbnails of many pages are expensive on CPU. This is a good candidate once phase 5 GPU offload exists.

### Saved reading themes

Save a set of font, size, margins, line spacing, and alignment as a named theme ("Night in bed", "Large print"). Switch with one menu row.

- From: Kindle Themes.
- Phase: 4.
- Fit: a theme is a typography spec, which is the page cache key. Switching themes drops the cache. The CFI keeps the position.

### Search in book

Search the text of the open book and jump to a result.

- From: Kindle.
- Phase: no phase. Fits after phase 4.
- Fit: Readium has search. Typing a query with buttons only needs an on-screen keyboard driven by arrows. That keyboard is shared with notes and Wi-Fi passwords, so it may deserve its own small issue.

### Invert (dark mode)

White text on black.

- From: Kindle Dark Mode.
- Phase: 4.
- Fit: an inverted page ghosts more on `fast`. It may need a shorter clearing cadence when inverted.

### Landscape

Rotate the reader, with buttons remapped to the new orientation.

- From: Kindle orientation.
- Phase: no phase.
- Fit: the Crosspoint reference already says to remap buttons from the current orientation. The profile has width and height but no rotation.

## Dictionary and word tools

### Dictionary lookup

Select a word and see a definition in a panel at the bottom of the page. Buttons move a word cursor, and touch long-presses a word.

- From: Kindle dictionary.
- Phase: no phase.
- Fit: needs an offline dictionary file on the device and a license that lets us ship it, or lets the user add their own. StarDict or a similar open format would avoid writing a parser. Open question.

### Vocabulary builder

Every looked-up word is saved with the sentence it came from. A review screen shows them as flashcards.

- From: Kindle Vocabulary Builder.
- Phase: no phase. Depends on dictionary lookup.
- Fit: SQLite. Could sync later as its own protocol type.

### Word Wise

Short hints above hard words.

- From: Kindle Word Wise.
- Phase: no phase.
- Fit: needs a difficulty word list and changes line spacing. Probably later than the rest of this section.

### X-Ray

A list of characters, places, and terms in the book, with every place each one appears.

- From: Kindle X-Ray.
- Phase: no phase.
- Fit: building the list is a server job, like alignment in phase 9. Shelf would compute it once per book and the device would only display it. Spoiler control: show only mentions before the current CFI.

## Highlights and notes

### Highlight styles

Highlight, underline, and a few marks. In gray now, in color on a color profile in phase 11.

- From: Kindle highlights. Kindle Colorsoft has four highlight colors.
- Phase: 4 for the gray mark. 11 for color.
- Fit: anchor is a CFI range plus a stable id, already locked. On a gray panel, styles must be told apart by pattern (solid, underline, dotted), not by gray level alone.

### Note on a highlight

Attach typed or handwritten text to a highlight.

- From: Kindle notes.
- Phase: typed text after phase 4, once the on-screen keyboard exists. Handwritten is phase 12.
- Fit: stylus present: handwritten note. Stylus absent: typed note or no note. A button-only device still needs a way to type one.

### Notebook view

One screen per book listing every highlight, note, and bookmark in book order. Enter jumps to it in the text.

- From: Kindle Notebook.
- Phase: 4 for local. 7 when they sync.
- Fit: simple list screen. Good early win after phase 4.

### Export highlights

Export a book's highlights and notes as a Markdown or plain-text file.

- From: Kindle export notebook by email.
- Phase: no phase.
- Fit: product.md defers Readwise export. A plain file from shelf's admin UI is a different, smaller thing and might be acceptable. Open question.

### Write in the margins

With a stylus, write directly on the page. Ink anchors to the text next to it so it survives a font change.

- From: Kindle Scribe Active Canvas and margin notes.
- Phase: 12.
- Fit: the hard part is the anchor. A stroke stored only in screen coordinates breaks on relayout, the same reason a highlight is a CFI range. A stroke group could anchor to a CFI and store its offset from that text.

### Notebooks with page templates

Standalone notebooks with blank, lined, grid, and dotted pages.

- From: Kindle Scribe notebooks.
- Phase: 12.
- Fit: templates are static gray8 backgrounds. Ink is vector strokes, already in the plan.

### Pen tools

Pen, highlighter, eraser, and lasso select to move or delete strokes.

- From: Kindle Scribe.
- Phase: 12.
- Fit: stylus only. Hidden when the profile has no stylus. Live ink needs the lowest-latency mode, so this tests the `fast` path hard.

## Settings

### Reading settings in one place

Page-turn refresh cadence (already planned: 1, 5, 10, 15, 30, never), default theme, footer, tap zones on or off, invert.

- From: Kindle reading settings.
- Phase: 4.
- Fit: every row works with Up, Down, Enter, Left. Same screen code as phase 3 settings.

### Frontlight warmth

Warm and cool light, plus a schedule (warmer at night).

- From: Kindle Paperwhite and Oasis warm light.
- Phase: 12, if at all.
- Fit: the profile has `frontlight: yes | no`. Warmth would need a new profile value, and the profile is locked. Open question.

### Sleep screen shows the cover

When the device sleeps, the panel keeps the current book's cover.

- From: Kindle lock-screen cover option.
- Phase: deferred. Sleep waits for a board port.
- Fit: compatible with the deferred rule that sleep keeps the last finished frame. We would draw the cover as that frame.

## Kindle features that clash with the plan

These are listed so they are not proposed again by accident.

| Kindle feature | Why it is out | Source of the rule |
| --- | --- | --- |
| Popular Highlights | Social reading is deferred. | product.md, Deferred |
| Reading streaks and badges | Achievements are deferred. | product.md, Deferred |
| Text-to-speech, Assistive Reader | Text-to-speech is deferred. Read-along uses aligned narration (phase 8). | product.md, Deferred |
| Goodreads, Readwise sync | Export to those services is deferred. | product.md, Deferred |
| Store, recommendations, ads on the lock screen | Shelf does not ship books. There is no store. | product.md, Locked choices |
| Wikipedia and translation lookups | Need a live online service. The reader stays usable with no server and with the radio off. | product.md, Locked choices (Radio) |
| AI notebook summaries | Needs an online model. Same radio rule. Paid APIs are also out of budget. | product.md, Locked choices (Radio) |

## Open questions

- On home, how do Left and Right move along a row of books without clashing with Left as "back"?
- Is Wi-Fi a toggle the user controls, or only "sync now"?
- Which offline dictionary format and source, and who supplies the file?
- Is a plain-file highlight export acceptable while Readwise export stays deferred?
- Does frontlight warmth justify a new profile value?
- One on-screen keyboard for search, notes, and Wi-Fi: who owns it, and in which phase?
