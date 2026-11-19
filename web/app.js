"use strict";
import { request } from "./api.mjs";
const $ = (id) => document.getElementById(id);
let catalogue = [];
let works = [];
let authors = [];
let collections = [];
let scopes = [];
let libraryResults = null;
let bookOffset = 0;
let hasMoreBooks = false;
const BOOK_LIMIT = 500;
let setupRequired = false;
let signedIn = false;
let progressBook = null;
let removeBook = null;
let openEdition = null;
let alignPollTimer = null;
let activityTimer = null;
let activityBusy = false;
let lastView = null;
const ACTIVITY_POLL_MS = 2500;
const CONVERSION_ACTIVE = ["pending", "running", "queued", "processing"];
let searchTimer = null;
let shelfTab = "all";
let matchSelection = null;
let matchCurrentMeta = null;

function readFiltersFromForm() {
  return {
    author: ($("filter-author")?.value || "").trim(),
    series: ($("filter-series")?.value || "").trim(),
    status: $("filter-status")?.value || "",
    format: $("filter-format")?.value || "",
    collection_id: $("filter-collection")?.value || "",
  };
}

function hasActiveFilters(filters = readFiltersFromForm()) {
  return Boolean(
    filters.author ||
      filters.series ||
      filters.status ||
      filters.format ||
      filters.collection_id,
  );
}

function activeFilterCount(filters = readFiltersFromForm()) {
  return [
    filters.author,
    filters.series,
    filters.status,
    filters.format,
    filters.collection_id,
  ].filter(Boolean).length;
}

function syncLibraryChrome(library) {
  const toolbar = $("library-toolbar");
  const filters = $("library-filters");
  const count = $("filter-count");
  const clear = $("filter-clear");
  const books = $("books");
  if (toolbar) toolbar.hidden = !library;
  if (books) books.classList.toggle("catalogue", library);
  if (!library) {
    if (filters) filters.open = false;
    return;
  }
  const n = activeFilterCount();
  const q = ($("search")?.value || "").trim();
  if (count) {
    count.hidden = n === 0;
    count.textContent = String(n);
  }
  if (clear) clear.hidden = !(n > 0 || q);
}

function booksQuery(offset, filters, q) {
  const params = new URLSearchParams();
  params.set("limit", String(BOOK_LIMIT));
  params.set("offset", String(offset));
  if (filters.author) params.set("author", filters.author);
  if (filters.series) params.set("series", filters.series);
  if (filters.status) params.set("status", filters.status);
  if (filters.format) params.set("format", filters.format);
  if (filters.collection_id) params.set("collection_id", filters.collection_id);
  if (q) params.set("q", q);
  return `books?${params}`;
}

function writeFiltersToForm(filters = {}) {
  if ($("filter-author")) $("filter-author").value = filters.author || "";
  if ($("filter-series")) $("filter-series").value = filters.series || "";
  if ($("filter-status")) $("filter-status").value = filters.status || "";
  if ($("filter-format")) $("filter-format").value = filters.format || "";
  if ($("filter-collection"))
    $("filter-collection").value = filters.collection_id
      ? String(filters.collection_id)
      : "";
  if (filters.query != null && $("search")) $("search").value = filters.query;
}

async function loadOrg() {
  const [authorList, collectionList, scopeList] = await Promise.all([
    api("authors").catch(() => []),
    api("collections").catch(() => []),
    api("scopes").catch(() => []),
  ]);
  authors = Array.isArray(authorList) ? authorList : [];
  collections = Array.isArray(collectionList) ? collectionList : [];
  scopes = Array.isArray(scopeList) ? scopeList : [];
  fillOrgControls();
}

function fillOrgControls() {
  const authorOptions = $("author-options");
  if (authorOptions) {
    authorOptions.replaceChildren();
    for (const entry of authors) {
      const name = entry?.name || entry;
      if (!name) continue;
      const opt = document.createElement("option");
      opt.value = name;
      authorOptions.append(opt);
    }
  }
  for (const selectId of ["filter-collection", "manage-collection"]) {
    const select = $(selectId);
    if (!select) continue;
    const current = select.value;
    const first = selectId === "manage-collection" ? "None" : "Any";
    select.replaceChildren();
    const blank = document.createElement("option");
    blank.value = "";
    blank.textContent = first;
    select.append(blank);
    for (const col of collections) {
      const opt = document.createElement("option");
      opt.value = String(col.id);
      opt.textContent = col.name || `Collection ${col.id}`;
      select.append(opt);
    }
    if ([...select.options].some((o) => o.value === current)) select.value = current;
  }
  renderChips();
}

function renderChips() {
  const authorChips = $("author-chips");
  if (authorChips) {
    authorChips.replaceChildren();
    for (const entry of authors.slice(0, 24)) {
      const name = entry?.name || "";
      if (!name) continue;
      const count = entry.book_count != null ? ` (${entry.book_count})` : "";
      const chip = element("button", `${name}${count}`, "chip");
      chip.type = "button";
      chip.addEventListener("click", () => {
        $("filter-author").value = name;
        void applyLibraryFilters();
      });
      authorChips.append(chip);
    }
    authorChips.hidden = authors.length === 0;
  }
  const collectionChips = $("collection-chips");
  if (collectionChips) {
    collectionChips.replaceChildren();
    for (const col of collections) {
      const chip = element("button", col.name || `Collection ${col.id}`, "chip");
      chip.type = "button";
      chip.addEventListener("click", () => {
        $("filter-collection").value = String(col.id);
        void applyLibraryFilters();
      });
      collectionChips.append(chip);
    }
  }
  const scopeChips = $("scope-chips");
  if (scopeChips) {
    scopeChips.replaceChildren();
    for (const scope of scopes) {
      const chip = element("button", scope.name || `Scope ${scope.id}`, "chip");
      chip.type = "button";
      chip.addEventListener("click", () => {
        const filter = scope.filter || scope;
        writeFiltersToForm({
          author: filter.author || "",
          series: filter.series || "",
          status: filter.status || "",
          format: filter.format || "",
          collection_id: filter.collection_id || "",
          query: filter.query || "",
        });
        void applyLibraryFilters();
      });
      scopeChips.append(chip);
    }
  }
}

async function applyLibraryFilters({ append = false } = {}) {
  const filters = readFiltersFromForm();
  const q = ($("search")?.value || "").trim();
  const active = hasActiveFilters(filters) || Boolean(q);
  if (!active && !append) {
    libraryResults = null;
    bookOffset = catalogue.length;
    hasMoreBooks = catalogue.length === BOOK_LIMIT;
    render();
    return;
  }
  const offset = append && libraryResults ? libraryResults.length : 0;
  const books = await api(booksQuery(offset, filters, q));
  const list = Array.isArray(books) ? books : [];
  libraryResults = append && libraryResults ? libraryResults.concat(list) : list;
  bookOffset = libraryResults.length;
  hasMoreBooks = list.length === BOOK_LIMIT;
  render();
}

async function loadMoreBooks() {
  if (!hasMoreBooks) return;
  const filters = readFiltersFromForm();
  const q = ($("search")?.value || "").trim();
  const usingFilters = hasActiveFilters(filters) || Boolean(q);
  if (usingFilters) {
    await applyLibraryFilters({ append: true });
    return;
  }
  const books = await api(booksQuery(bookOffset, {}, ""));
  const list = Array.isArray(books) ? books : [];
  catalogue = catalogue.concat(list);
  bookOffset = catalogue.length;
  hasMoreBooks = list.length === BOOK_LIMIT;
  render();
}

