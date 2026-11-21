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

test("Manage splits Merge fold from Combine readalong and shows filename", () => {
  const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
  const css = readFileSync(new URL("../web/app.css", import.meta.url), "utf8");
  const js = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
  assert.match(html, /id="manage-filename"/);
  assert.match(html, /id="manage-merge"/);
  assert.match(html, /id="manage-combine"/);
  assert.match(html, /Merge into one card/);
  assert.match(html, /Combine to readalong/);
  assert.doesNotMatch(html, /Merge &amp; Combine/);
  assert.match(css, /\.manage-filename/);
  assert.match(js, /function mergeCandidates\(/);
  assert.match(js, /function combineCandidates\(/);
  assert.match(js, /books\/merge/);
  assert.match(js, /book\.merged_into/);
  assert.match(js, /book\.original_filename/);
  assert.match(js, /combineEditions\(/);
});

test("shell nav uses a drawer chrome and reserves scrollbar gutter", () => {
  const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
  const css = readFileSync(new URL("../web/app.css", import.meta.url), "utf8");
  const js = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
  assert.match(html, /id="nav-toggle"/);
  assert.match(html, /id="nav-collapse"/);
  assert.match(html, /id="sidebar-panel"/);
  assert.match(html, /id="nav-backdrop"/);
  assert.match(html, /class="auth-brand"/);
  assert.match(html, /id="settings-account"/);
  assert.match(html, /id="settings-account"[\s\S]*?id="signout"/);
  const foot = html.match(/<div class="sidebar-foot">[\s\S]*?<\/div>/);
  assert.ok(foot, "sidebar-foot present");
  assert.doesNotMatch(foot[0], /id="signout"/);
  assert.match(html, /class="sidebar-foot">[\s\S]*?<\/div>\s*<p class="edition">/);
  assert.match(css, /scrollbar-gutter:\s*stable/);
  assert.match(css, /--sidebar-width-icon/);
  assert.match(css, /body\.nav-collapsed/);
  assert.match(css, /\.nav-collapse-mark\s*\{[\s\S]*?rotate\(135deg\)/);
  assert.match(css, /body\.nav-collapsed \.nav-collapse-mark\s*\{[\s\S]*?rotate\(-45deg\)/);
  assert.match(css, /\.nav-label\s*\{[\s\S]*?color:\s*var\(--text\)/);
  assert.match(css, /\.nav-icon\s*\{[\s\S]*?color:\s*var\(--brass\)/);
  assert.match(css, /@media\s*\(max-width:\s*650px\)\s*\{[\s\S]*?\.nav-toggle\s*\{/);
  assert.match(css, /body\.nav-open\s*\.sidebar-panel/);
  assert.match(css, /body\.auth-open \.sidebar/);
  assert.match(css, /body\.auth-open \.masthead/);
  assert.doesNotMatch(
    css,
    /@media\s*\(max-width:\s*650px\)\s*\{[\s\S]*?\.sidebar\s+#signout\s*\{[\s\S]*?position:\s*absolute/,
  );
  assert.match(js, /function setNavOpen\(/);
  assert.match(js, /function setNavCollapsed\(/);
  assert.match(js, /shelf\.nav\.collapsed/);
  assert.match(js, /classList\.add\("auth-open"\)/);
  assert.match(js, /classList\.remove\("auth-open"\)/);
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
  const collapse = window.document.getElementById("nav-collapse");
  assert.ok(collapse);
  assert.equal(collapse.getAttribute("aria-expanded"), "true");
  collapse.click();
  assert.equal(window.document.body.classList.contains("nav-collapsed"), true);
  assert.equal(window.document.getElementById("sidebar").dataset.state, "collapsed");
  assert.equal(collapse.getAttribute("aria-expanded"), "false");
  collapse.click();
  assert.equal(window.document.body.classList.contains("nav-collapsed"), false);
  assert.equal(window.document.getElementById("sidebar").dataset.state, "expanded");
  dom.window.close();
});

test("sidebar and overview quotes cycle without colliding; heading follows daypart", async () => {
  const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
  const js = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
  assert.match(html, /id="side-quote"/);
  assert.match(js, /const BOOK_QUOTES = \[/);
  assert.match(js, /function daypartHeading\(/);
  assert.match(js, /A morning well read\./);
  assert.match(js, /An afternoon well read\./);
  assert.match(js, /An evening well read\./);
  assert.match(js, /A night well read\./);
  assert.match(js, /hour >= 5 && hour < 12/);
  assert.match(js, /hour >= 12 && hour < 17/);
  assert.match(js, /hour >= 17 && hour < 21/);

  const require = createRequire(
    new URL("../web/package.json", import.meta.url),
  );
  const { JSDOM } = require("jsdom");
  const RealDate = Date;
  // Local 08:30 so daypartHeading is morning regardless of host TZ.
  const fixed = new RealDate(2026, 10, 21, 8, 30, 0);
  class MockDate extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [fixed.getTime()]));
    }
    static now() {
      return fixed.getTime();
    }
  }
  const dom = new JSDOM(
    readFileSync(new URL("../web/index.html", import.meta.url), "utf8"),
    { url: "http://localhost/#overview", runScripts: "outside-only" },
  );
  const { window } = dom;
  window.Date = MockDate;
  window.request = async (path) =>
    path === "setup"
      ? { setup_required: false }
      : path === "books"
        ? []
        : path === "works"
          ? []
          : { books: 0, reading: 0, finished: 0, pages_read: 0 };
  window.eval(
    readFileSync(new URL("../web/app.js", import.meta.url), "utf8").replace(
      /^import .* from .*;\r?\n/m,
      "",
    ),
  );
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  const side = window.document.getElementById("side-quote").textContent;
  const subtitle = window.document.getElementById("subtitle").textContent;
  const heading = window.document.getElementById("heading").textContent;
  const bare = (s) => s.replace(/[\u201c\u201d"]/g, "").trim();
  assert.ok(side.length > 4, "side quote applied");
  assert.equal(heading, "A morning well read.");
  assert.notEqual(bare(side), bare(subtitle));
  assert.match(side, /^[\u201c"].*[\u201d"]$/);
  dom.window.close();
});

test("Overview Currently reading excludes finished; Manage ships finish date", () => {
  const html = readFileSync(new URL("../web/index.html", import.meta.url), "utf8");
  const css = readFileSync(new URL("../web/app.css", import.meta.url), "utf8");
  const js = readFileSync(new URL("../web/app.js", import.meta.url), "utf8");
  assert.match(html, /id="manage-finished-at"/);
  assert.match(html, /id="manage-finish-block"/);
  assert.match(css, /\.manage-finish-date/);
  assert.match(js, /function progressPercent\(/);
  assert.match(js, /function localDayIso\(/);
  assert.match(
    js,
    /if\s*\(!library\)\s*\{[\s\S]*?shelfState\(book\)\s*===\s*["']reading["']/
  );
  assert.match(js, /finished_at:\s*day/);
  assert.doesNotMatch(js, /Math\.round\((?:fraction\(book\)|book\.percentage)\s*\*\s*100\)/);
  assert.equal(evalProgressPercent(js, 0.9978021978021978), 99);
  assert.equal(evalProgressPercent(js, 1), 100);
  assert.equal(evalProgressPercent(js, 0), 0);
});

function evalProgressPercent(js, frac) {
  const match = js.match(/function progressPercent\(frac\)\s*\{([\s\S]*?)\n\}/);
  assert.ok(match, "progressPercent body");
  // eslint-disable-next-line no-new-func
  return Function(`"use strict"; ${match[0]}; return progressPercent(${frac});`)();
}
