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

### Home layouts

Three ways to draw the same home. Which one a device gets depends on panel size from the profile, never on a device model. Not decided.

- **Calm**: the sketch above. Two choices, a status line each, no covers. Fewest redraws. Fits small button-only panels.
- **Cards**: Books and Notes as two cards, each showing its two most recent items with thumbnails. Nothing else on home. Home redraws only when it is opened, so the thumbnails cost one `quality` frame.
- **Rail**: on a large panel, Books, Notes, and Settings sit in a rail down the left side, and the chosen library fills the rest. Up and Down move along the rail. Right moves into the content. Left returns to the rail.

### Lists turn pages, they do not scroll

Every library (Books, Notes, search results) shows one full page of items at a time, with "page 2 of 5" at the bottom. Moving past the last row turns to the next page. Scrolling on e-ink is many partial redraws and leaves ghosting. A page turn is one frame.

- Phase: 4, with the first library.
- Fit: a library page turn is a `quality` frame. Moving the highlight within a page stays `fast`. Buttons: Down past the last row is the next page, Up past the first is the previous one. A touch swipe turns the page too.

### Search everything

One search across books and notes together. A search for a word shows the books that match, your marks in them, and notebook pages that mention it, in one list. This is the "books and notes are equals" rule applied to search.

- Phase: no phase. After phase 4, once the on-screen keyboard exists. Handwritten queries on a stylus profile in phase 12.
- Fit: Readium searches book text. Marks and typed notes are in SQLite. Handwritten pages need recognition to be searchable, which is its own open question.

### The Books library

- A list by default, with a cover grid as a choice. On a small button-only panel, a list is quicker to move through and moving the highlight stays `fast`.
- Sort by recent, title, or author. Filter by unread, in progress, or finished.
- Progress shown as a thin bar on each row. The percentage is derived from the CFI and the current layout. It is not stored.
- Small corner tags on each cover or row: the format (EPUB, PDF, and later a read-along tag for phase 8) and the progress. Plain text in the gray8 buffer, no color needed.
- Shelves: groups the user makes. A book can sit on more than one. Local in SQLite first. Syncing them needs a protocol type in phase 7 or later.
- Phase: 4 for a local folder. 6 and 7 for books from shelf.

### The Notes library

Everything you wrote or marked, in one place, whichever book it came from.

- **Notebooks**: standalone pen notebooks. Shown only when the profile has a stylus (phase 12).
- **Marks**: every highlight and note, grouped by book or listed by date. Enter on a mark opens the book at that spot (its CFI range).
- **Words**: words you looked up, with the sentence each came from. Depends on the dictionary idea below.
- **Today**: one row that opens today's notebook page, made on first open. A daily journal with no folder setup. Stylus profile, phase 12.
- **New notebook**: the first row in Notes, so making one is a single Enter. No trip into a menu.
- Most recent first by default. Folders are optional, so nothing ever lands in an "uncategorized" pile.
- A notebook's cover is its own page: the last page you wrote on, or one you choose. Drawn from the vector strokes at thumbnail size, like a book cover is made at draw size. A small tag shows the page count.
- Pin a notebook (or a book) and it stays at the top of its library.
- Titles, not files. No `.note` extensions and no file sizes in the library. Inside a folder, one line at the top shows the path back out.
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

A list of the people, places, and terms in the book, with where each appears. Spoiler-safe by default: it shows only what comes before your current place. Off unless the reader turns it on, per book or for all books, so a reader who wants a plain page never sees it.

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

When the device sleeps, the panel shows a picture the user picks: the cover of the current book, a page from a notebook, their own image, or a rotation through a folder of images.

- Phase: deferred. Sleep waits for a board port. The setting and the image handling can be designed earlier.
- Fit: matches the deferred rule that sleep keeps the last finished frame. We draw the chosen picture as that frame. User images are scaled to the panel size from the profile, then dithered once with error diffusion. A sleep picture costs nothing while asleep, since e-ink holds an image with no power.

## What people unlock on other readers

People jailbreak locked e-readers to get a terminal, their own software, and the features below. oseink already ships the first two: the desktop session is a real Linux machine, with no jailbreak needed. The rest are ideas for the reader session, written our way.

### Reading log