async function renderLibraryRoots() {
  const list = $("library-roots");
  const empty = $("library-roots-empty");
  if (!list || !empty) return;
  list.replaceChildren();
  try {
    const roots = await api("library/roots");
    const items = Array.isArray(roots) ? roots : roots?.roots || [];
    empty.hidden = items.length > 0;
    for (const root of items) {
      const path = typeof root === "string" ? root : root.path || root.root || "";
      if (!path) continue;
      list.append(element("li", path));
    }
    if (!items.length) empty.hidden = false;
  } catch (error) {
    empty.hidden = false;
    empty.textContent = error.message;
  }
}
async function api(path, options = {}) {
  try {
    return await request(path, options);
  } catch (error) {
    if (error.status === 401 && signedIn) showAuth(false);
    throw error;
  }
}
/** Filename stems often use underscores; show them as spaces in the catalogue. */
function displayTitle(title) {
  return String(title || "")
    .replace(/_+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}
function isAudio(book) {
  return (
    book.format === "mp3" ||
    book.format === "m4b" ||
    book.format === "opus" ||
    book.format === "ogg" ||
    book.format === "flac" ||
    book.format === "audiozip" ||
    book.format === "aax"
  );
}
function isPlayableAudiobook(book) {
  return isAudio(book) && book.format !== "aax";
}
function isImported(book) {
  return (
    book.format === "epub" ||
    book.format === "pdf" ||
    book.format === "cbz" ||
    book.format === "cbr" ||
    book.format === "cb7" ||
    book.format === "djvu" ||
    isAudio(book)
  );
}

function companionKey(title, author) {
  const t = String(title || "")
    .toLowerCase()
    .replace(/\([^)]*\)/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  const a = String(author || "").toLowerCase().trim();
  return `${t}|${a}`;
}

/** EPUB ↔ audiobook pair when both editions share a title/author. */
function findCompanionEdition(book, catalogue) {
  const key = companionKey(book.title, book.author);
  if (!book.title || key.startsWith("|")) return null;
  const wantAudio = isImported(book) && !isAudio(book);
  const wantReader = isPlayableAudiobook(book);
  if (!wantAudio && !wantReader) return null;
  for (const other of catalogue) {
    if (other.id === book.id) continue;
    if (companionKey(other.title, other.author) !== key) continue;
    if (wantAudio && isPlayableAudiobook(other)) return other;
    if (wantReader && isImported(other) && !isAudio(other)) return other;
  }
  return null;
}

/** Opposite-format editions available for a manual merge (titles may differ). */
function mergeCandidates(book, catalogue) {
  // Already paired by shared title/author — no merge UI needed.
  if (findCompanionEdition(book, catalogue)) return [];
  if (book.format === "epub") {
    return catalogue.filter(
      (other) => other.id !== book.id && isPlayableAudiobook(other),
    );
  }
  if (isPlayableAudiobook(book)) {
    return catalogue.filter(
      (other) => other.id !== book.id && other.format === "epub",
    );
  }
  return [];
}

/** Storyteller/BO shape: one library card per EPUB+audio pair; hide audio + readaloud rows. */
function findWorkForPair(epub, audio) {
  if (!epub || !audio) return null;
  return (
    works.find(
      (w) => w.epub_book_id === epub.id && w.audio_book_id === audio.id,
    ) || null
  );
}

/** Prefer a ready work for this EPUB; else in-flight; else any (incl. failed). */
function findWorkForEpub(epub) {
  if (!epub) return null;
  const matches = works.filter((w) => w.epub_book_id === epub.id);
  if (!matches.length) return null;
  return (
    matches.find((w) => w.status === "ready") ||
    matches.find(
      (w) => w.status === "processing" || w.status === "queued",
    ) ||
    matches[0]
  );
}

/**
 * Audio edition for Open → Listen / Combine.
 * Works win over title|author companion matching (MP3 tags often lack author).
 */
function pairedAudioForEpub(book, catalogue) {
  if (!book || book.format !== "epub") return null;
  const work = findWorkForEpub(book);
  if (work?.audio_book_id != null) {
    const audio = catalogue.find((b) => b.id === work.audio_book_id);
    if (audio && isPlayableAudiobook(audio)) return { audio, work };
  }
  const companion = findCompanionEdition(book, catalogue);
  if (companion && isPlayableAudiobook(companion)) {
    return { audio: companion, work: findWorkForPair(book, companion) };
  }
  return null;
}

function hiddenEditionIds(catalogue, worksList) {
  const hide = new Set();
  // When a ready work owns an audio file, hide other works' EPUBs for that audio
  // (e.g. failed Combine on a bad EPUB, then re-import + ready on a good one).
  const readyByAudio = new Map();
  for (const w of worksList) {
    if (w.status === "ready" && w.audio_book_id != null) {
      readyByAudio.set(w.audio_book_id, w);
    }
  }
  for (const w of worksList) {
    if (w.readaloud_book_id != null) hide.add(w.readaloud_book_id);
    if (w.audio_book_id != null) hide.add(w.audio_book_id);
    const ready =
      w.audio_book_id != null ? readyByAudio.get(w.audio_book_id) : null;
    if (ready && w.epub_book_id !== ready.epub_book_id) {
      hide.add(w.epub_book_id);
    }
  }
  for (const book of catalogue) {
    // Orphan readalouds (work lost the link) — same title cue as mobile.
    if (/\(readaloud\)\s*$/i.test(book.title || "")) hide.add(book.id);
    if (!isPlayableAudiobook(book)) continue;
    if (findCompanionEdition(book, catalogue)) hide.add(book.id);
  }
  return hide;
}

/** Stats for the folded catalogue (one entry per EPUB+audio work). */
function libraryStats(catalogue, worksList) {
  const hide = hiddenEditionIds(catalogue, worksList);
  const visible = catalogue.filter((b) => !hide.has(b.id));
  let reading = 0;
  let finished = 0;
  let sum = 0;
  let n = 0;
  for (const book of visible) {
    if (book.reading_status === "finished") finished += 1;
    const frac = isImported(book)
      ? Number(book.percentage) || 0
      : book.total_pages > 0
        ? book.pages_read / book.total_pages
        : 0;
    const inProgress = isImported(book)
      ? book.position && frac < 1
      : book.pages_read > 0 && book.pages_read < book.total_pages;
    if (
      book.reading_status === "rereading" ||
      (book.reading_status === "open" && inProgress)
    ) {
      reading += 1;
    }
    if (isImported(book) ? book.position : book.pages_read > 0) {
      sum += frac;
      n += 1;
    }
  }
  return {
    books: visible.length,
    reading,
    finished,
    average_progress: n ? sum / n : 0,
  };
}

/** One reading state per book, matching the folded stats rules. */
function shelfState(book) {
  if (book.reading_status === "finished") return "finished";
  if (book.reading_status === "rereading") return "reading";
  const inProgress = isImported(book)
    ? book.position && Number(book.percentage) < 1
    : book.pages_read > 0 && book.pages_read < book.total_pages;
  if (inProgress) return "reading";
  return isFinished(book) ? "finished" : "unread";
}

function shelfBooks() {
  const hide = hiddenEditionIds(catalogue, works);
  return catalogue.filter((b) => !hide.has(b.id));
}

/** Up to `max` genre tags from book metadata; null when none are recorded. */
function genreTags(book, max = 2) {
  const genres = Array.isArray(book.genres) ? book.genres.filter(Boolean) : [];
  if (!genres.length) return null;
  const list = element("ul", undefined, "genre-tags");
  list.setAttribute("aria-label", "Genres");
  for (const genre of genres.slice(0, max)) {
    const tag = element("li", genre, "genre-tag");
    tag.title = genre;
    list.append(tag);
  }
  return list;
}

function romanYear(year) {
  const marks = [
    [1000, "M"], [900, "CM"], [500, "D"], [400, "CD"], [100, "C"], [90, "XC"],
    [50, "L"], [40, "XL"], [10, "X"], [9, "IX"], [5, "V"], [4, "IV"], [1, "I"],
  ];
  let out = "";
  for (const [value, mark] of marks) {
    while (year >= value) {
      out += mark;
      year -= value;
    }
  }
  return out;
}

function coverNode(book, title) {
  const cover = element("div", undefined, "cover");
  cover.setAttribute("aria-hidden", "true");
  if (book.has_cover) {
    const image = element("img");
    image.src = `/api/v1/books/${book.id}/cover`;
    image.alt = "";
    image.loading = "lazy";
    cover.append(image);
  } else {
    cover.append(element("span", title), element("small", "SHELF / CATALOGUE"));
  }
  return cover;
}

function openHref(book) {
  return isImported(book) && book.format !== "aax" ? readerHref(book) : "#library";
}

function renderLedger(folded) {
  const total = folded.books;
  const done = folded.finished;
  $("ring-total").textContent = total.toLocaleString();
  const circumference = 2 * Math.PI * 52;
  const frac = total ? Math.min(done / total, 1) : 0;
  $("ring-value").style.strokeDasharray = `${frac * circumference} ${circumference}`;
  const open = total - done;
  $("ring-note").textContent = !total
    ? "Add a book to start your ledger."
    : open
      ? `${open.toLocaleString()} ${open === 1 ? "book" : "books"} still open`
      : "Every book on your shelf is finished.";
  $("ins-books").textContent = total.toLocaleString();
  $("ins-finished").textContent = done.toLocaleString();
  $("ins-reading").textContent = folded.reading.toLocaleString();
  $("ins-progress").textContent = $("stat-pages").textContent;
  const counts = { all: 0, reading: 0, unread: 0, finished: 0 };
  for (const book of shelfBooks()) {
    counts.all += 1;
    counts[shelfState(book)] += 1;
  }
  for (const [key, n] of Object.entries(counts)) {
    $(`tab-${key}`).textContent = n.toLocaleString();
  }
}

function renderShelfStrip() {
  const strip = $("shelf-strip");
  strip.replaceChildren();
  const recent = shelfBooks()
    .slice()
    .sort((a, b) => b.id - a.id)
    .slice(0, 5);
  if (!recent.length) {
    strip.append(element("p", "Books you add will gather here.", "fine shelf-empty"));
    return;
  }
  for (const book of recent) {
    const title = displayTitle(book.title);
    const link = element("a", undefined, "shelf-book");
    link.href = openHref(book);
    link.setAttribute("aria-label", `${title} by ${book.author || "unknown author"}`);
    const meta = element("div", undefined, "shelf-meta");
    meta.append(
      element("h3", title),
      element("p", book.author || "Author not recorded", "author"),
    );
    meta.append(
      genreTags(book) || element("p", formatLabel(book), "catalogue-no"),
    );
    link.append(coverNode(book, title), meta);
    strip.append(link);
  }
}

function renderFinished() {
  const list = $("finished-list");
  list.replaceChildren();
  const done = shelfBooks()
    .filter((b) => shelfState(b) === "finished")
    .sort((a, b) => b.id - a.id)
    .slice(0, 3);
  if (!done.length) {
    list.append(
      element("p", "Nothing finished yet.", "finished-lede"),
      element("p", "The last page of your next book will be noted here.", "fine"),
    );
    return;
  }
  const [lead, ...rest] = done;
  const leadTitle = displayTitle(lead.title);
  const feature = element("a", undefined, "finished-lead");
  feature.href = openHref(lead);
  const text = element("div");
  text.append(
    element("p", leadTitle, "finished-lede"),
    element("p", lead.author || "Author not recorded", "author"),
    element("p", `Finished · ${formatLabel(lead)}`, "fine"),
  );
  const leadTags = genreTags(lead);
  if (leadTags) text.append(leadTags);
  feature.append(coverNode(lead, leadTitle), text);
  list.append(feature);
  if (rest.length) {
    const more = element("ul", undefined, "finished-more");
    for (const book of rest) {
      const item = element("li");
      item.append(
        element("span", displayTitle(book.title)),
        element("span", book.author || "", "fine"),
      );
      more.append(item);
    }
    list.append(more);
  }
}

function renderAuthorBreakdown() {
  const box = $("author-breakdown");
  box.replaceChildren();
  const tally = new Map();
  const books = shelfBooks();
  for (const book of books) {
    const name = (book.author || "Author not recorded").trim();
    const key = name.toLowerCase().replace(/[^a-z0-9]/g, "");
    const entry = tally.get(key) || { name, count: 0 };
    entry.count += 1;
    tally.set(key, entry);
  }
  const rows = [...tally.values()].sort((a, b) => b.count - a.count).slice(0, 6);
  if (!rows.length) {
    box.append(element("p", "Authors appear once your shelf has books.", "fine"));
    return;
  }
  rows.forEach((row, index) => {
    const share = row.count / books.length;
    const line = element("div", undefined, index === 0 ? "breakdown-row lead" : "breakdown-row");
    const bar = element("progress");
    bar.max = 1;
    bar.value = share;
    bar.setAttribute("aria-label", `${row.name}: ${row.count} of ${books.length} books`);
    line.append(
      element("span", row.name),
      bar,
      element("span", `${Math.round(share * 100)}%`, "fine"),
    );
    box.append(line);
  });
}

let insightsReading = null;
let insightsReadingPromise = null;

function formatDuration(seconds) {
  const s = Math.max(0, Math.round(Number(seconds) || 0));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem ? `${h}h ${rem}m` : `${h}h`;
}

function addDaysIso(iso, delta) {
  const [y, m, d] = String(iso).split("-").map(Number);
  const dt = new Date(Date.UTC(y, m - 1, d));
  dt.setUTCDate(dt.getUTCDate() + delta);
  return dt.toISOString().slice(0, 10);
}

function weekdayMonday0(iso) {
  const [y, m, d] = String(iso).split("-").map(Number);
  const wd = new Date(Date.UTC(y, m - 1, d)).getUTCDay(); // 0=Sun
  return (wd + 6) % 7; // Mon=0
}

function heatmapLevel(seconds, maxSeconds) {
  if (!seconds || seconds <= 0) return 0;
  if (!maxSeconds || maxSeconds <= 0) return 1;
  const ratio = seconds / maxSeconds;
  if (ratio < 0.25) return 1;
  if (ratio < 0.5) return 2;
  if (ratio < 0.75) return 3;
  return 4;
}

function renderHeatmap(payload) {
  const grid = $("reading-calendar");
  if (!grid) return;
  const to = payload?.to || new Date().toISOString().slice(0, 10);
  const from =
    payload?.from || addDaysIso(to, -(7 * 26 - 1));
  const byDay = new Map();
  for (const row of payload?.heatmap || []) {
    byDay.set(row.day, Number(row.readingSeconds) || 0);
  }
  let max = 0;
  for (const v of byDay.values()) max = Math.max(max, v);

  // Columns are weeks starting Monday; pad so the grid ends near `to`.
  let cursor = addDaysIso(to, -(7 * 26 - 1));
  cursor = addDaysIso(cursor, -weekdayMonday0(cursor));
  grid.replaceChildren();
  let activeDays = 0;
  for (let i = 0; i < 7 * 26; i += 1) {
    const day = addDaysIso(cursor, i);
    const secs = byDay.get(day) || 0;
    if (secs > 0) activeDays += 1;
    const cell = element("i");
    const level = heatmapLevel(secs, max);
    if (level) cell.dataset.level = String(level);
    cell.title =
      secs > 0
        ? `${day}: ${formatDuration(secs)}`
        : `${day}: no reading`;
    grid.append(cell);
  }
  grid.setAttribute(
    "aria-label",
    activeDays
      ? `Reading heatmap from ${from} to ${to}: ${activeDays} days with reading`
      : `Reading heatmap from ${from} to ${to}: no reading sessions recorded yet`,
  );
  const range = $("ins-heatmap-range");
  if (range) range.textContent = `${from} → ${to}`;
  const streakEl = $("ins-streak");
  const streak = payload?.streak;
  if (streakEl) {
    if (streak && (streak.currentStreak || streak.longestStreak)) {
      streakEl.hidden = false;
      const dots = (streak.lastSevenDays || [])
        .map((on) => (on ? "●" : "○"))
        .join("");
      streakEl.textContent = `Streak ${streak.currentStreak || 0} · best ${streak.longestStreak || 0} · ${dots}`;
    } else {
      streakEl.hidden = true;
      streakEl.textContent = "";
    }
  }
}

function renderSourceBars(payload) {
  const box = $("ins-source-bars");
  const empty = $("ins-source-empty");
  if (!box) return;
  box.replaceChildren();
  const slices = [...(payload?.sourceDistribution?.slices || [])].sort(
    (a, b) => (b.readingSeconds || 0) - (a.readingSeconds || 0),
  );
  const total =
    Number(payload?.sourceDistribution?.totalSeconds) ||
    slices.reduce((n, s) => n + (Number(s.readingSeconds) || 0), 0);
  if (!slices.length || total <= 0) {
    if (empty) empty.hidden = false;
    return;
  }
  if (empty) empty.hidden = true;
  for (const slice of slices) {
    const secs = Number(slice.readingSeconds) || 0;
    const share = total ? secs / total : 0;
    const row = element("div", undefined, "ins-bar-row");
    row.setAttribute("role", "listitem");
    const bar = element("progress");
    bar.max = 1;
    bar.value = share;
    bar.setAttribute(
      "aria-label",
      `${slice.bucket}: ${formatDuration(secs)} (${Math.round(share * 100)}%)`,
    );
    row.append(
      element("span", slice.bucket, "ins-bar-label"),
      bar,
      element("span", formatDuration(secs), "fine"),
    );
    box.append(row);
  }
}

function renderSessionTimeline(payload) {
  const list = $("ins-session-timeline");
  const empty = $("ins-timeline-empty");
  if (!list) return;
  list.replaceChildren();
  const rows = payload?.sessionTimeline || [];
  if (!rows.length) {
    if (empty) empty.hidden = false;
    return;
  }
  if (empty) empty.hidden = true;
  for (const row of rows) {
    const li = element("li", undefined, "ins-session-row");
    const title = displayTitle(row.bookTitle) || "Untitled";
    const when = String(row.endedAt || "").replace("T", " ").replace(/Z$/, " UTC");
    li.append(
      element("span", title, "ins-session-title"),
      element(
        "span",
        `${formatDuration(row.durationSeconds)} · ${row.bookSource || row.bookFormat || "session"}`,
        "fine",
      ),
      element("span", when, "fine ins-session-when"),
    );
    list.append(li);
  }
}

async function loadReadingInsights(force = false) {
  if (!signedIn) return null;
  if (insightsReading && !force) return insightsReading;
  if (insightsReadingPromise && !force) return insightsReadingPromise;
  insightsReadingPromise = api("insights/reading")
    .then((data) => {
      insightsReading = data;
      insightsReadingPromise = null;
      return data;
    })
    .catch((err) => {
      insightsReadingPromise = null;
      throw err;
    });
  return insightsReadingPromise;
}

function renderWidgets(payload) {
  const grid = $("ins-widget-grid");
  if (!grid) return;
  grid.replaceChildren();
  const widgets = payload?.widgets || [];
  for (const widget of widgets) {
    const card = element("article", undefined, "ins-widget");
    card.dataset.type = widget.type || "";
    const data = widget.data || {};
    if (widget.type === "reading-streak") {
      card.append(
        element("h3", "Streak", "ins-widget-kicker"),
        element("p", String(data.currentStreak || 0), "ins-widget-value"),
        element(
          "p",
          `Best ${data.longestStreak || 0} · ${(data.lastSevenDays || []).map((on) => (on ? "●" : "○")).join("")}`,
          "fine",
        ),
      );
    } else if (widget.type === "reading-goal") {
      const goal = data.goalBooks;
      const done = data.completedBooks || 0;
      const label =
        goal == null
          ? `${done} finished in ${data.year || ""} · set a goal in prefs`
          : `${done} / ${goal} books · ${data.year || ""}`;
      const bar = element("progress");
      bar.max = Math.max(1, Number(goal) || 1);
      bar.value = Math.min(bar.max, done);
      card.append(
        element("h3", "Annual goal", "ins-widget-kicker"),
        element("p", goal == null ? String(done) : `${done}/${goal}`, "ins-widget-value"),
        bar,
        element("p", label, "fine"),
      );
    } else if (widget.type === "monthly-challenge") {
      const bar = element("progress");
      bar.max = Math.max(1, Number(data.target) || 1);
      bar.value = Math.min(bar.max, Number(data.progress) || 0);
      card.append(
        element("h3", data.title || "Challenge", "ins-widget-kicker"),
        element(
          "p",
          `${data.progress || 0} / ${data.target || 0}`,
          "ins-widget-value",
        ),
        bar,
        element("p", data.description || "", "fine"),
      );
    } else if (widget.type === "reading-dna") {
      card.append(
        element("h3", "Reading DNA", "ins-widget-kicker"),
        element("p", data.archetype || "Uncharted", "ins-widget-value ins-widget-archetype"),
        element(
          "p",
          [data.lengthLabel, data.timeLabel, data.rhythmLabel]
            .filter((x) => x && x !== "—")
            .join(" · ") || "Read a few sessions to shape this portrait.",
          "fine",
        ),
      );
    } else {
      continue;
    }
    grid.append(card);
  }
}

function renderAchievements(payload) {
  const root = $("ins-achievements");
  const empty = $("ins-achievements-empty");
  const summary = $("ins-achievements-summary");
  if (!root) return;
  root.replaceChildren();
  const categories = payload?.categories || [];
  const earned = payload?.totalEarned || 0;
  const available = payload?.totalAvailable || 0;
  if (summary) {
    summary.textContent = available
      ? `${earned} of ${available} unlocked`
      : "";
  }
  if (!categories.length) {
    if (empty) empty.hidden = false;
    return;
  }
  if (empty) empty.hidden = true;
  for (const cat of categories) {
    const section = element("section", undefined, "ins-ach-cat");
    const head = element("div", undefined, "ins-ach-cat-head");
    head.append(
      element("h3", cat.label || cat.key || ""),
      element(
        "p",
        `${cat.earnedCount || 0}/${cat.totalCount || 0}`,
        "fine",
      ),
    );
    const grid = element("div", undefined, "ins-ach-grid");
    for (const item of cat.achievements || []) {
      const card = element("article", undefined, "ins-ach");
      card.dataset.earned = item.earned ? "true" : "false";
      const threshold = Number(item.threshold) || 0;
      const progress = Number(item.currentProgress) || 0;
      card.append(
        element("h4", item.name || item.key || "", "ins-ach-name"),
        element("p", item.description || "", "ins-ach-desc"),
      );
      const meta = element("div", undefined, "ins-ach-meta");
      meta.append(
        element("span", item.rarity || "common"),
        element(
          "span",
          item.earned
            ? "Unlocked"
            : threshold
              ? `${Math.min(progress, threshold)} / ${threshold}`
              : "—",
        ),
      );
      card.append(meta);
      if (!item.earned && threshold > 0) {
        const bar = element("progress");
        bar.max = threshold;
        bar.value = Math.min(threshold, progress);
        card.append(bar);
      }
      grid.append(card);
    }
    section.append(head, grid);
    root.append(section);
  }
}

async function renderReadingInsights() {
  try {
    const [data, widgets, achievements] = await Promise.all([
      loadReadingInsights(),
      api("insights/widgets").catch(() => ({ widgets: [] })),
      api("insights/achievements").catch(() => ({
        categories: [],
        totalEarned: 0,
        totalAvailable: 0,
      })),
    ]);
    if (data) {
      renderHeatmap(data);
      renderSourceBars(data);
      renderSessionTimeline(data);
    }
    renderWidgets(widgets);
    renderAchievements(achievements);
  } catch {
    renderHeatmap({ heatmap: [], from: "", to: "" });
    renderSourceBars({ sourceDistribution: { slices: [], totalSeconds: 0 } });
    renderSessionTimeline({ sessionTimeline: [] });
    renderWidgets({ widgets: [] });
    renderAchievements({ categories: [], totalEarned: 0, totalAvailable: 0 });
  }
}

function renderCalendar() {
  void renderReadingInsights();
}

function readerHref(book) {
  if (!book) return "#";
  if (isPlayableAudiobook(book)) {
    return `/reader?id=${book.id}&format=${book.format}`;
  }
  const fmt =
    book.format === "pdf" ||
    book.format === "cbz" ||
    book.format === "cbr" ||
    book.format === "cb7" ||
    book.format === "djvu"
      ? `&format=${book.format}`
      : "";
  return `/reader?id=${book.id}${fmt}`;
}

function setAlignStatus(el, work) {
  if (!el || !work) return;
  el.hidden = false;
  el.classList.add("align-status");
  const pct = Math.round((work.progress || 0) * 100);
  const stage = work.stage || "working";
  let label;
  if (work.status === "ready") label = "Read-aloud ready.";
  else if (work.status === "failed")
    label = work.error || "Alignment failed.";
  else label = `Aligning (${stage} · ${pct}%)…`;
  el.replaceChildren();
  const row = element("div", undefined, "align-status-row");
  if (work.status === "processing" || work.status === "queued") {
    row.append(element("span", "", "align-spinner"));
  }
  row.append(element("span", label, "fine"));
  el.append(row);
  if (work.status === "processing" || work.status === "queued") {
    const bar = element("progress");
    bar.max = 1;
    bar.value = Math.max(0, Math.min(1, Number(work.progress) || 0));
    bar.setAttribute("aria-label", `Alignment ${pct}%`);
    el.append(bar);
  }
}

function scheduleAlignPoll() {
  const busy = works.some(
    (w) => w.status === "processing" || w.status === "queued",
  );
  if (!busy) {
    if (alignPollTimer) {
      clearInterval(alignPollTimer);
      alignPollTimer = null;
    }
    return;
  }
  if (alignPollTimer) return;
  alignPollTimer = setInterval(() => {
    void refreshWorksOnly();
  }, 2000);
}

async function refreshWorksOnly() {
  if (!signedIn) return;
  try {
    works = await api("works");
    render();
    scheduleAlignPoll();
  } catch {
    /* keep last known works */
  }
}

async function combineEditions(epubBook, audioBook, statusEl, title) {
  if (statusEl) {
    setAlignStatus(statusEl, {
      status: "queued",
      stage: "SPLIT_TRACKS",
      progress: 0,
    });
  }
  const body = {
    epub_book_id: epubBook.id,
    audio_book_id: audioBook.id,
    start: true,
  };
  const merged = String(title || "").trim();
  if (merged) body.title = merged;
  const work = await api("works", {
    method: "POST",
    body: JSON.stringify(body),
  });
  let current = work;
  const upsertWork = (w) => {
    const i = works.findIndex((x) => x.id === w.id);
    if (i >= 0) works[i] = w;
    else works.push(w);
  };
  upsertWork(current);
  scheduleAlignPoll();
  // Whisper + multi-chapter align can take many minutes; poll up to ~30 min.
  for (let i = 0; i < 1800; i++) {
    if (current.status === "ready") {
      if (statusEl) setAlignStatus(statusEl, current);
      await refresh();
      return current;
    }
    if (current.status === "failed") {
      if (statusEl) setAlignStatus(statusEl, current);
      throw new Error(current.error || "Alignment failed.");
    }
    if (statusEl) setAlignStatus(statusEl, current);
    await new Promise((r) => setTimeout(r, 1000));
    current = await api(`works/${work.id}`);
    upsertWork(current);
  }
  if (statusEl) {
    statusEl.hidden = false;
    statusEl.textContent =
      "Still aligning in the background — progress keeps updating on this card.";
  }
  return current;
}

function formatLabel(book) {
  if (isAudio(book)) return "Audiobook";
  switch (book.format) {
    case "pdf":
      return "PDF";
    case "cbz":
      return "CBZ";
    case "cbr":
      return "CBR";
    case "cb7":
      return "CB7";
    case "djvu":
      return "DjVu";
    default:
      return "EPUB";
  }
}
function audioStatusLine(book) {
  if (book.format !== "aax") return null;
  if (book.conversion_status === "failed") {
    return book.conversion_error || "AAX conversion failed.";
  }
  return "Converting AAX…";
}
function isFinished(book) {
  if (book.reading_status === "finished") return true;
  if (
    book.reading_status === "unfinished" ||
    book.reading_status === "rereading"
  )
    return false;
  return isImported(book)
    ? book.percentage >= 1
    : book.pages_read >= book.total_pages;
}
function manageHelp(book) {
  switch (book.reading_status) {
    case "finished":
      return "This book is marked finished.";
    case "unfinished":
      return "Marked unfinished — not counting as a re-read.";
    case "rereading":
      return "Marked as re-reading.";
    default:
      return "Catalogue tools for this book.";
  }
}
function message(text) {
  $("message").textContent = text;
  $("message").hidden = !text;
}

/** Same-origin preview URL so CSP allows provider cover art. */

function openCoverLightbox(src, alt = "") {
  const dialog = $("cover-lightbox");
  const img = $("cover-lightbox-image");
  if (!dialog || !img || !src) return;
  img.src = src;
  img.alt = alt || "Cover preview";
  if (!dialog.open) dialog.showModal();
}

function wireCoverPreview(img) {
  if (!img || img.tagName !== "IMG" || img.dataset.zoomWired === "1") return;
  img.dataset.zoomWired = "1";
  img.classList.add("cover-zoom");
  img.tabIndex = 0;
  img.setAttribute("role", "button");
  img.setAttribute("aria-label", "View larger cover");
  const open = (event) => {
    event.preventDefault();
    event.stopPropagation();
    openCoverLightbox(img.currentSrc || img.src, img.alt);
  };
  img.addEventListener("click", open);
  img.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") open(event);
  });
}

