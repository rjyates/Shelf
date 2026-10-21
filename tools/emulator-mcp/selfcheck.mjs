/**
 * Offline checks + docker-gate checks.
 * Run: node tools/emulator-mcp/selfcheck.mjs
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { MODES, parseDamage } from "../../emulator/viewer/damage.js";
import { decodeFbcon, stripSerialLog } from "./lib/console.mjs";
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

const font = readFileSync(new URL("./lib/font8x16.bin", import.meta.url));
const sample = "oseink-damage:ok";
const sampleW = (sample.length + 1) * 8;
const sampleRgb = Buffer.alloc(sampleW * 16 * 3);
for (let i = 0; i < sample.length; i++) {
  const glyph = font.subarray(sample.charCodeAt(i) * 16, sample.charCodeAt(i) * 16 + 16);
  for (let row = 0; row < 16; row++) {
    const bits = glyph[row];
    for (let col = 0; col < 8; col++) {
      if (bits & (0x80 >> col)) {
        const o = (row * sampleW + i * 8 + col) * 3;
        sampleRgb[o] = sampleRgb[o + 1] = sampleRgb[o + 2] = 255;
      }
    }
  }
}
// Underline cursor in the last cell: bottom two rows only.
for (const row of [14, 15]) {
  for (let col = 0; col < 8; col++) {
    const o = (row * sampleW + sample.length * 8 + col) * 3;
    sampleRgb[o] = sampleRgb[o + 1] = sampleRgb[o + 2] = 255;
  }
}
assert(decodeFbcon(sampleRgb, sampleW, 16) === sample, "fbcon decode");
const serial = stripSerialLog(
  "qemu-1  | \u001b[1;31m❯ Warning: COW\u001b[0m\nqemu-1  | SeaBIOS (version 1)\nqemu-1  | Welcome to oseink\n",
);
assert(serial === "SeaBIOS (version 1)\nWelcome to oseink", `serial strip: ${serial}`);

const serverSrc = readFileSync(join(ROOT, "tools/emulator-mcp/server.mjs"), "utf8");
for (const tool of [
  "panel_screenshot",
  "display_screenshot",
  "console_read",
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

console.log("ok: offline parse/png");
console.log("ok: tools + host configs");

const id = await containerId();
if (!id) {
  try {
    await requireContainer();
    assert(false, "requireContainer should throw when down");
  } catch (err) {
    assert(/not running/i.test(String(err.message)), `gate message: ${err.message}`);
  }
  console.log("ok: docker gate rejects when container is down");
  console.log("skip: live panel_screenshot (start emulator compose to exercise)");
  process.exit(0);
}

const { viewerScreenshotPng } = await import("./lib/viewer.mjs");
const shot = await viewerScreenshotPng();
assert(shot.png[0] === 0x89, "viewer png magic");
assert(shot.png.length > 500, `viewer png too small (${shot.png.length})`);
console.log(`ok: live panel_screenshot via playwright (${shot.png.length} bytes)`);

const { captureVga, readSerial } = await import("./lib/console.mjs");
const vga = await captureVga();
assert(vga.png[0] === 0x89, "vga png magic");
assert(vga.width >= 640 && vga.height >= 400, `vga size ${vga.width}x${vga.height}`);
assert(vga.png.length > 1000, `vga png too small (${vga.png.length})`);
assert(typeof vga.text === "string", "vga text");
console.log(`ok: live display_screenshot ${vga.width}x${vga.height}`);
console.log(`ok: tty0 ${JSON.stringify(vga.text.split("\n").slice(0, 4).join(" | "))}`);
const ttyS0 = await readSerial(80);
assert(typeof ttyS0 === "string", "ttyS0 text");
console.log(`ok: ttyS0 ${JSON.stringify(ttyS0.split("\n").slice(0, 4).join(" | "))}`);
console.log("ok: docker-backed MCP path");
