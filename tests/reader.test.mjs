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
