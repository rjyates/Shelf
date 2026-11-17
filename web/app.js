"use strict";
import { request } from "./api.mjs";
const $ = (id) => document.getElementById(id);
let catalogue = [];
let works = [];
let setupRequired = false;
let signedIn = false;
let progressBook = null;
let removeBook = null;
let openEdition = null;
let alignPollTimer = null;
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
function showAuth(setup) {
  setupRequired = setup;
  signedIn = false;
  $("auth").hidden = false;
  $("content").hidden = true;
  $("open-add").hidden = true;
  $("signout").hidden = true;
  for (const id of [
    "progress-dialog",
    "manage-dialog",
    "remove-dialog",
    "open-edition-dialog",
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
  const [books, workList] = await Promise.all([
    api("books"),
    api("works").catch(() => []),
  ]);
  catalogue = books;
  works = Array.isArray(workList) ? workList : [];
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
  render();
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
  for (const link of document.querySelectorAll("[data-view]")) {
    if (link.dataset.view === view) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  $("heading").textContent = library
    ? "Your library."
    : insights
      ? "A life in pages."
      : settings
        ? "Listening setup."
        : "An evening well read.";
  $("subtitle").textContent = library
    ? "A home for every chapter."
    : insights
      ? "The reading you have recorded, so far."
      : settings
        ? "Configure a speech provider before the reader offers listen."
        : "A little further into another world.";
  $("breadcrumb").textContent = `YOUR READING ROOM / ${view.toUpperCase()}`;
  $("section-title").textContent = library
    ? "01 / Your catalogue"
    : "01 / Currently reading";
  $("search-label").hidden = !library;
  $("view-library").hidden = library || insights || settings;
  $("library-panel").hidden = insights || settings;
  $("insights-panel").hidden = !insights;
  $("settings-panel").hidden = !settings;
  if (settings) void renderTtsProviders();
  const query = $("search").value.toLowerCase();
  const fraction = (book) =>
    isImported(book) ? book.percentage : book.pages_read / book.total_pages;
  const foldedAway = hiddenEditionIds(catalogue, works);
  const visible = catalogue.filter((book) => {
    if (foldedAway.has(book.id)) return false;
    return library
      ? `${book.title} ${book.author}`.toLowerCase().includes(query)
      : isImported(book)
        ? book.position && book.percentage < 1
        : book.pages_read > 0 && book.pages_read < book.total_pages;
  });
  $("books").replaceChildren();
  $("empty").hidden = visible.length > 0;
  $("empty").querySelector("h3").textContent = catalogue.length
    ? library
      ? "No matching books."
      : "A new chapter is waiting."
    : "Your next chapter starts here.";
  $("empty").querySelector("p").textContent = catalogue.length
    ? library
      ? "Try another title or author."
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
    info.append(progress, element("p", metaLabel, "fine"), actions);
    if (combineStatus) info.append(combineStatus);
    article.append(cover, info);
    $("books").append(article);
  }
  $("progress-list").replaceChildren();
  for (const book of catalogue) {
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
$("search").addEventListener("input", render);
window.addEventListener("hashchange", render);
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
