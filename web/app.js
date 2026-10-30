"use strict";
import { request } from "./api.mjs";
const $ = (id) => document.getElementById(id);
let catalogue = [];
let setupRequired = false;
let signedIn = false;
let progressBook = null;
async function api(path, options = {}) {
  try {
    return await request(path, options);
  } catch (error) {
    if (error.status === 401 && signedIn) showAuth(false);
    throw error;
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
  for (const id of ["add-dialog", "progress-dialog"])
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
    ["pages", "pages_read"],
  ]) {
    $(`stat-${id}`).textContent = stats[key].toLocaleString();
  }
  render();
}
function element(tag, text, className) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}
function render() {
  const requested = location.hash.slice(1);
  const view = ["overview", "library", "insights"].includes(requested)
    ? requested
    : "overview";
  const library = view === "library";
  const insights = view === "insights";
  for (const link of document.querySelectorAll("[data-view]")) {
    if (link.dataset.view === view) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  $("heading").textContent = library
    ? "Your library."
    : insights
      ? "A life in pages."
      : "An evening well read.";
  $("subtitle").textContent = library
    ? "A home for every chapter."
    : insights
      ? "The reading you have recorded, so far."
      : "A little further into another world.";
  $("breadcrumb").textContent = `YOUR READING ROOM / ${view.toUpperCase()}`;
  $("section-title").textContent = library
    ? "01 / Your catalogue"
    : "01 / Currently reading";
  $("search-label").hidden = !library;
  $("view-library").hidden = library;
  $("library-panel").hidden = insights;
  $("insights-panel").hidden = !insights;
  const query = $("search").value.toLowerCase();
  const visible = catalogue.filter((book) =>
    library
      ? `${book.title} ${book.author}`.toLowerCase().includes(query)
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
  $("empty-add").hidden = catalogue.length > 0;
  for (const book of visible) {
    const article = element("article", undefined, "book");
    const cover = element("div", undefined, "cover");
    cover.setAttribute("aria-hidden", "true");
    cover.append(
      element("span", book.title),
      element("small", "SHELF / CATALOGUE"),
    );
    const info = element("div");
    info.append(
      element("h3", book.title),
      element("p", book.author || "Author not recorded", "author"),
    );
    const progress = element("progress");
    progress.max = book.total_pages;
    progress.value = book.pages_read;
    progress.setAttribute(
      "aria-label",
      `${book.title}: ${book.pages_read} of ${book.total_pages} pages`,
    );
    const update = element("button", "Record progress", "quiet");
    update.addEventListener("click", () => {
      progressBook = book;
      $("progress-book").textContent =
        `${book.title} · ${book.total_pages} pages`;
      $("progress-pages").max = book.total_pages;
      $("progress-pages").value = book.pages_read;
      $("progress-error").hidden = true;
      $("progress-dialog").showModal();
    });
    info.append(
      progress,
      element(
        "p",
        `${book.pages_read} / ${book.total_pages} pages · ${Math.round((book.pages_read / book.total_pages) * 100)}%`,
        "fine",
      ),
      update,
    );
    article.append(cover, info);
    $("books").append(article);
  }
  $("progress-list").replaceChildren();
  for (const book of catalogue) {
    const row = element("div", undefined, "progress-row");
    const bar = element("progress");
    bar.max = book.total_pages;
    bar.value = book.pages_read;
    bar.setAttribute(
      "aria-label",
      `${book.title}: ${book.pages_read} of ${book.total_pages} pages`,
    );
    row.append(
      element("span", book.title),
      bar,
      element(
        "span",
        `${Math.round((book.pages_read / book.total_pages) * 100)}%`,
        "fine",
      ),
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
for (const id of ["open-add", "empty-add"])
  $(id).addEventListener("click", () => {
    $("add-error").hidden = true;
    $("add-dialog").showModal();
  });
$("close-add").addEventListener("click", () => $("add-dialog").close());
$("add-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const button =
    form.querySelector("[type=submit]") || form.querySelector(".primary");
  button.disabled = true;
  try {
    await api("books", {
      method: "POST",
      body: JSON.stringify({
        title: form.elements.title.value,
        author: form.elements.author.value,
        total_pages: Number(form.elements.total_pages.value),
      }),
    });
    form.reset();
    $("add-dialog").close();
    location.hash = "library";
    await refresh();
    message("Book added to your library.");
  } catch (error) {
    $("add-error").hidden = false;
    $("add-error").textContent = error.message;
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