function proxiedImageUrl(url) {
  if (!url) return "";
  if (
    url.startsWith("/") ||
    url.startsWith("blob:") ||
    url.startsWith("data:")
  ) {
    return url;
  }
  return `/api/v1/meta/image?url=${encodeURIComponent(url)}`;
}

function manageStatus(text) {
  const el = $("manage-status");
  if (!el) {
    message(text || "");
    return;
  }
  el.textContent = text || "";
  el.hidden = !text;
  if (text) message(text);
}

async function runBusy(button, busyLabel, work) {
  const prev = button ? button.textContent : null;
  if (button) {
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
    if (busyLabel) button.textContent = busyLabel;
  }
  if (busyLabel) manageStatus(busyLabel);
  try {
    return await work();
  } finally {
    if (button) {
      button.disabled = false;
      button.removeAttribute("aria-busy");
      if (prev != null) button.textContent = prev;
    }
  }
}

function showAuth(setup) {
  stopActivityPoll();
  lastView = null;
  setupRequired = setup;
  signedIn = false;
  $("auth").hidden = false;
  $("content").hidden = true;
  $("open-add").hidden = true;
  $("masthead-search").hidden = true;
  $("signout").hidden = true;
  for (const id of [
    "progress-dialog",
    "manage-dialog",
    "remove-dialog",
    "open-edition-dialog",
    "cover-lightbox",
  ])
    if ($(id)?.open) $(id).close();
  $("auth-heading").textContent = setup
    ? "Make yourself at home."
    : "Open your reading room.";
  $("auth-help").textContent = setup
    ? "Create your account with a password of at least 12 characters."
    : "Sign in to your library.";
  $("auth-submit").textContent = setup ? "Create account" : "Sign in";
  const password = $("auth-form").elements.password;
  password.autocomplete = setup ? "new-password" : "current-password";
  password.minLength = setup ? 12 : 1;
}
async function refresh() {
  insightsReading = null;
  insightsReadingPromise = null;
  const [books, workList] = await Promise.all([
    api(booksQuery(0, {}, "")),
    api("works").catch(() => []),
  ]);
  catalogue = Array.isArray(books) ? books : [];
  works = Array.isArray(workList) ? workList : [];
  bookOffset = catalogue.length;
  hasMoreBooks = catalogue.length === BOOK_LIMIT;
  libraryResults = null;
  signedIn = true;
  $("auth").hidden = true;
  $("content").hidden = false;
  $("open-add").hidden = false;
  $("signout").hidden = false;
  // Folded counts: EPUB+audio(+readaloud) is one shelf book.
  const folded = libraryStats(catalogue, works);
  for (const [id, key] of [
    ["books", "books"],
    ["reading", "reading"],
    ["finished", "finished"],
  ]) {
    $(`stat-${id}`).textContent = folded[key].toLocaleString();
  }
  // EPUB has no printed page count (decision 0003); show mean completion instead.
  $("stat-pages").textContent =
    `${Math.round(Number(folded.average_progress || 0) * 100)}%`;
  renderLedger(folded);
  await loadOrg().catch(() => {});
  if (hasActiveFilters() || ($("search")?.value || "").trim()) {
    await applyLibraryFilters().catch(() => render());
  } else {
    render();
  }
  scheduleAlignPoll();
}
function element(tag, text, className) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}
const KOKORO_VOICES = [
  { id: "af_heart", name: "Heart", shortName: "af_heart", language: "English", locale: "en-US", gender: "Female" },
  { id: "af_bella", name: "Bella", shortName: "af_bella", language: "English", locale: "en-US", gender: "Female" },
  { id: "af_sarah", name: "Sarah", shortName: "af_sarah", language: "English", locale: "en-US", gender: "Female" },
  { id: "am_adam", name: "Adam", shortName: "am_adam", language: "English", locale: "en-US", gender: "Male" },
  { id: "am_michael", name: "Michael", shortName: "am_michael", language: "English", locale: "en-US", gender: "Male" },
  { id: "bf_emma", name: "Emma", shortName: "bf_emma", language: "English", locale: "en-GB", gender: "Female" },
  { id: "bm_george", name: "George", shortName: "bm_george", language: "English", locale: "en-GB", gender: "Male" },
];


