/**
 * Guest damage protocol (issue 7). Modes map EBC-class refresh behavior into
 * gray depth, delay, ghosting, and full-refresh flash. Same shape a later
 * panel driver would consume; no Rockchip ioctl ABI on the wire.
 *
 * Message (JSON):
 *   {
 *     v: 1,
 *     mode: "quality"|"fast"|"full",
 *     x, y, w, h,          // damage window (ebc win_x1..win_y2 style)
 *     panel_w, panel_h,    // full panel size; pixels file is panel_w*panel_h gray8
 *     seq,
 *     format: "gray8",
 *     pixels?: "frame.gray"
 *   }
 */

/** @typedef {"quality"|"fast"|"full"} DamageMode */

/** Placeholder delays (200 / 35 / 140). docs/product.md "Panel timing" is the table issue 25 installs. */
export const MODES = {
  // Page turns: slower grayscale, soft residual (GC16 / GL16 class).
  quality: { grayLevels: 16, delayMs: 200, ghost: 0.32, flash: false },
  // Menus: quicker mono / few levels, harsher residual (A2 / DU class).
  fast: { grayLevels: 2, delayMs: 35, ghost: 0.58, flash: false },
  // Full refresh: flash then clear ghosts (FORCE_FULL / RESET class).
  full: { grayLevels: 16, delayMs: 140, ghost: 0, flash: true },
};

export function parseDamage(raw) {
  const msg = typeof raw === "string" ? JSON.parse(raw) : raw;
  if (!msg || msg.v !== 1) {
    throw new Error("damage: unsupported version");
  }
  if (!MODES[msg.mode]) {
    throw new Error(`damage: unknown mode ${msg.mode}`);
  }
  const x = msg.x | 0;
  const y = msg.y | 0;
  const w = msg.w | 0;
  const h = msg.h | 0;
  const panel_w = (msg.panel_w | 0) || w;
  const panel_h = (msg.panel_h | 0) || h;
  if (w <= 0 || h <= 0 || panel_w <= 0 || panel_h <= 0) {
    throw new Error("damage: empty rect");
  }
  return {
    v: 1,
    mode: /** @type {DamageMode} */ (msg.mode),
    x,
    y,
    w,
    h,
    panel_w,
    panel_h,
    seq: msg.seq | 0,
    format: msg.format || "gray8",
    pixels: msg.pixels || "frame.gray",
  };
}

/** Point a Panel at the knobs for this mode (mutates panel). */
export function configurePanel(panel, mode) {
  const m = MODES[mode];
  if (!m) throw new Error(`damage: unknown mode ${mode}`);
  panel.grayLevels = m.grayLevels;
  panel.delayMs = m.delayMs;
  panel.ghost = m.ghost;
  return m;
}

/**
 * Apply one damage message. `pixels` is gray8 for the full panel
 * (panel_w * panel_h) or a Float32 0–1 buffer of that length.
 * quality/fast update only the damage rect; full refreshes the panel.
 */
export async function applyDamage(panel, msg, pixels) {
  const parsed = parseDamage(msg);
  const preset = configurePanel(panel, parsed.mode);
  const rect = { x: parsed.x, y: parsed.y, w: parsed.w, h: parsed.h };

  if (parsed.mode === "full") {
    await panel.full(pixels, { flash: preset.flash });
    return parsed;
  }
  await panel.partial(pixels, rect);
  return parsed;
}
