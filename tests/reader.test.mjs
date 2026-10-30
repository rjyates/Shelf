import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { sanitizeChapter } from "../web/reader-security.mjs";

const require = createRequire(new URL("../web/package.json", import.meta.url));
const { JSDOM } = require("jsdom");
function sanitize(source) {
  const dom = new JSDOM("", { runScripts: "outside-only" });
  dom.window.eval(
    readFileSync(
      new URL("../web/vendor/purify.min.js", import.meta.url),
      "utf8",
    ),
  );
  const clean = sanitizeChapter(dom.window.DOMPurify, source);
  dom.window.close();
  return new JSDOM(clean).window.document;
}

test("reader removes scripts, events, active embeds, forms, and unsafe links", () => {
  const doc = sanitize(
    '<html><head><base href="https://evil.example"><meta http-equiv="refresh" content="0;url=https://evil.example"></head><body onload="alert(1)"><script>alert(1)</script><iframe src="https://evil.example"></iframe><object data="x"></object><form action="https://evil.example"><input></form><a href="javascript:alert(1)" onclick="alert(1)">Chapter</a><p>Keep this text.</p></body></html>',
  );
  assert.equal(doc.querySelector("script,iframe,object,form,base,meta"), null);
  assert.equal(doc.body.hasAttribute("onload"), false);
  assert.equal(doc.querySelector("a").hasAttribute("href"), false);
  assert.equal(doc.querySelector("a").hasAttribute("onclick"), false);
  assert.match(doc.body.textContent, /Keep this text/);
});

test("reader preserves local EPUB images, SVG covers, stylesheets, and chapter anchors", () => {
  const doc = sanitize(
    '<html><head><link rel="stylesheet" href="blob:https://shelf.example/style"></head><body><img src="blob:https://shelf.example/image"><svg xmlns="http://www.w3.org/2000/svg"><image href="blob:https://shelf.example/cover" /></svg><a href="chapter.xhtml#part">Continue</a></body></html>',
  );
  assert.equal(
    doc.querySelector("img").getAttribute("src"),
    "blob:https://shelf.example/image",
  );
  assert.equal(
    doc.querySelector("image").getAttribute("href"),
    "blob:https://shelf.example/cover",
  );
  assert.equal(
    doc.querySelector("link").getAttribute("href"),
    "blob:https://shelf.example/style",
  );
  assert.equal(
    doc.querySelector("a").getAttribute("href"),
    "chapter.xhtml#part",
  );
});

test("vendored reader bytes match recorded provenance", () => {
  const manifest = JSON.parse(
    readFileSync(new URL("../web/vendor/manifest.json", import.meta.url)),
  );
  for (const item of manifest) {
    const bytes = readFileSync(
      new URL(`../web/vendor/${item.file}`, import.meta.url),
    );
    assert.equal(createHash("sha256").update(bytes).digest("hex"), item.sha256);
  }
});