async function renderMetaProviders() {
  const list = $("meta-providers");
  if (!list) return;
  list.replaceChildren();
  try {
    const providers = await api("metadata/providers");
    for (const provider of providers) {
      const card = element("article", undefined, "tts-provider");
      card.append(
        element("h3", provider.name),
        element(
          "p",
          `${provider.key} · ${provider.kind} · ${provider.enabled ? "on" : "off"}${provider.has_api_key ? " · key set" : ""}`,
          "fine",
        ),
      );
      const actions = element("div", undefined, "tts-actions");
      const toggle = element("button", provider.enabled ? "Disable" : "Enable", "quiet");
      toggle.type = "button";
      toggle.addEventListener("click", async () => {
        await api(`metadata/providers/${provider.id}`, {
          method: "PATCH",
          body: JSON.stringify({ enabled: !provider.enabled }),
        });
        await renderMetaProviders();
      });
      actions.append(toggle);
      if (["google", "hardcover", "comicvine"].includes(provider.key)) {
        const keyBtn = element("button", "Set API key", "quiet");
        keyBtn.type = "button";
        keyBtn.addEventListener("click", async () => {
          const key = prompt(`${provider.name} API key (blank clears)`);
          if (key === null) return;
          await api(`metadata/providers/${provider.id}`, {
            method: "PATCH",
            body: JSON.stringify({ api_key: key === "" ? null : key }),
          });
          message("Provider updated.");
          await renderMetaProviders();
        });
        actions.append(keyBtn);
      }
      if (provider.key === "amazon" || provider.key === "kobo") {
        const cfgBtn = element("button", "Config", "quiet");
        cfgBtn.type = "button";
        cfgBtn.addEventListener("click", async () => {
          const hint =
            provider.key === "amazon"
              ? '{"domain":"amazon.com","cookie":""}'
              : '{"country":"us","language":"en"}';
          const raw = prompt(`${provider.name} config JSON`, hint);
          if (raw === null) return;
          await api(`metadata/providers/${provider.id}`, {
            method: "PATCH",
            body: JSON.stringify({ config_json: raw }),
          });
          message("Provider config saved.");
          await renderMetaProviders();
        });
        actions.append(cfgBtn);
      }
      card.append(actions);
      list.append(card);
    }
  } catch (error) {
    list.textContent = error.message;
  }
}


async function refreshMetaFetchStatus() {
  await refreshActivity();
}

function bookTitle(id) {
  const book = catalogue.find((b) => b.id === id);
  return book ? displayTitle(book.title) : `Book ${id}`;
}

function activityCounts(dl, pairs) {
  dl.replaceChildren();
  for (const [label, value] of pairs) {
    const wrap = element("div", undefined, value ? "is-set" : "");
    wrap.append(element("dt", label), element("dd", String(value)));
    dl.append(wrap);
  }
}

function activityItem(title, detail, progress) {
  const item = element("div", undefined, "activity-item");
  const row = element("div", undefined, "activity-item-row");
  row.append(element("span", "", "align-spinner"), element("span", title, "activity-item-title"));
  if (detail) row.append(element("span", detail, "fine"));
  item.append(row);
  if (progress != null) {
    const bar = element("progress");
    bar.max = 1;
    bar.value = Math.max(0, Math.min(1, Number(progress) || 0));
    bar.setAttribute("aria-label", `${title} ${Math.round(bar.value * 100)}%`);
    item.append(bar);
  }
  return item;
}

function renderMetaActivity(st) {
  const row = $("activity-meta");
  const detail = $("activity-meta-detail");
  const active = st.queued + st.processing;
  const state = st.paused ? "paused" : active ? "busy" : st.failed ? "warn" : "idle";
  row.dataset.state = state;
  detail.textContent = st.paused
    ? `Paused${active ? ` with ${active} waiting` : ""}.`
    : st.current
      ? `Fetching “${st.current.title}”.`
      : active
        ? "Working through the queue."
        : st.failed
          ? "Queue clear. Some books could not be matched."
          : "Nothing queued.";
  activityCounts($("activity-meta-counts"), [
    ["Queued", st.queued],
    ["Working", st.processing],
    ["Failed", st.failed],
  ]);
  $("meta-pause").hidden = st.paused || !active;
  $("meta-resume").hidden = !st.paused;
  $("meta-cancel").hidden = !st.queued;
  return state;
}

function renderAlignActivity(list) {
  const active = list.filter((w) => w.status === "queued" || w.status === "processing");
  $("activity-align").dataset.state = active.length ? "busy" : "idle";
  $("activity-align-detail").textContent = active.length
    ? `${active.length} edition${active.length === 1 ? "" : "s"} aligning.`
    : "No editions aligning.";
  $("activity-align-items").replaceChildren(
    ...active.map((w) =>
      activityItem(
        bookTitle(w.epub_book_id),
        w.status === "queued" ? "waiting" : `${w.stage || "working"} · ${Math.round((w.progress || 0) * 100)}%`,
        w.status === "processing" ? w.progress : null,
      ),
    ),
  );
  return active.length;
}

function renderConvertActivity(books) {
  const active = books.filter((b) => CONVERSION_ACTIVE.includes(b.conversion_status));
  $("activity-convert").dataset.state = active.length ? "busy" : "idle";
  $("activity-convert-detail").textContent = active.length
    ? `${active.length} AAX file${active.length === 1 ? "" : "s"} converting.`
    : "No audiobooks converting.";
  $("activity-convert-items").replaceChildren(
    ...active.map((b) =>
      activityItem(
        displayTitle(b.title),
        b.conversion_status === "pending" || b.conversion_status === "queued" ? "waiting" : "converting",
        null,
      ),
    ),
  );
  return active.length;
}