A private record of your reading: a calendar of the days you read, time spent per book, pages per hour, and books finished this year. Tapping or entering a day shows what you read that day.

- Phase: no phase. After phase 4, which has the page turns to count.
- Fit: on the device, in SQLite, never shared. No streaks, goals, or badges, since achievements are deferred. A plain record, not a game. Syncing it to shelf is optional and would need a protocol type in phase 7 or later. The pages-per-hour figure also feeds "time left" in the footer.

### Your own fonts

Copy font files into a folder on the device, or upload them to shelf, and they show up in the reader's font list beside the built-in ones.

- Phase: 4.
- Fit: Readium can use user fonts. The font is part of the typography spec, so changing it drops the page cache and keeps the CFI, which phase 4's "done when" already tests.

### Add-ons

A way to make the reader your own without forking it. Two levels:

1. **Packs** (data only, no code): fonts, dictionaries, sleep pictures, notebook page templates, reading layouts. Drop a pack in a folder and the reader picks it up. Low risk, since a pack cannot draw to the panel or run anything.
2. **Plugins** (code): new screens, new sources, new tools. Much larger. A plugin would have to draw through our toolkit and the damage protocol, never the panel directly, and must work with buttons alone. It needs a language, a sandbox, and a stable API we are willing to maintain.

- Phase: no phase. Packs could follow phase 4. Plugins need their own decision in product.md.
- Fit: anyone who wants full control already has the desktop session. Plugins only make sense for things that belong inside the reader session itself. Open question.

### Manga and comics

A comics mode: CBZ and CBR files, right-to-left page order for manga, two-page spreads on a large panel, zoom to a panel, and a dither tuned for line art and screentone.

- Phase: none. **Comics are on the product.md Deferred list.** This idea needs that decision reopened first.
- Fit: for sources, the same rule as books applies. Shelf does not ship sources or indexers. A user could point shelf at their own files or at a comics server they already run that speaks OPDS. A built-in browser of online manga sites would conflict with that rule.

### Online book sources

Browse and borrow from outside sources, not only your own shelf.

- **Free, DRM-free catalogs** (public-domain libraries that publish OPDS feeds): these fit today. Phase 10 acquisition already lists a remote OPDS server as a source. Shelf adds the feed, and the books arrive through the normal catalog.
- **Library lending apps (Libby and similar)**: loans come locked with DRM, delivered to a store reader or through Adobe's DRM, and there is no open API. DRM is out (see the Crosspoint reference). This would need a reversal of that rule, and probably isn't possible without a vendor agreement.
- Fit: browsing happens in shelf's admin UI, or on the device through shelf's catalog, so the radio rule (Wi-Fi up to fetch, then down) still holds.

### Articles to read later

Save a web article or follow a feed, and shelf turns it into an EPUB that syncs to the device like any book.

- Phase: no phase.
- Fit: shelf is books only today, following the acquisition model in product.md. This would widen that scope. Open question.

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
| Email or an app launcher on the reader home | Apps live in the desktop session. Email also needs the radio up. | product.md, Session model |
| A shortcut to a third-party store reader | Third-party readers run on the desktop and do not replace the built-in reader. | product.md, Session model |
| A general file browser in the reader | Phase 4 opens a local folder of books. The desktop has real file tools. | product.md, phase 4 and Session model |

## Open questions

- Calm, cards, or rail for home? At what panel size does the rail take over?
- Is handwriting recognition in scope, so handwritten notes show up in search?
- Should a button jump a whole library page at once, and which one, given Left already means back?
- Do we reopen the comics deferral for manga? Which formats come first?
- Packs only, or code plugins too? If plugins, in what language and with what sandbox?
- Should shelf widen past books to saved articles and feeds?
- Does the reading log stay on the device only, or sync to shelf?
- Does Books open to the library, or straight back into the last book?
- Is Settings a third row on home, or reached another way so home has only two choices?
- Where do Words live if the dictionary never ships: hide the row, or leave it out of Notes?
- Is Wi-Fi a toggle the user controls, or only "sync now"?
- Which offline dictionary format and source, and who supplies the file?
- Does light warmth justify a new profile value?
- One on-screen keyboard for search, notes, and Wi-Fi: who owns it, and in which phase?
