import { sanitizeChapter } from "./reader-security.mjs";
("use strict");
const $ = (id) => document.getElementById(id);
const id = Number(new URL(location.href).searchParams.get("id"));
let rendition;
let saving = Promise.resolve();
let opening = true;
async function request(path, options = {}) {
  const response = await fetch(`/api/v1/${path}`, options);
  if (!response.ok)
    throw new Error(
      response.status === 401
        ? "Your session expired. Return to Library to sign in."
        : "Could not load or save this book. Please try again.",
    );
  return response;
}
function navigate(target) {
  $("status").textContent = "Opening chapter…";
  return rendition
    .display(target)
    .then(() => {
      $("status").textContent = "";
    })
    .catch(() => {
      $("status").textContent =
        "This chapter could not be opened. Try another chapter.";
    });
}
async function open() {
  if (!Number.isSafeInteger(id) || id < 1)
    throw new Error("Choose a book from your library.");
  const catalogue = await (await request("books")).json();
  const entry = catalogue.find(
    (book) => book.id === id && book.format === "epub",
  );
  if (!entry) throw new Error("This EPUB is not in your library.");
  $("title").textContent = entry.title;
  document.title = `${entry.title} · Shelf`;
  const bytes = await (await request(`books/${id}/file`)).arrayBuffer();
  const book = ePub(bytes);
  await book.ready;
  // Sanitize each chapter before epub.js writes it into its script-disabled iframe.
  book.spine.hooks.serialize.register((output, section) => {
    section.output = sanitizeChapter(DOMPurify, section.output);
  });
  rendition = book.renderTo("pages", {
    width: "100%",
    height: "100%",
    spread: "none",
    allowScriptedContent: false,
    allowPopups: false,
  });
  rendition.themes.default({
    body: {
      color: "#e4dcc8 !important",
      background: "#171916 !important",
      "line-height": "1.65 !important",
    },
    a: { color: "#b5a16b !important" },
    img: { "max-width": "100% !important", "max-height": "80vh !important" },
  });
  rendition.themes.fontSize("120%");
  rendition.hooks.content.register((contents) => {
    contents.document.addEventListener(
      "click",
      (event) => {
        const link = event.target.closest?.("a");
        if (
          link &&
          /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(link.getAttribute("href") || "")
        ) {
          event.preventDefault();
          event.stopImmediatePropagation();
        }
      },
      true,
    );
  });
  const navigation = await book.loaded.navigation;
  $("chapters").replaceChildren();
  function chapters(items, depth = 0) {
    for (const item of items) {
      const option = document.createElement("option");
      option.value = item.href;
      option.textContent = `${"— ".repeat(Math.min(depth, 4))}${item.label.trim()}`;
      $("chapters").append(option);
      chapters(item.subitems || [], depth + 1);
    }
  }
  chapters(navigation.toc);
  $("chapters").disabled = !$("chapters").options.length;
  $("chapters").addEventListener("change", () => navigate($("chapters").value));
  await book.locations.generate(1600);
  rendition.on("relocated", (location) => {
    const percentage = location.atEnd
      ? 1
      : book.locations.percentageFromCfi(location.start.cfi);
    $("progress").textContent = `${Math.round(percentage * 100)}%`;
    const chapter = [...$("chapters").options].find(
      (option) =>
        option.value.split("#")[0] === location.start.href.split("#")[0],
    );
    if (chapter) $("chapters").value = chapter.value;
    $("previous").disabled = location.atStart;
    $("next").disabled = location.atEnd;
    if (opening) return;
    $("status").textContent = "Saving position…";
    saving = saving
      .catch(() => {})
      .then(() =>
        request(`books/${id}/location`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ position: location.start.cfi, percentage }),
          keepalive: true,
        }),
      )
      .then(() => {
        $("status").textContent = "Position saved";
      })
      .catch(() => {
        $("status").textContent =
          "Position not saved. Check your connection before leaving.";
      });
  });
  try {
    await rendition.display(entry.position || undefined);
  } catch {
    await rendition.display();
    $("status").textContent =
      "The saved location could not be opened. Starting at the beginning.";
  }
  opening = false;
  $("status").textContent = "Ready to read";
  $("size").addEventListener("change", () =>
    rendition.themes.fontSize(`${$("size").value}%`),
  );
  $("previous").addEventListener("click", () =>
    rendition.prev().catch(() => {
      $("status").textContent = "Could not open the previous page.";
    }),
  );
  $("next").addEventListener("click", () =>
    rendition.next().catch(() => {
      $("status").textContent = "Could not open the next page.";
    }),
  );
  rendition.on("keydown", (event) => {
    if (event.key === "ArrowRight") $("next").click();
    if (event.key === "ArrowLeft") $("previous").click();
  });
  window.addEventListener("keydown", (event) => {
    if (event.target.matches("input,select,button")) return;
    if (event.key === "ArrowRight") $("next").click();
    if (event.key === "ArrowLeft") $("previous").click();
  });
}
open().catch((error) => {
  $("status").textContent = error.message || "This EPUB could not be opened.";
  $("title").textContent = "Unable to open book";
});
