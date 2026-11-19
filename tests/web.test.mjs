import { test } from "node:test";
import assert from "node:assert/strict";
import { request } from "../web/api.mjs";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

test("Add book is the single action and opens the EPUB picker", async () => {
  const require = createRequire(
    new URL("../web/package.json", import.meta.url),
  );
  const { JSDOM } = require("jsdom");
  const dom = new JSDOM(
    readFileSync(new URL("../web/index.html", import.meta.url), "utf8"),
    { url: "http://localhost/#library", runScripts: "outside-only" },
  );
  const { window } = dom;
  window.request = async (path) =>
    path === "setup"
      ? { setup_required: false }
      : path === "books"
        ? []
        : { books: 0, reading: 0, finished: 0, pages_read: 0 };
  window.eval(
    readFileSync(new URL("../web/app.js", import.meta.url), "utf8").replace(
      /^import .* from .*;\r?\n/m,
      "",
    ),
  );
  await new Promise((resolve) => setImmediate(resolve));
  const buttons = [...window.document.querySelectorAll("button")].filter(
    (button) => button.textContent.trim() === "Add book",
  );
  assert.equal(buttons.length, 1);
  assert.equal(window.document.getElementById("open-import"), null);
  assert.equal(window.document.getElementById("add-dialog"), null);
  let opened = 0;
  window.document.getElementById("epub-files").click = () => opened++;
  buttons[0].click();
  assert.equal(opened, 1);
  dom.window.close();
});

test("setup accepts an empty 201 response and progress accepts an empty 204", async () => {
  for (const status of [201, 204]) {
    assert.equal(
      await request("setup", {}, async () => new Response(null, { status })),
      null,
    );
  }
});
test("parses successful JSON and preserves authentication errors", async () => {
  assert.deepEqual(
    await request("books", {}, async () => Response.json([{ id: 1 }])),
    [{ id: 1 }],
  );
  await assert.rejects(
    request("books", {}, async () =>
      Response.json({ error: "Please sign in." }, { status: 401 }),
    ),
    { message: "Please sign in.", status: 401 },
  );
});

test("shell nav uses a drawer chrome and reserves scrollbar gutter", () => {
  const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
  const css = readFileSync(new URL("../web/app.css", import.meta.url), "utf8");
  const js = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
  assert.match(html, /id="nav-toggle"/);
  assert.match(html, /id="sidebar-panel"/);
  assert.match(html, /id="nav-backdrop"/);
  assert.match(css, /scrollbar-gutter:\s*stable/);
  assert.match(css, /@media\s*\(max-width:\s*650px\)\s*\{[\s\S]*?\.nav-toggle\s*\{/);
  assert.match(css, /body\.nav-open\s*\.sidebar-panel/);
  assert.doesNotMatch(
    css,
    /@media\s*\(max-width:\s*650px\)\s*\{[\s\S]*?\.sidebar\s+#signout\s*\{[\s\S]*?position:\s*absolute/,
  );
  assert.match(js, /function setNavOpen\(/);
});

test("nav toggle opens and closes the mobile drawer", async () => {
  const require = createRequire(
    new URL("../web/package.json", import.meta.url),
  );
  const { JSDOM } = require("jsdom");
  const dom = new JSDOM(
    readFileSync(new URL("../web/index.html", import.meta.url), "utf8"),
    { url: "http://localhost/#overview", runScripts: "outside-only" },
  );
  const { window } = dom;
  window.request = async (path) =>
    path === "setup"
      ? { setup_required: false }
      : path === "books"
        ? []
        : { books: 0, reading: 0, finished: 0, pages_read: 0 };
  window.eval(
    readFileSync(new URL("../web/app.js", import.meta.url), "utf8").replace(
      /^import .* from .*;\r?\n/m,
      "",
    ),
  );
  await new Promise((resolve) => setImmediate(resolve));
  const toggle = window.document.getElementById("nav-toggle");
  const backdrop = window.document.getElementById("nav-backdrop");
  assert.ok(toggle);
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  toggle.click();
  assert.equal(window.document.body.classList.contains("nav-open"), true);
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  assert.equal(backdrop.hidden, false);
  backdrop.click();
  assert.equal(window.document.body.classList.contains("nav-open"), false);
  assert.equal(backdrop.hidden, true);
  dom.window.close();
});
