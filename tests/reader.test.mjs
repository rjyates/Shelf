import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";

test("orbit-reader build embeds Book Orbit attribution", () => {
  const attribution = readFileSync(
    new URL("../web/orbit-reader/vendor/bookorbit/ATTRIBUTION.md", import.meta.url),
    "utf8",
  );
  assert.match(attribution, /Book Orbit/);
  assert.match(attribution, /AGPL-3\.0/);
  assert.match(attribution, /https:\/\/github\.com\/bookorbit\/bookorbit/);
});

test("orbit-reader dist includes the Foliate host and shell", () => {
  const dist = new URL("../web/orbit-reader/dist/", import.meta.url);
  assert.equal(existsSync(new URL("index.html", dist)), true);
  assert.equal(existsSync(new URL("orbit-reader.js", dist)), true);
  assert.equal(existsSync(new URL("assets/foliate/view.js", dist)), true);
  const html = readFileSync(new URL("index.html", dist), "utf8");
  assert.match(html, /Book Orbit/);
  assert.match(html, /orbit-reader\.js/);
});

test("orbit progress adapter reads Shelf's flattened EPUB catalogue fields", () => {
  const src = readFileSync(
    new URL("../web/orbit-reader/vendor/bookorbit/lib/api.ts", import.meta.url),
    "utf8",
  );
  // Shelf uses #[serde(flatten)] on epub — percentage/position are top-level, not book.epub.*.
  assert.match(src, /export function shelfEpubFields/);
  assert.match(src, /book\.format === 'epub'/);
  assert.match(src, /book\.percentage/);
  assert.match(src, /book\.position/);
  assert.doesNotMatch(
    src,
    /if \(!book\?\.epub\) return jsonResponse\(\{ items: \[\] \}\)/,
  );
});

test("reader chrome pin preference persists in localStorage", () => {
  const visibility = readFileSync(
    new URL(
      "../web/orbit-reader/vendor/bookorbit/features/reader/shared/composables/useVisibility.ts",
      import.meta.url,
    ),
    "utf8",
  );
  assert.match(visibility, /READER_CHROME_PIN_KEY = 'reader:chrome:pinned'/);
  assert.match(visibility, /writeStoredChromePinned/);
  assert.match(visibility, /readStoredChromePinned/);
  const sidebar = readFileSync(
    new URL(
      "../web/orbit-reader/vendor/bookorbit/features/reader/epub/composables/useReaderSidebarPin.ts",
      import.meta.url,
    ),
    "utf8",
  );
  assert.match(sidebar, /localStorage/);
  assert.doesNotMatch(sidebar, /sessionStorage/);
});

test("legacy epub.js vendor bytes still match recorded provenance when present", () => {
  const manifestUrl = new URL("../web/vendor/manifest.json", import.meta.url);
  if (!existsSync(manifestUrl)) return;
  const manifest = JSON.parse(readFileSync(manifestUrl, "utf8"));
  for (const item of manifest) {
    const bytes = readFileSync(
      new URL(`../web/vendor/${item.file}`, import.meta.url),
    );
    assert.equal(createHash("sha256").update(bytes).digest("hex"), item.sha256);
  }
});