async function refreshActivity() {
  if (!$("activity-list") || activityBusy) return;
  activityBusy = true;
  const converting = catalogue.some((b) => CONVERSION_ACTIVE.includes(b.conversion_status));
  try {
    const [st, workList, books] = await Promise.all([
      api("metadata/fetch/status").catch((error) => ({ error })),
      api("works").catch(() => works),
      converting ? api(booksQuery(0, {}, "")).catch(() => catalogue) : Promise.resolve(catalogue),
    ]);
    if (Array.isArray(workList)) works = workList;
    if (converting && Array.isArray(books)) {
      const finished = !books.some((b) => CONVERSION_ACTIVE.includes(b.conversion_status));
      catalogue = books;
      if (finished) void refresh().catch(() => {});
    }
    let busy = 0;
    let paused = false;
    if (st.error) {
      $("activity-meta").dataset.state = "warn";
      $("activity-meta-detail").textContent = st.error.message;
    } else {
      const metaState = renderMetaActivity(st);
      if (metaState === "busy") busy += 1;
      paused = metaState === "paused";
    }
    busy += renderAlignActivity(works);
    busy += renderConvertActivity(catalogue);
    const summary = $("activity-summary");
    summary.dataset.state = busy ? "busy" : paused ? "paused" : "idle";
    const running = busy ? `${busy} task${busy === 1 ? "" : "s"} running` : "";
    $("activity-summary-text").textContent =
      [running, paused ? "Metadata paused" : ""].filter(Boolean).join(" · ") ||
      "No background work right now.";
    $("activity-list").setAttribute("aria-busy", "false");
  } finally {
    activityBusy = false;
  }
}

function startActivityPoll() {
  void refreshActivity();
  if (activityTimer) return;
  activityTimer = setInterval(() => {
    if (document.hidden) return;
    void refreshActivity();
  }, ACTIVITY_POLL_MS);
}

function stopActivityPoll() {
  if (activityTimer) clearInterval(activityTimer);
  activityTimer = null;
}

async function loadManageMetadata(book) {
  manageStatus("");
  const desc = $("manage-meta-description");
  const covers = $("manage-meta-covers");
  const matchRoot = $("manage-meta-match");
  const results = $("manage-meta-results");
  const compare = $("manage-meta-compare");
  const coverResults = $("manage-meta-cover-results");
  matchSelection = null;
  matchCurrentMeta = null;
  if (matchRoot) matchRoot.hidden = true;
  if (results) results.replaceChildren();
  if (compare) {
    compare.hidden = true;
    const fields = $("manage-meta-compare-fields");
    if (fields) fields.replaceChildren();
    const applySelected = $("manage-meta-apply-selected");
    if (applySelected) applySelected.hidden = true;
  }
  if (coverResults) {
    coverResults.hidden = true;
    coverResults.replaceChildren();
  }
  if (covers) covers.replaceChildren();
  if (!desc) return;
  desc.value = "";
  try {
    const meta = await api(`books/${book.id}/metadata`);
    matchCurrentMeta = meta;
    desc.value = meta.description || "";
    if ($("manage-meta-title") && meta.title) {
      $("manage-meta-title").value = meta.title;
    }
    if ($("manage-meta-author")) {
      const author =
        (Array.isArray(meta.authors) && meta.authors[0]) ||
        meta.author ||
        "";
      if (author) $("manage-meta-author").value = author;
    }
    if ($("manage-meta-series")) {
      $("manage-meta-series").value = meta.series_name || book.series || "";
    }
    if ($("manage-meta-series-index")) {
      const idx =
        meta.series_index != null && meta.series_index !== ""
          ? meta.series_index
          : book.series_index;
      $("manage-meta-series-index").value =
        idx != null && idx !== "" ? idx : "";
    }
    if (covers && Array.isArray(meta.covers)) {
      for (const cover of meta.covers) {
        const fig = document.createElement("figure");
        const img = document.createElement("img");
        img.alt = `${cover.medium} cover`;
        img.src = `/api/v1/books/${book.id}/covers/${cover.id}`;
        wireCoverPreview(img);
        const btn = element("button", cover.active ? "Active" : "Use", "quiet");
        btn.type = "button";
        btn.disabled = !!cover.active;
        btn.addEventListener("click", async () => {
          await api(`books/${book.id}/covers/${cover.id}/activate`, {
            method: "POST",
            body: "{}",
          });
          await loadManageMetadata(book);
          message("Active cover updated.");
        });
        fig.append(img, btn);
        covers.append(fig);
      }
    }
  } catch (_) {
    /* older servers */
  }
}

const MATCH_COMPARE_FIELDS = [
  "title",
  "subtitle",
  "description",
  "authors",
  "genres",
  "series_name",
  "series_index",
  "publisher",
  "published_date",
  "published_year",
  "language",
  "page_count",
  "isbn10",
  "isbn13",
];

const MATCH_APPLY_ALL_FIELDS = [
  "title","subtitle","description","authors","genres","tags","narrators",
  "series_name","series_index","publisher","published_date","published_year",
  "language","page_count","isbn10","isbn13","cover","audio_cover",
];

function fieldDisplay(value) {
  if (value == null || value === "") return "—";
  if (Array.isArray(value)) return value.join(", ") || "—";
  return String(value);
}

function currentFieldValue(meta, field) {
  if (!meta) return null;
  if (field === "title") return meta.title ?? $("manage-meta-title")?.value;
  if (field === "authors") {
    if (Array.isArray(meta.authors) && meta.authors.length) return meta.authors;
    const a = $("manage-meta-author")?.value;
    return a ? [a] : [];
  }
  if (field === "series_name") return meta.series_name ?? $("manage-meta-series")?.value;
  if (field === "series_index") return meta.series_index ?? $("manage-meta-series-index")?.value;
  if (field === "description") return meta.description ?? $("manage-meta-description")?.value;
  return meta[field];
}

function candidateFieldValue(candidate, field) {
  if (!candidate) return null;
  if (field === "authors") return candidate.authors || [];
  return candidate[field];
}

function isMissingValue(value) {
  if (value == null) return true;
  if (Array.isArray(value)) return value.length === 0;
  return String(value).trim() === "";
}

function renderMatchCompare(book, candidate) {
  const wrap = $("manage-meta-compare");
  const fields = $("manage-meta-compare-fields");
  const heading = $("manage-meta-compare-heading");
  if (!wrap || !fields) return;
  matchSelection = { book, candidate };
  wrap.hidden = false;
  fields.replaceChildren();
  if (heading) {
    heading.textContent = `Compare · ${candidate.provider}${
      candidate.title ? " · " + candidate.title : ""
    }`;
  }
  for (const field of MATCH_COMPARE_FIELDS) {
    const current = currentFieldValue(matchCurrentMeta, field);
    const next = candidateFieldValue(candidate, field);
    if (isMissingValue(current) && isMissingValue(next)) continue;
    const same =
      fieldDisplay(current) === fieldDisplay(next);
    const row = element("div", undefined, "meta-match-row");
    const check = document.createElement("input");
    check.type = "checkbox";
    check.dataset.field = field;
    check.checked = !same && !isMissingValue(next);
    check.disabled = isMissingValue(next);
    const key = element("span", field.replaceAll("_", " "), "meta-match-key");
    const cur = element("span", fieldDisplay(current), "meta-match-val");
    cur.dataset.side = "current";
    const cand = element("span", fieldDisplay(next), "meta-match-val");
    cand.dataset.side = "candidate";
    const lab = document.createElement("label");
    lab.append(check);
    row.append(lab, key, cur, cand);
    fields.append(row);
  }
  const applySelected = $("manage-meta-apply-selected");
  if (applySelected) applySelected.hidden = false;
}

async function applyMatchCandidate(book, candidate, fields) {
  await api(`books/${book.id}/metadata/apply`, {
    method: "POST",
    body: JSON.stringify({ candidate, fields, respect_locks: true }),
  });
  const meta = await api(`books/${book.id}/metadata`);
  matchCurrentMeta = meta;
  if ($("manage-meta-title") && (candidate.title || meta.title))
    $("manage-meta-title").value = candidate.title || meta.title || "";
  if ($("manage-meta-author")) {
    const author =
      (candidate.authors && candidate.authors[0]) ||
      (meta.authors && meta.authors[0]) ||
      meta.author ||
      "";
    if (author) $("manage-meta-author").value = author;
  }
  if ($("manage-meta-series") && (candidate.series_name || meta.series_name))
    $("manage-meta-series").value = candidate.series_name || meta.series_name || "";
  if ($("manage-meta-description"))
    $("manage-meta-description").value = meta.description || "";
  await loadManageMetadata(book);
}

/** BO-shaped match workspace: Strong / Possible / Weak rails + compare pane. */
function showMetaResults(book, payload) {
  const root = $("manage-meta-match");
  const box = $("manage-meta-results");
  const compare = $("manage-meta-compare");
  if (!box) return;
  matchSelection = null;
  if (compare) compare.hidden = true;
  box.replaceChildren();
  const groups = payload?.groups || {
    strong: [],
    possible: [],
    weak: (payload?.results || []).map((candidate) =>
      candidate.candidate
        ? candidate
        : { candidate, match: { tier: "weak" } },
    ),
  };
  const tiers = [
    ["strong", "Strong matches"],
    ["possible", "Possible matches"],
    ["weak", "Weak matches"],
  ];
  let total = 0;
  for (const [tier, label] of tiers) {
    const entries = groups[tier] || [];
    if (!entries.length) continue;
    total += entries.length;
    const section = element("section", undefined, "meta-match-tier");
    section.dataset.tier = tier;
    section.append(element("h4", `${label} · ${entries.length}`));
    const list = element("div", undefined, "meta-match-list");
    for (const entry of entries) {
      const candidate = entry.candidate || entry;
      const assessment = entry.match || {};
      const btn = element("button", undefined, "meta-result");
      btn.type = "button";
      btn.setAttribute("aria-pressed", "false");
      if (candidate.cover_url) {
        const img = element("img", undefined, "meta-result-cover");
        img.src = proxiedImageUrl(candidate.cover_url);
        img.alt = candidate.title || "Cover";
        img.loading = "lazy";
        img.referrerPolicy = "no-referrer";
        img.addEventListener("error", () => {
          const ph = element("div", "—", "meta-result-cover");
          ph.dataset.empty = "true";
          img.replaceWith(ph);
        });
        wireCoverPreview(img);
        btn.append(img);
      } else {
        const ph = element("div", "—", "meta-result-cover");
        ph.dataset.empty = "true";
        btn.append(ph);
      }
      const body = element("div", undefined, "meta-result-body");
      body.append(
        element("strong", candidate.title || "(no title)"),
        element(
          "p",
          `${candidate.provider}${
            candidate.authors?.length ? " · " + candidate.authors.join(", ") : ""
          }`,
          "fine",
        ),
      );
      if (assessment.isbn_match || assessment.linked) {
        body.append(
          element(
            "p",
            assessment.linked ? "Linked provider id" : "ISBN match",
            "fine",
          ),
        );
      }
      const badge = element(
        "span",
        assessment.tier ? assessment.tier : tier,
        "meta-result-badge",
      );
      btn.append(body, badge);
      btn.addEventListener("click", () => {
        for (const other of list.querySelectorAll(".meta-result")) {
          other.setAttribute("aria-pressed", "false");
        }
        btn.setAttribute("aria-pressed", "true");
        renderMatchCompare(book, candidate);
      });
      list.append(btn);
    }
    section.append(list);
    box.append(section);
  }
  if (!total) {
    box.append(element("p", "No matching records from enabled providers.", "meta-match-empty"));
  }
  if (root) root.hidden = false;
}

