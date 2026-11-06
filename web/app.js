"use strict";
import { request } from "./api.mjs";
const $ = (id) => document.getElementById(id);
let catalogue = [];
let setupRequired = false;
let signedIn = false;
let progressBook = null;
let removeBook = null;
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
function isImported(book) {
  return (
    book.format === "epub" ||
    book.format === "pdf" ||
    book.format === "cbz" ||
    book.format === "cbr" ||
    book.format === "cb7" ||
    book.format === "djvu"
  );
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
  for (const id of ["progress-dialog", "manage-dialog", "remove-dialog"])
    if ($(id).open) $(id).close();
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
  const [books, stats] = await Promise.all([api("books"), api("stats")]);
  catalogue = books;
  signedIn = true;
  $("auth").hidden = true;
  $("content").hidden = false;
  $("open-add").hidden = false;
  $("signout").hidden = false;
  for (const [id, key] of [
    ["books", "books"],
    ["reading", "reading"],
    ["finished", "finished"],
  ]) {
    $(`stat-${id}`).textContent = stats[key].toLocaleString();
  }
  // EPUB has no printed page count (decision 0003); show mean completion instead.
  $("stat-pages").textContent =
    `${Math.round(Number(stats.average_progress || 0) * 100)}%`;
  render();
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
  const visible = catalogue.filter((book) =>
    library
      ? `${book.title} ${book.author}`.toLowerCase().includes(query)
      : isImported(book)
        ? book.position && book.percentage < 1
        : book.pages_read > 0 && book.pages_read < book.total_pages,
  );
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
    if (isImported(book)) {
      actions.append(
        Object.assign(
          element(
            "a",
            book.position ? "Continue reading →" : "Read book →",
            "quiet",
          ),
          {
            href: `/reader?id=${book.id}${
              book.format === "pdf" ||
              book.format === "cbz" ||
              book.format === "cbr" ||
              book.format === "cb7" ||
              book.format === "djvu"
                ? `&format=${book.format}`
                : ""
            }`,
          },
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
      $("manage-dialog").showModal();
    });
    actions.append(manage);
    info.append(
      progress,
      element(
        "p",
        isImported(book)
          ? `${Math.round(book.percentage * 100)}% read · ${
              book.format === "pdf"
                ? "PDF"
                : book.format === "cbz"
                  ? "CBZ"
                  : book.format === "cbr"
                    ? "CBR"
                    : book.format === "cb7"
                      ? "CB7"
                      : book.format === "djvu"
                        ? "DjVu"
                        : "EPUB"
            }`
          : `${book.pages_read} / ${book.total_pages} pages · ${Math.round(fraction(book) * 100)}%`,
        "fine",
      ),
      actions,
    );
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
