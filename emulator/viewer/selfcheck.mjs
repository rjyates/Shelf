/**
 * Asserts issue 6 acceptance: partial updates leave residual ghosting,
 * and a full refresh clears it. Run: node emulator/viewer/selfcheck.mjs
 */
import { Panel, checkerboard, solid } from "./panel.js";

function assert(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
}

const panel = new Panel({
  width: 64,
  height: 48,
  grayLevels: 16,
  ghost: 0.4,
  delayMs: 0,
  dither: false,
});

const a = solid(panel, 1);
const b = checkerboard(panel, 4);
const c = solid(panel, 0);

panel.applyFull(a);
assert(panel.errorAgainst(a) < 1e-6, "full lands on target");

panel.applyPartial(b);
const afterFirst = panel.errorAgainst(b);
assert(afterFirst > 0.05, `partial leaves ghost vs target (err=${afterFirst})`);

panel.applyPartial(c);
const afterSecond = panel.errorAgainst(c);
assert(afterSecond > 0.05, `stacked partials still degrade (err=${afterSecond})`);
assert(afterSecond > 0.01, "degradation is visible in mean absolute error");

panel.applyFull(c);
const cleared = panel.errorAgainst(c);
assert(cleared < 1e-6, `full refresh clears ghosting (err=${cleared})`);
assert(panel.flashPhase === "done", "full refresh records a flash");

// Quantization is applied (values snap to levels).
const mid = solid(panel, 0.53);
panel.applyFull(mid);
const q = panel.ink[0];
const expected = Math.round(0.53 * 15) / 15;
assert(Math.abs(q - expected) < 1e-6, "grayscale quantization");

console.log("ok: partial degrades, full clears");