async function renderTtsProviders() {
  const list = $("tts-providers");
  const empty = $("tts-empty");
  if (!list || !empty) return;
  list.replaceChildren();
  try {
    const providers = await api("tts/admin/providers");
    empty.hidden = providers.length > 0;
    for (const provider of providers) {
      const card = element("article", undefined, "tts-provider");
      const title = element("h3", provider.name);
      const url = element("p", provider.baseUrl, "fine");
      const meta = element(
        "p",
        `${provider.enabled ? "Enabled" : "Disabled"} · ${(provider.staticVoices || []).length} voices · model ${provider.defaultModel || "—"}`,
        "fine",
      );
      const actions = element("div", undefined, "tts-actions");
      const toggle = element("button", provider.enabled ? "Disable" : "Enable", "quiet");
      toggle.type = "button";
      toggle.addEventListener("click", async () => {
        try {
          await api(`tts/admin/providers/${provider.id}`, {
            method: "PUT",
            body: JSON.stringify({ enabled: !provider.enabled }),
          });
          message(provider.enabled ? "TTS provider disabled." : "TTS provider enabled.");
          await renderTtsProviders();
        } catch (error) {
          message(error.message);
        }
      });
      const remove = element("button", "Remove", "quiet");
      remove.type = "button";
      remove.addEventListener("click", async () => {
        if (!confirm(`Remove TTS provider “${provider.name}”?`)) return;
        try {
          await api(`tts/admin/providers/${provider.id}`, { method: "DELETE" });
          message("TTS provider removed.");
          await renderTtsProviders();
        } catch (error) {
          message(error.message);
        }
      });
      actions.append(toggle, remove);
      card.append(title, url, meta, actions);
      list.append(card);
    }
  } catch (error) {
    empty.hidden = false;
    empty.textContent = error.message;
  }
}

function render() {
  const requested = location.hash.slice(1);
  const view = ["overview", "library", "insights", "settings"].includes(requested)
    ? requested
    : "overview";
  const library = view === "library";
  const insights = view === "insights";
  const settings = view === "settings";
  const overview = view === "overview";
  document.body.dataset.view = view;
  for (const link of document.querySelectorAll("[data-view]")) {
    if (link.dataset.view === view) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  $("heading").textContent = library
    ? "Your library."
    : insights
      ? "A life in pages."
      : settings
        ? "Settings."
        : "An evening well read.";
  const volumes = shelfBooks().length;
  $("subtitle").textContent = library
    ? `${volumes.toLocaleString()} ${volumes === 1 ? "volume" : "volumes"}`
    : insights
      ? "The reading you have recorded, so far."
      : settings
        ? "How the reading room runs, and what it is doing now."
        : "A little further into another world.";
  $("breadcrumb").textContent = `YOUR READING ROOM / ${view.toUpperCase()}`;
  $("section-title").replaceChildren(
    element("span", "01 /"),
    library ? " Your catalogue" : " Currently reading",
  );
  syncLibraryChrome(library);
  for (const button of document.querySelectorAll("#status-tabs [data-tab]")) {
    button.setAttribute("aria-pressed", String(button.dataset.tab === shelfTab));
  }
  $("masthead-search").hidden = !signedIn || library || settings;
  $("view-library").hidden = library || insights || settings;
  for (const id of ["ledger", "shelf-panel", "finished-panel"]) {
    $(id).hidden = !overview;
  }
  $("library-panel").hidden = insights || settings;
  $("insights-panel").hidden = !insights;
  $("settings-panel").hidden = !settings;
  if (settings && lastView !== "settings") {
    void renderTtsProviders();
    void renderMetaProviders();
    void renderLibraryRoots();
    startActivityPoll();
  } else if (!settings) {
    stopActivityPoll();
  }
  lastView = view;
  const query = $("search").value.toLowerCase();
  const fraction = (book) =>
    isImported(book) ? book.percentage : book.pages_read / book.total_pages;
  const foldedAway = hiddenEditionIds(catalogue, works);
  const source =
    library && libraryResults != null ? libraryResults : catalogue;
  const visible = source.filter((book) => {
    if (foldedAway.has(book.id)) return false;
    if (!library) {
      return isImported(book)
        ? book.position && book.percentage < 1
        : book.pages_read > 0 && book.pages_read < book.total_pages;
    }
    if (shelfTab !== "all" && shelfState(book) !== shelfTab) return false;
    if (libraryResults != null) return true;
    return `${book.title} ${book.author} ${book.series || ""}`
      .toLowerCase()
      .includes(query);
  });
  $("books").replaceChildren();
  if ($("load-more")) {
    $("load-more").hidden = !(library && hasMoreBooks);
  }
  $("empty").hidden = visible.length > 0;
  $("empty").querySelector("h3").textContent = catalogue.length
    ? library
      ? "No matching books."
      : "A new chapter is waiting."
    : "Your next chapter starts here.";
  $("empty").querySelector("p").textContent = catalogue.length
    ? library
      ? shelfTab !== "all"
        ? "Nothing on this shelf yet. Try All books."
        : "Try another title or author."
      : "Open Library to record your progress on a book."
    : "Add a book to begin building your library.";
  for (const book of visible) {
    const title = displayTitle(book.title);
    const article = element("article", undefined, "book");
    const cover = element("div", undefined, "cover");
    cover.setAttribute("aria-hidden", "true");
    cover.append(
      element("span", title),
      element("small", "SHELF / CATALOGUE"),
    );
    if (book.has_cover) {
      cover.replaceChildren();
      const image = element("img");
      image.src = `/api/v1/books/${book.id}/cover`;
      image.alt = "";
      image.loading = "lazy";
      cover.append(image);
    }
    const info = element("div");
    info.append(
      element("h3", title),
      element("p", book.author || "Author not recorded", "author"),
    );
    const progress = element("progress");
    progress.max = 1;
    progress.value = fraction(book);
    progress.setAttribute(
      "aria-label",
      `${title}: ${Math.round(fraction(book) * 100)}%`,
    );
    const update = element("button", "Record progress", "quiet");
    update.addEventListener("click", () => {
      progressBook = book;
      $("progress-book").textContent =
        `${title} · ${book.total_pages} pages`;
      $("progress-pages").max = book.total_pages;
      $("progress-pages").value = book.pages_read;
      $("progress-error").hidden = true;
      $("progress-dialog").showModal();
    });
    const actions = element("div", undefined, "book-actions");
    const pair = pairedAudioForEpub(book, catalogue);
    const pairedAudio = pair?.audio ?? null;
    const pairWork = pair?.work ?? null;
    const readaloud =
      pairWork?.readaloud_book_id != null
        ? catalogue.find((b) => b.id === pairWork.readaloud_book_id) || null
        : null;
    if (pairedAudio) {
      const open = element("button", "Open →", "quiet");
      open.type = "button";
      open.addEventListener("click", () => {
        openEdition = {
          title,
          epub: book,
          audio: pairedAudio,
          readaloud,
          work: pairWork,
        };
        $("open-edition-title").textContent = title;
        $("open-edition-help").textContent = readaloud
          ? "Read opens the aligned edition (highlight + narration). Listen opens the audiobook player."
          : "Choose Read for the ebook or Listen for the audiobook.";
        $("open-edition-dialog").showModal();
      });
      actions.append(open);
    } else if (isPlayableAudiobook(book)) {
      actions.append(
        Object.assign(
          element(
            "a",
            book.position ? "Continue listening →" : "Listen →",
            "quiet",
          ),
          { href: readerHref(book) },
        ),
      );
    } else if (book.format === "aax") {
      actions.append(element("p", audioStatusLine(book), "fine"));
    } else if (isImported(book)) {
      actions.append(
        Object.assign(
          element(
            "a",
            book.position ? "Continue reading →" : "Read book →",
            "quiet",
          ),
          { href: readerHref(book) },
        ),
      );
    } else {
      actions.append(update);
    }
    const manage = element("button", undefined, "icon-button");
    manage.type = "button";
    manage.setAttribute("aria-label", `Manage ${title}`);
    manage.title = "Manage book";
    const manageIcon = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "svg",
    );
    manageIcon.setAttribute("class", "icon-button-glyph");
    manageIcon.setAttribute("viewBox", "0 0 24 24");
    manageIcon.setAttribute("aria-hidden", "true");
    const manageUse = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "use",
    );
    manageUse.setAttribute("href", "/icons.svg#edit");
    manageIcon.append(manageUse);
    manage.append(manageIcon);
    manage.addEventListener("click", () => {
      removeBook = book;
      $("manage-title").textContent = title;
      $("manage-error").hidden = true;
      $("manage-meta-title").value = displayTitle(book.title);
      $("manage-meta-author").value = book.author || "";
      if ($("manage-meta-series"))
        $("manage-meta-series").value = book.series || "";
      if ($("manage-meta-series-index"))
        $("manage-meta-series-index").value =
          book.series_index != null && book.series_index !== ""
            ? book.series_index
            : "";
      if ($("manage-collection")) $("manage-collection").value = "";
      $("manage-meta-submit").disabled = false;
      const finished = isFinished(book);
      $("manage-finish").hidden = finished;
      $("manage-finish").disabled = finished;
      $("manage-unfinished").hidden = !finished;
      $("manage-unfinished").disabled = !finished;
      $("manage-rereading").hidden = !finished;
      $("manage-rereading").disabled = !finished;
      $("manage-help").textContent = manageHelp(book);
      const candidates = mergeCandidates(book, catalogue);
      const mergeForm = $("manage-merge");
      const mergeSelect = $("manage-merge-with");
      const mergeStatus = $("manage-merge-status");
      mergeStatus.hidden = true;
      mergeStatus.textContent = "";
      $("manage-merge-submit").disabled = false;
      mergeSelect.replaceChildren();
      if (candidates.length) {
        for (const other of candidates) {
          const opt = document.createElement("option");
          opt.value = String(other.id);
          opt.textContent = `${displayTitle(other.title)} · ${formatLabel(other)}`;
          mergeSelect.append(opt);
        }
        $("manage-merge-title").value = displayTitle(book.title);
        mergeForm.hidden = false;
      } else {
        mergeForm.hidden = true;
      }
      $("manage-dialog").showModal();
    });
    let combineStatus = null;
    if (pairedAudio) {
      combineStatus = element("div", undefined, "align-status");
      combineStatus.hidden = true;
      const busy =
        pairWork &&
        (pairWork.status === "processing" || pairWork.status === "queued");
      const failed = pairWork && pairWork.status === "failed";
      const ready = pairWork && pairWork.status === "ready" && readaloud;
      if (busy || failed) setAlignStatus(combineStatus, pairWork);
      if (!ready && !busy) {
        const combine = element(
          "button",
          failed ? "Retry Combine →" : "Combine →",
          "quiet",
        );
        combine.type = "button";
        combine.addEventListener("click", async () => {
          combine.disabled = true;
          try {
            const result = await combineEditions(
              book,
              pairedAudio,
              combineStatus,
            );
            if (result.status !== "ready") combine.disabled = false;
          } catch (error) {
            setAlignStatus(combineStatus, {
              status: "failed",
              error: error.message || "Combine failed.",
              progress: 1,
            });
            combine.disabled = false;
          }
        });
        actions.append(combine);
      }
    }
    actions.append(manage);
    const metaLabel = pairedAudio
      ? `${Math.round(book.percentage * 100)}% read · Ebook + audiobook`
      : isImported(book)
        ? `${Math.round(book.percentage * 100)}% ${
            isAudio(book) ? "played" : "read"
          } · ${formatLabel(book)}`
        : `${book.pages_read} / ${book.total_pages} pages · ${Math.round(fraction(book) * 100)}%`;
    info.append(progress, element("p", metaLabel, "fine"));
    if (library) {
      const tags = genreTags(book);
      if (book.series || tags) {
        const ledger = element("div", undefined, "catalogue-no");
        if (book.series) ledger.append(element("p", book.series, "card-series"));
        if (tags) ledger.append(tags);
        info.append(ledger);
      }
    }
    info.append(actions);
    if (combineStatus) info.append(combineStatus);
    article.append(cover, info);
    $("books").append(article);
  }
  if (overview) {
    renderShelfStrip();
    renderFinished();
  }
  if (insights) {
    renderAuthorBreakdown();
    renderCalendar();
  }
  $("progress-list").replaceChildren();
  for (const book of shelfBooks()) {
    const row = element("div", undefined, "progress-row");
    const bar = element("progress");
    bar.max = 1;
    bar.value = fraction(book);
    bar.setAttribute(
      "aria-label",
      `${book.title}: ${Math.round(fraction(book) * 100)}%`,
    );
    row.append(
      element("span", displayTitle(book.title)),
      bar,
      element("span", `${Math.round(fraction(book) * 100)}%`, "fine"),
    );
    $("progress-list").append(row);
  }
  if (!catalogue.length)
    $("progress-list").append(
      element("p", "Add a book to start recording your reading."),
    );
}
$("auth-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const button = $("auth-submit");
  button.disabled = true;
  const credentials = {
    username: form.elements.username.value,
    password: form.elements.password.value,
  };
  try {
    if (setupRequired) {
      await api("setup", { method: "POST", body: JSON.stringify(credentials) });
      setupRequired = false;
    }
    await api("login", { method: "POST", body: JSON.stringify(credentials) });
    form.reset();
    message("");
    await refresh();
  } catch (error) {
    message(error.message);
  } finally {
    button.disabled = false;
  }
});
$("signout").addEventListener("click", async () => {
  try {
    await api("logout", { method: "POST" });
    catalogue = [];
    showAuth(false);
    message("Signed out.");
  } catch (error) {
    message(error.message);
  }
});
$("close-progress").addEventListener("click", () =>
  $("progress-dialog").close(),
);
$("close-manage").addEventListener("click", () => $("manage-dialog").close());
$("close-cover-lightbox")?.addEventListener("click", () => $("cover-lightbox")?.close());
$("cover-lightbox")?.addEventListener("click", (event) => {
  if (event.target === $("cover-lightbox")) $("cover-lightbox").close();
});

