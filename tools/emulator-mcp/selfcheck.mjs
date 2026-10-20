/**
 * Offline checks + docker-gate checks.
 * Run: node tools/emulator-mcp/selfcheck.mjs
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { MODES, parseDamage } from "../../emulator/viewer/damage.js";
import { containerId, requireContainer } from "./lib/docker.mjs";
import { ROOT } from "./lib/paths.mjs";
import { rgbToPng } from "./lib/png.mjs";

function assert(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
}

const msg = parseDamage({
  v: 1,
  mode: "quality",
  x: 0,
  y: 0,
  w: 8,
  h: 8,
  panel_w: 400,
  panel_h: 300,
  seq: 1,
});
assert(msg.panel_w === 400, "parseDamage panel size");
assert(MODES.quality.delayMs > MODES.fast.delayMs, "mode timing");

const rgb = Buffer.alloc(4 * 3, 200);
const png = rgbToPng(2, 2, rgb);
assert(png[0] === 0x89 && png[1] === 0x50, "PNG magic");

const serverSrc = readFileSync(join(ROOT, "tools/emulator-mcp/server.mjs"), "utf8");
for (const tool of [
  "panel_screenshot",
  "guest_type",
  "emulator_up",
  "overlay_touch",
  "damage_render",
]) {
  assert(serverSrc.includes(`"${tool}"`), `tool registered: ${tool}`);
}

for (const cfg of [".cursor/mcp.json", ".mcp.json", ".vscode/mcp.json"]) {
  const text = readFileSync(join(ROOT, cfg), "utf8");
  assert(text.includes("oseink-emulator"), `${cfg} names server`);
  assert(text.includes("emulator-mcp/run.sh"), `${cfg} uses run.sh`);
}

const id = await containerId();
if (!id) {
  try {
    await requireContainer();
    assert(false, "requireContainer should throw when down");
  } catch (err) {
    assert(/not running/i.test(String(err.message)), `gate message: ${err.message}`);
  }
  console.log("ok: offline parse/png");
  console.log("ok: tools + host configs");
  console.log("ok: docker gate rejects when container is down");
  console.log("skip: live panel_screenshot (start emulator compose to exercise)");
  process.exit(0);
}

const { viewerScreenshotPng } = await import("./lib/viewer.mjs");
const shot = await viewerScreenshotPng();
assert(shot.png[0] === 0x89, "viewer png magic");
assert(shot.png.length > 500, `viewer png too small (${shot.png.length})`);
console.log(`ok: live panel_screenshot via playwright (${shot.png.length} bytes)`);
console.log("ok: docker-backed MCP path");
