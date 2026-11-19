import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const root = new URL("../", import.meta.url);
const html = readFileSync(new URL("web/index.html", root), "utf8");
const css = readFileSync(new URL("web/app.css", root), "utf8");
const js = readFileSync(new URL("web/app.js", root), "utf8");

test("Insights markup gates custom range and uses a GitHub-style heatmap shell", () => {
  assert.match(html, /data-range-preset="custom"/);
  assert.match(
    html,
    /<div class="ins-range-custom" id="ins-range-custom" hidden>/,
  );
  assert.doesNotMatch(html, /id="ins-hours-spark"/);
  assert.match(
    html,
    /calendar-days"[^>]*>\s*<span>Mon<\/span><span><\/span><span>Wed<\/span><span><\/span><span>Fri<\/span>/,
  );
  assert.match(html, /id="ins-month-labels" class="calendar-months"/);
  assert.doesNotMatch(html, /fonts\.googleapis\.com|family=Prata|family=Manrope/);
});

test("Insights styles stay on Shelf v3 tokens with weekdot labels and celestial fill", () => {
  assert.match(css, /--ins-paper:\s*var\(--ink\)/);
  assert.match(css, /--ins-display:\s*var\(--display\)/);
  assert.match(css, /--ins-body:\s*var\(--body\)/);
  assert.match(css, /\.ins-weekdot-day\s*\{/);
  assert.match(css, /\.calendar-months\s*\{/);
  assert.match(css, /body\[data-view="insights"\]\s*\.masthead\s*\{[\s\S]*?overflow:\s*visible/);
  assert.match(
    css,
    /body\[data-view="insights"\]\s*\.masthead\s*\.celestial\s*\{[\s\S]*?object-fit:\s*cover[\s\S]*?opacity:\s*0\.94/,
  );
  assert.match(
    css,
    /body\[data-view="insights"\]\s*\.masthead\s*\.celestial\s*\{[\s\S]*?height:\s*clamp\(380px/,
  );
  assert.match(
    css,
    /body\[data-view="insights"\]\s*#content[\s\S]*?z-index:\s*2/,
  );
  assert.doesNotMatch(css, /--ins-display:\s*"Prata"/);
});

test("Insights script filters finished books and builds labeled weekdots + month labels", () => {
  assert.match(
    js,
    /shelfBooks\(\)\.filter\(\(\s*book\s*\)\s*=>\s*shelfState\(book\)\s*===\s*["']reading["']\)/,
  );
  assert.match(js, /function renderMonthLabels\(/);
  assert.match(js, /ins-weekdot-day/);
  assert.match(js, /data-range-preset|preset === "custom"/);
  assert.doesNotMatch(js, /function renderHoursSpark\(/);
  assert.doesNotMatch(js, /ins-hours-spark/);
});

async function loadInsightsDom({
  books = [],
  reading = null,
  widgets = { widgets: [] },
} = {}) {
  const require = createRequire(new URL("web/package.json", root));
  const { JSDOM } = require("jsdom");
  const from = "2026-05-18";
  const to = "2026-11-19";
  const readingPayload =
    reading ||
    ({
      from,
      to,
      heatmap: [
        { day: "2026-11-18", readingSeconds: 600, sessionCount: 1 },
        { day: "2026-11-19", readingSeconds: 120, sessionCount: 1 },
      ],
      sourceDistribution: { slices: [], totalSeconds: 720 },
      sessionTimeline: [],
      streak: {
        currentStreak: 1,
        longestStreak: 2,
        lastSevenDays: [false, false, false, false, false, true, true],
      },
    });

  const dom = new JSDOM(html, {
    url: "http://localhost/#insights",
    runScripts: "outside-only",
    pretendToBeVisual: true,
  });
  const { window } = dom;

  window.request = async (path) => {
    if (path === "setup") return { setup_required: false };
    if (String(path).startsWith("books?")) return books;
    if (path === "works" || path === "authors" || path === "collections" || path === "scopes") {
      return [];
    }
    if (String(path).startsWith("insights/reading")) return readingPayload;
    if (String(path).startsWith("insights/widgets")) return widgets;
    if (path === "insights/achievements") {
      return { categories: [], totalEarned: 0, totalAvailable: 10 };
    }
    if (path === "insights/library") {
      return {
        formatDistribution: { total: books.length, slices: [] },
        metadataCompleteness: { total: books.length },
        languageDistribution: { slices: [] },
        storageByFormat: { slices: [] },
      };
    }
    return {};
  };

  window.eval(js.replace(/^import .* from .*;\r?\n/m, ""));
  // Boot calls start() → refresh(); give microtasks a turn to settle.
  for (let i = 0; i < 20; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
    if (window.document.body.dataset.view === "insights") break;
  }
  // Force insights render after catalogue is in place.
  window.location.hash = "insights";
  window.dispatchEvent(new window.Event("hashchange"));
  for (let i = 0; i < 30; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
    const cells = window.document.querySelectorAll("#reading-calendar button");
    if (cells.length > 0) break;
  }
  return dom;
}

test("Insights UI: custom range stays hidden until Custom chip", async () => {
  const dom = await loadInsightsDom();
  const { document } = dom.window;
  const custom = document.getElementById("ins-range-custom");
  assert.equal(custom.hidden, true);
  document.querySelector('[data-range-preset="custom"]').click();
  assert.equal(custom.hidden, false);
  document.querySelector('[data-range-preset="30d"]').click();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(custom.hidden, true);
  dom.window.close();
});

test("Insights UI: progress list omits finished 100% books", async () => {
  const dom = await loadInsightsDom({
    books: [
      {
        id: 1,
        title: "The Fellowship of the Ring",
        author: "Tolkien",
        format: "epub",
        percentage: 0.997,
        position: "epubcfi(/6/2)",
        reading_status: "finished",
        pages_read: 0,
        total_pages: 0,
      },
      {
        id: 2,
        title: "The Return of the King",
        author: "Tolkien",
        format: "epub",
        percentage: 0.04,
        position: "epubcfi(/6/4)",
        reading_status: "reading",
        pages_read: 0,
        total_pages: 0,
      },
    ],
  });
  const list = dom.window.document.getElementById("progress-list");
  for (let i = 0; i < 20 && !(list?.textContent || "").trim(); i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
  const text = list?.textContent || "";
  assert.match(text, /Return of the King/);
  assert.doesNotMatch(text, /Fellowship/);
  assert.doesNotMatch(text, /100%/);
  dom.window.close();
});

test("Insights UI: streak weekdots carry weekday letters", async () => {
  const today = new Date();
  const dayLetters = ["S", "M", "T", "W", "T", "F", "S"];
  const expected = [];
  for (let i = 6; i >= 0; i -= 1) {
    const d = new Date(
      Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate() - i),
    );
    expected.push(dayLetters[d.getUTCDay()]);
  }
  const dom = await loadInsightsDom({
    widgets: {
      widgets: [
        {
          type: "reading-streak",
          data: {
            currentStreak: 2,
            longestStreak: 4,
            lastSevenDays: [false, false, false, false, false, true, true],
          },
        },
      ],
    },
  });
  const letters = [
    ...dom.window.document.querySelectorAll(".ins-weekdot-day"),
  ].map((el) => el.textContent);
  assert.deepEqual(letters, expected);
  dom.window.close();
});

test("Insights UI: heatmap pads weeks and emits month labels", async () => {
  const dom = await loadInsightsDom();
  const { document } = dom.window;
  const months = [...document.querySelectorAll("#ins-month-labels span")]
    .map((el) => el.textContent)
    .filter(Boolean);
  assert.ok(months.length >= 2, `expected month labels, got ${months.join(",")}`);
  const cells = document.querySelectorAll("#reading-calendar button");
  assert.equal(cells.length % 7, 0);
  assert.ok(cells.length >= 7);
  assert.equal(document.getElementById("ins-hours-spark"), null);
  dom.window.close();
});
