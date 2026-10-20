/**
 * Issue 6: partial leaves ghosting; full clears it.
 * Issue 7: quality / fast / full mode presets drive distinct simulator behavior.
 * Run: node emulator/viewer/selfcheck.mjs
 */
import { Panel, checkerboard, solid } from "./panel.js";
import { MODES, applyDamage, parseDamage } from "./damage.js";

function assert(cond, msg) {
  if (!cond) {
    console.error(`FAIL: ${msg}`);
    process.exit(1);
  }
}

// --- issue 6 ---
{
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

  panel.applyFull(c);
  const cleared = panel.errorAgainst(c);
  assert(cleared < 1e-6, `full refresh clears ghosting (err=${cleared})`);
  assert(panel.flashPhase === "done", "full refresh records a flash");

  const mid = solid(panel, 0.53);
  panel.applyFull(mid);
  const q = panel.ink[0];
  const expected = Math.round(0.53 * 15) / 15;
  assert(Math.abs(q - expected) < 1e-6, "grayscale quantization");
}

// --- issue 7 mode mapping ---
assert(MODES.quality.delayMs > MODES.fast.delayMs, "quality slower than fast");
assert(MODES.quality.grayLevels > MODES.fast.grayLevels, "quality has more gray levels");
assert(MODES.fast.ghost > MODES.quality.ghost, "fast leaves harsher residual");
assert(MODES.full.flash === true, "full flashes");

{
  const panel = new Panel({
    width: 32,
    height: 24,
    delayMs: 0,
    dither: false,
  });
  const page = solid(panel, 0.7);
  const menu = solid(panel, 0.1);

  panel.applyFull(solid(panel, 1), { flash: false });

  await applyDamage(
    panel,
    {
      v: 1,
      mode: "quality",
      x: 0,
      y: 0,
      w: 32,
      h: 24,
      panel_w: 32,
      panel_h: 24,
      seq: 1,
    },
    page,
  );
  assert(panel.grayLevels === 16, "quality configures 16 levels");
  assert(panel.delayMs === MODES.quality.delayMs, "quality delay");
  const qualityErr = panel.errorAgainst(page);
  assert(qualityErr > 0.01, `quality partial leaves soft ghost (err=${qualityErr})`);
  assert(qualityErr < 0.5, `quality ghost stays soft (err=${qualityErr})`);

  // Fresh panel so fast residual is measured on a full-rect update.
  const fastPanel = new Panel({
    width: 32,
    height: 24,
    delayMs: 0,
    dither: false,
  });
  fastPanel.applyFull(solid(fastPanel, 1), { flash: false });
  await applyDamage(
    fastPanel,
    {
      v: 1,
      mode: "fast",
      x: 0,
      y: 0,
      w: 32,
      h: 24,
      panel_w: 32,
      panel_h: 24,
      seq: 2,
    },
    menu,
  );
  assert(fastPanel.grayLevels === 2, "fast configures mono");
  assert(fastPanel.delayMs === MODES.fast.delayMs, "fast delay");
  const fastErr = fastPanel.errorAgainst(menu);
  assert(fastErr > qualityErr, `fast residual harsher than quality (${fastErr} > ${qualityErr})`);

  await applyDamage(
    panel,
    {
      v: 1,
      mode: "full",
      x: 0,
      y: 0,
      w: 32,
      h: 24,
      panel_w: 32,
      panel_h: 24,
      seq: 3,
    },
    menu,
  );
  assert(panel.flashPhase === "done", "full mode flashes");
  assert(panel.errorAgainst(menu) < 1e-6, "full clears ghosting");
}

{
  const msg = parseDamage(
    '{"v":1,"mode":"quality","x":0,"y":0,"w":8,"h":8,"panel_w":400,"panel_h":300,"seq":9}',
  );
  assert(msg.panel_w === 400 && msg.w === 8, "rect vs panel size");
}

console.log("ok: partial degrades, full clears");
console.log("ok: quality slow gray, fast mono, full flashes");