$("close-open-edition").addEventListener("click", () =>
  $("open-edition-dialog").close(),
);
$("open-edition-read").addEventListener("click", () => {
  if (!openEdition) return;
  const target = openEdition.readaloud || openEdition.epub;
  location.href = readerHref(target);
});
$("open-edition-listen").addEventListener("click", () => {
  if (!openEdition?.audio) return;
  location.href = readerHref(openEdition.audio);
});
async function setReadingStatus(path, okMessage, buttonId) {
  if (!removeBook) return;
  const button = $(buttonId);
  button.disabled = true;
  $("manage-error").hidden = true;
  try {
    await api(`books/${removeBook.id}/${path}`, { method: "POST" });
    $("manage-dialog").close();
    removeBook = null;
    await refresh();
    message(okMessage);
  } catch (error) {
    $("manage-error").textContent = error.message;
    $("manage-error").hidden = false;
    button.disabled = false;
  }
}
$("manage-meta").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!removeBook) return;
  const title = $("manage-meta-title").value.trim();
  const author = $("manage-meta-author").value.trim();
  const seriesRaw = ($("manage-meta-series")?.value || "").trim();
  const seriesIndexRaw = ($("manage-meta-series-index")?.value || "").trim();
  if (!title) return;
  const button = $("manage-meta-submit");
  button.disabled = true;
  $("manage-error").hidden = true;
  const payload = {
    title,
    author,
  };
  // Only patch series when the inputs have a value, or when clearing a known value.
  if (seriesRaw) {
    payload.series = seriesRaw;
    payload.series_index =
      seriesIndexRaw === "" ? null : Number(seriesIndexRaw);
  } else if (removeBook.series) {
    // Explicit clear — user blanked a previously set series.
    payload.series = null;
    payload.series_index = null;
  }
  try {
    await api(`books/${removeBook.id}`, {
      method: "PATCH",
      body: JSON.stringify(payload),
    });
    const description = ($("manage-meta-description")?.value || "").trim();
    // Never send description:null from an empty box; that wiped autofill on early Save.
    if (description) {
      await api(`books/${removeBook.id}/metadata`, {
        method: "PATCH",
        body: JSON.stringify({ description }),
      });
    }
    const collectionId = $("manage-collection")?.value;
    if (collectionId) {
      await api(`collections/${collectionId}`, {
        method: "PATCH",
        body: JSON.stringify({ add_book_ids: [removeBook.id] }),
      });
    }
    $("manage-dialog").close();
    removeBook = null;
    await refresh();
    message("Metadata saved.");
  } catch (error) {
    $("manage-error").textContent = error.message;
    $("manage-error").hidden = false;
    button.disabled = false;
  }
});
$("manage-finish").addEventListener("click", () => {
  if (!removeBook || isFinished(removeBook)) return;
  setReadingStatus("finished", "Marked as finished.", "manage-finish");
});
$("manage-unfinished").addEventListener("click", () => {
  if (!removeBook || !isFinished(removeBook)) return;
  setReadingStatus("unfinished", "Marked as unfinished.", "manage-unfinished");
});
$("manage-rereading").addEventListener("click", () => {
  if (!removeBook || !isFinished(removeBook)) return;
  setReadingStatus("rereading", "Marked as re-reading.", "manage-rereading");
});
$("manage-merge").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!removeBook) return;
  const otherId = Number($("manage-merge-with").value);
  const other = catalogue.find((book) => book.id === otherId);
  const mergedTitle = $("manage-merge-title").value.trim();
  if (!other || !mergedTitle) return;
  const epubBook =
    removeBook.format === "epub"
      ? removeBook
      : other.format === "epub"
        ? other
        : null;
  const audioBook = isPlayableAudiobook(removeBook)
    ? removeBook
    : isPlayableAudiobook(other)
      ? other
      : null;
  if (!epubBook || !audioBook) {
    $("manage-error").textContent =
      "Merge needs one EPUB and one playable audiobook.";
    $("manage-error").hidden = false;
    return;
  }
  const button = $("manage-merge-submit");
  const statusEl = $("manage-merge-status");
  button.disabled = true;
  $("manage-error").hidden = true;
  try {
    const result = await combineEditions(
      epubBook,
      audioBook,
      statusEl,
      mergedTitle,
    );
    if (result.status === "ready") {
      $("manage-dialog").close();
      removeBook = null;
      message("Read-aloud ready.");
      return;
    }
    await refresh();
    statusEl.hidden = false;
    statusEl.textContent =
      result.status === "failed"
        ? result.error || "Alignment failed."
        : "Still aligning in the background — refresh the library when it finishes.";
    button.disabled = false;
  } catch (error) {
    $("manage-error").textContent = error.message || "Merge failed.";
    $("manage-error").hidden = false;
    button.disabled = false;
  }
});
$("manage-remove").addEventListener("click", () => {
  if (!removeBook) return;
  $("manage-dialog").close();
  $("remove-title").textContent = displayTitle(removeBook.title);
  $("remove-error").hidden = true;
  $("remove-dialog").showModal();
});
$("close-remove").addEventListener("click", () => $("remove-dialog").close());
$("cancel-remove").addEventListener("click", () => $("remove-dialog").close());
$("confirm-remove").addEventListener("click", async () => {
  if (!removeBook) return;
  const button = $("confirm-remove");
  button.disabled = true;
  $("remove-error").hidden = true;
  try {
    await api(`books/${removeBook.id}`, { method: "DELETE" });
    $("remove-dialog").close();
    removeBook = null;
    await refresh();
    message("Book removed.");
  } catch (error) {
    $("remove-error").textContent = error.message;
    $("remove-error").hidden = false;
  } finally {
    button.disabled = false;
  }
});
$("progress-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const button = event.currentTarget.querySelector(".primary");
  button.disabled = true;
  try {
    await api(`books/${progressBook.id}/progress`, {
      method: "POST",
      body: JSON.stringify({ pages_read: Number($("progress-pages").value) }),
    });
    $("progress-dialog").close();
    await refresh();
    message("Reading progress saved.");
  } catch (error) {
    $("progress-error").hidden = false;
    $("progress-error").textContent = error.message;
  } finally {
    button.disabled = false;
  }
});
$("search").addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    const q = ($("search").value || "").trim();
    if (hasActiveFilters() || q) {
      void applyLibraryFilters().catch((error) => message(error.message));
    } else {
      libraryResults = null;
      hasMoreBooks = catalogue.length === BOOK_LIMIT;
      bookOffset = catalogue.length;
      render();
    }
  }, 250);
});
window.addEventListener("hashchange", render);
$("status-tabs").addEventListener("click", (event) => {
  const button = event.target.closest("[data-tab]");
  if (!button) return;
  shelfTab = button.dataset.tab;
  render();
});
$("masthead-search").addEventListener("submit", (event) => {
  event.preventDefault();
  $("search").value = $("masthead-q").value;
  $("masthead-q").value = "";
  location.hash = "library";
  $("search").dispatchEvent(new Event("input"));
  $("search").focus();
});
$("search-key").textContent = /Mac|iPhone|iPad/.test(navigator.platform || "")
  ? "⌘K"
  : "Ctrl K";
document.addEventListener("keydown", (event) => {
  if (event.key?.toLowerCase() !== "k" || !(event.metaKey || event.ctrlKey)) return;
  if (!signedIn) return;
  event.preventDefault();
  if (location.hash === "#library" || location.hash === "#settings") {
    location.hash = "library";
    $("search").focus();
  } else {
    $("masthead-q").focus();
  }
});
$("almanac").textContent = `${romanYear(new Date().getFullYear())}\nA calmer\nreading life`;
$("filter-apply")?.addEventListener("click", () => {
  void applyLibraryFilters().catch((error) => message(error.message));
});
$("filter-clear")?.addEventListener("click", () => {
  writeFiltersToForm({
    author: "",
    series: "",
    status: "",
    format: "",
    collection_id: "",
    query: "",
  });
  libraryResults = null;
  hasMoreBooks = catalogue.length === BOOK_LIMIT;
  bookOffset = catalogue.length;
  render();
});
$("load-more")?.addEventListener("click", () => {
  void loadMoreBooks().catch((error) => message(error.message));
});
$("scope-save")?.addEventListener("click", async () => {
  const name = ($("scope-name")?.value || "").trim();
  if (!name) {
    message("Name the scope before saving.");
    return;
  }
  const filters = readFiltersFromForm();
  const query = ($("search")?.value || "").trim();
  try {
    await api("scopes", {
      method: "POST",
      body: JSON.stringify({
        name,
        filter: {
          author: filters.author || null,
          series: filters.series || null,
          status: filters.status || null,
          format: filters.format || null,
          query: query || null,
          collection_id: filters.collection_id
            ? Number(filters.collection_id) || filters.collection_id
            : null,
        },
      }),
    });
    $("scope-name").value = "";
    await loadOrg();
    message(`Scope “${name}” saved.`);
  } catch (error) {
    message(error.message);
  }
});
$("collection-create")?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const name = ($("collection-name")?.value || "").trim();
  if (!name) return;
  try {
    await api("collections", {
      method: "POST",
      body: JSON.stringify({ name }),
    });
    $("collection-name").value = "";
    await loadOrg();
    message(`Collection “${name}” created.`);
  } catch (error) {
    message(error.message);
  }
});
$("library-scan")?.addEventListener("click", async () => {
  try {
    await runBusy($("library-scan"), "Scanning…", async () => {
      const result = await api("library/scan", {
        method: "POST",
        body: JSON.stringify({}),
      });
      const imported = result?.imported ?? result?.added ?? 0;
      const duplicates = result?.duplicates ?? 0;
      const failed = result?.failed ?? result?.errors?.length ?? 0;
      await refresh();
      void refreshActivity();
      message(
        `Scan finished. ${imported} imported, ${duplicates} duplicates, ${failed} failed.`,
      );
    });
  } catch (error) {
    message(error.message);
  }
});
for (const link of document.querySelectorAll(".settings-index a")) {
  link.addEventListener("click", (event) => {
    event.preventDefault();
    const target = document.getElementById(link.getAttribute("href").slice(1));
    target?.scrollIntoView({
      behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
      block: "start",
    });
    target?.querySelector("h2")?.focus({ preventScroll: true });
  });
}
$("tts-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target;
  const error = $("tts-error");
  error.hidden = true;
  const button = form.querySelector("button[type=submit]");
  button.disabled = true;
  try {
    const data = new FormData(form);
    const payload = {
      name: String(data.get("name") || "").trim(),
      baseUrl: String(data.get("baseUrl") || "").trim(),
      apiKey: String(data.get("apiKey") || "").trim() || undefined,
      defaultModel: String(data.get("defaultModel") || "").trim() || undefined,
      supportsVoiceDiscovery: true,
      staticVoices: data.get("loadKokoroVoices") ? KOKORO_VOICES : [],
    };
    await api("tts/admin/providers", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    form.reset();
    form.elements.defaultModel.value = "kokoro";
    form.elements.loadKokoroVoices.checked = true;
    message("TTS provider saved. Open an EPUB and use Listen.");
    await renderTtsProviders();
  } catch (err) {
    error.hidden = false;
    error.textContent = err.message;
  } finally {
    button.disabled = false;
  }
});
async function start() {
  try {
    const status = await api("setup");
    if (status.setup_required) showAuth(true);
    else {
      try {
        await refresh();
      } catch {
        showAuth(false);
      }
    }
  } catch (error) {
    message(`Could not connect to Shelf. ${error.message}`);
  }
}
start();

$("open-add").addEventListener("click", () => $("epub-files").click());
$("epub-files").addEventListener("change", async () => {
  const files = [...$("epub-files").files];
  $("open-add").disabled = true;
  let added = 0,
    duplicates = 0;
  const errors = [];
  for (const file of files) {
    message(`Importing ${file.name}…`);
    try {
      const name = file.name.toLowerCase();
      const contentType =
        file.type === "application/pdf" || name.endsWith(".pdf")
          ? "application/pdf"
          : file.type === "application/vnd.comicbook+zip" || name.endsWith(".cbz")
            ? "application/vnd.comicbook+zip"
            : name.endsWith(".aax")
              ? "audio/vnd.audible.aax"
              : name.endsWith(".mp3")
              ? "audio/mpeg"
              : name.endsWith(".m4b") || name.endsWith(".m4a")
                ? "audio/mp4"
                : name.endsWith(".opus")
                  ? "audio/ogg"
                  : name.endsWith(".ogg") || name.endsWith(".oga")
                    ? "audio/ogg"
                    : name.endsWith(".flac")
                      ? "audio/flac"
                      : name.endsWith(".zip")
                        ? "application/zip"
                        : name.endsWith(".cbr") ||
                    name.endsWith(".cb7") ||
                    name.endsWith(".7z") ||
                    name.endsWith(".djvu") ||
                    name.endsWith(".djv")
                  ? "application/octet-stream"
                  : "application/epub+zip";
      const result = await api("books/import", {
        method: "POST",
        headers: {
          "Content-Type": contentType,
          "X-Shelf-Filename": file.name,
        },
        body: file,
      });
      if (result.duplicate) duplicates++;
      else added++;
    } catch (error) {
      errors.push(`${file.name}: ${error.message}`);
    }
  }
  $("epub-files").value = "";
  $("open-add").disabled = false;
  location.hash = "library";
  try {
    await refresh();
  } catch (error) {
    errors.push(error.message);
  }
  message(
    `${added} imported. ${duplicates} already in your library. ${errors.join(" ")}`,
  );
});

$("manage-meta-search")?.addEventListener("click", async () => {
  if (!removeBook) return;
  const button = $("manage-meta-search");
  const err = $("manage-error");
  if (err) {
    err.hidden = true;
    err.textContent = "";
  }
  await runBusy(button, "Searching…", async () => {
    try {
      const root = $("manage-meta-match");
      const box = $("manage-meta-results");
      if (root) root.hidden = false;
      if (box) {
        box.replaceChildren();
        box.append(element("p", "Searching providers…", "meta-match-empty"));
      }
      matchCurrentMeta = await api(`books/${removeBook.id}/metadata`).catch(() => null);
      const data = await api("metadata/search", {
        method: "POST",
        body: JSON.stringify({
          title: $("manage-meta-title").value.trim(),
          author: $("manage-meta-author").value.trim(),
          book_id: removeBook.id,
          limit: 5,
        }),
      });
      showMetaResults(removeBook, data);
      const n = (data.results || []).length;
      const strong = (data.groups?.strong || []).length;
      manageStatus(
        strong
          ? `Found ${n} hits · ${strong} strong.`
          : `Found ${n} metadata hit${n === 1 ? "" : "s"}.`,
      );
    } catch (error) {
      if (err) {
        err.textContent = error.message;
        err.hidden = false;
      }
      manageStatus(error.message);
      throw error;
    }
  });
});

$("manage-meta-apply-all")?.addEventListener("click", async () => {
  if (!matchSelection) return;
  try {
    await applyMatchCandidate(
      matchSelection.book,
      matchSelection.candidate,
      MATCH_APPLY_ALL_FIELDS,
    );
    message("Provider metadata applied.");
  } catch (error) {
    message(error.message);
  }
});
$("manage-meta-apply-missing")?.addEventListener("click", async () => {
  if (!matchSelection) return;
  try {
    const fields = MATCH_COMPARE_FIELDS.filter((field) => {
      const current = currentFieldValue(matchCurrentMeta, field);
      const next = candidateFieldValue(matchSelection.candidate, field);
      return isMissingValue(current) && !isMissingValue(next);
    });
    if (!fields.length) {
      message("Nothing missing to apply.");
      return;
    }
    // Always include cover when the candidate has one and we lack an active sense of description-level missing set.
    if (matchSelection.candidate.cover_url) fields.push("cover");
    await applyMatchCandidate(
      matchSelection.book,
      matchSelection.candidate,
      fields,
    );
    message("Missing fields applied.");
  } catch (error) {
    message(error.message);
  }
});
$("manage-meta-apply-selected")?.addEventListener("click", async () => {
  if (!matchSelection) return;
  try {
    const fields = [
      ...document.querySelectorAll(
        "#manage-meta-compare-fields input[type=checkbox]:checked",
      ),
    ].map((el) => el.dataset.field);
    if (!fields.length) {
      message("Select at least one field.");
      return;
    }
    await applyMatchCandidate(
      matchSelection.book,
      matchSelection.candidate,
      fields,
    );
    message("Selected fields applied.");
  } catch (error) {
    message(error.message);
  }
});

$("manage-meta-cover-search")?.addEventListener("click", async () => {
  const book = removeBook;
  if (!book) return;
  const box = $("manage-meta-cover-results");
  const button = $("manage-meta-cover-search");
  if (!box) return;
  box.hidden = false;
  await runBusy(button, "Finding covers…", async () => {
    box.replaceChildren();
    box.append(element("p", "Searching cover sources…", "fine"));
    try {
      const title = $("manage-meta-title")?.value || book.title || "";
      const author = $("manage-meta-author")?.value || book.author || "";
      const data = await api(`books/${book.id}/covers/search`, {
        method: "POST",
        body: JSON.stringify({ title, author, audiobook: false }),
      });
      box.replaceChildren();
      const hits = data.results || [];
      if (!hits.length) {
        box.append(element("p", "No covers found.", "fine"));
        manageStatus("No covers found.");
        return;
      }
      for (const hit of hits.slice(0, 16)) {
        const fig = document.createElement("figure");
        const remote = hit.preview_url || hit.url;
        const img = document.createElement("img");
        img.alt = hit.source || "cover";
        img.src = proxiedImageUrl(remote);
        img.loading = "lazy";
        img.referrerPolicy = "no-referrer";
        img.addEventListener("error", () => {
          img.removeAttribute("src");
          img.alt = "Preview unavailable";
        });
        wireCoverPreview(img);
        const source = element("p", hit.source || "cover", "fine");
        const btn = element("button", "Use", "quiet");
        btn.type = "button";
        btn.title = hit.source || "";
        btn.addEventListener("click", async () => {
          await runBusy(btn, "Saving…", async () => {
            await api(`books/${book.id}/covers`, {
              method: "POST",
              body: JSON.stringify({ url: hit.url, medium: "ebook" }),
            });
            await loadManageMetadata(book);
            manageStatus("Cover saved.");
          });
        });
        fig.append(img, source, btn);
        box.append(fig);
      }
      manageStatus(`Found ${hits.length} cover${hits.length === 1 ? "" : "s"}.`);
    } catch (error) {
      box.replaceChildren();
      box.append(element("p", error.message, "fine"));
      manageStatus(error.message);
      throw error;
    }
  });
});

$("manage-meta-autofill")?.addEventListener("click", async () => {
  if (!removeBook) return;
  const button = $("manage-meta-autofill");
  const err = $("manage-error");
  if (err) {
    err.hidden = true;
    err.textContent = "";
  }
  await runBusy(button, "Auto-filling…", async () => {
    try {
      const result = await api(`books/${removeBook.id}/metadata/autofill`, {
        method: "POST",
        body: "{}",
      });
      // Refresh catalogue row so series/title survive Save + reopen.
      try {
        const books = await api("books");
        const fresh = (books || []).find((b) => b.id === removeBook.id);
        if (fresh) removeBook = fresh;
      } catch (_) {
        /* keep prior row */
      }
      await loadManageMetadata(removeBook);
      const applied = Array.isArray(result?.applied) ? result.applied : [];
      const fields = [
        ...new Set(applied.map((a) => a.field).filter(Boolean)),
      ];
      manageStatus(
        fields.length
          ? `Auto-fill applied ${fields.length} field${
              fields.length === 1 ? "" : "s"
            }: ${fields.join(", ")}.`
          : "Auto-fill finished — nothing new to apply.",
      );
    } catch (error) {
      if (err) {
        err.textContent = error.message;
        err.hidden = false;
      }
      manageStatus(error.message);
      throw error;
    }
  });
});
$("manage-meta-refresh")?.addEventListener("click", async () => {
  if (!removeBook) return;
  const button = $("manage-meta-refresh");
  const err = $("manage-error");
  if (err) {
    err.hidden = true;
    err.textContent = "";
  }
  await runBusy(button, "Queueing refresh…", async () => {
    try {
      await api(`books/${removeBook.id}/metadata/refresh`, {
        method: "POST",
        body: "{}",
      });
      manageStatus("Refresh queued — providers will re-fetch in the background.");
      await refreshMetaFetchStatus();
    } catch (error) {
      if (err) {
        err.textContent = error.message;
        err.hidden = false;
      }
      manageStatus(error.message);
      throw error;
    }
  });
});
async function metaAction(id, busyLabel, path, body, done) {
  try {
    await runBusy($(id), busyLabel, async () => {
      const res = await api(path, { method: "POST", body: JSON.stringify(body) });
      manageStatus("");
      message(done ? done(res) : "");
      await refreshActivity();
    });
  } catch (error) {
    message(error.message);
  }
}
$("meta-run")?.addEventListener("click", () =>
  metaAction(
    "meta-run",
    "Queuing…",
    "metadata/fetch/run",
    { ignore_cache: !!$("meta-ignore-cache")?.checked },
    (res) => `Queued ${res.queued} books. Follow progress under Activity.`,
  ),
);
$("meta-pause")?.addEventListener("click", () =>
  metaAction("meta-pause", "Pausing…", "metadata/fetch/pause", {}),
);
$("meta-resume")?.addEventListener("click", () =>
  metaAction("meta-resume", "Resuming…", "metadata/fetch/resume", {}),
);
$("meta-cancel")?.addEventListener("click", () =>
  metaAction(
    "meta-cancel",
    "Cancelling…",
    "metadata/fetch/cancel",
    {},
    (res) => `Cancelled ${res.cancelled} jobs.`,
  ),
);

