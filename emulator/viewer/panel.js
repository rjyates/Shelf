/**
 * E-ink panel simulator. Quantizes to gray levels, delays refreshes,
 * leaves ghosting on partial updates, and clears with a full-refresh flash.
 * Guest damage modes (damage.js) call applyPartial / applyFull here.
 */

const DEFAULT_GHOST = 0.42;
const DEFAULT_DELAY_MS = 80;
const DEFAULT_GRAY_LEVELS = 4;

/** Warm e-paper front; ink sits on it. Pure #000/#fff reads as LCD without this. */
const PAPER = [214, 208, 194];
const INK = [34, 32, 28];

// Bayer 4×4 thresholds in [0, 1).
const BAYER4 = [
  [0 / 16, 8 / 16, 2 / 16, 10 / 16],
  [12 / 16, 4 / 16, 14 / 16, 6 / 16],
  [3 / 16, 11 / 16, 1 / 16, 9 / 16],
  [15 / 16, 7 / 16, 13 / 16, 5 / 16],
];

function clamp01(v) {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

function toGray01(src, length) {
  const out = new Float32Array(length);
  if (src instanceof Float32Array) {
    for (let i = 0; i < length; i++) out[i] = clamp01(src[i]);
    return out;
  }
  if (src.length >= length * 4) {
    for (let i = 0; i < length; i++) {
      const o = i * 4;
      out[i] = clamp01((0.299 * src[o] + 0.587 * src[o + 1] + 0.114 * src[o + 2]) / 255);
    }
    return out;
  }
  for (let i = 0; i < length; i++) out[i] = clamp01(src[i] / 255);
  return out;
}

function quantize(v, levels) {
  if (levels <= 2) return v < 0.5 ? 0 : 1;
  const step = levels - 1;
  return Math.round(v * step) / step;
}

/** Ordered dither into `levels` so flat regions still look like e-paper. */
function quantizeDither(v, levels, x, y) {
  const threshold = BAYER4[y & 3][x & 3];
  if (levels <= 2) return v < threshold ? 0 : 1;
  const step = levels - 1;
  const scaled = v * step;
  const base = Math.floor(scaled);
  const frac = scaled - base;
  const pick = frac > threshold ? base + 1 : base;
  return Math.min(step, Math.max(0, pick)) / step;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class Panel {
  constructor({
    width = 400,
    height = 300,
    grayLevels = DEFAULT_GRAY_LEVELS,
    ghost = DEFAULT_GHOST,
    delayMs = DEFAULT_DELAY_MS,
    dither = true,
    // shortcut: VGA console is light-on-dark; real shell will draw dark-on-paper.
    invert = false,
  } = {}) {
    this.width = width;
    this.height = height;
    this.grayLevels = grayLevels;
    this.ghost = ghost;
    this.delayMs = delayMs;
    this.dither = dither;
    this.invert = invert;
    this.length = width * height;
    /** Ink state in [0, 1]. What the panel is showing. */
    this.ink = new Float32Array(this.length);
    this.ink.fill(1);
    /** Last flash phase for demos/tests: null | "black" | "white" | "done". */
    this.flashPhase = null;
  }

  /** Display buffer as 0–255 grayscale bytes (logical ink, not paper tint). */
  pixels() {
    const out = new Uint8Array(this.length);
    for (let i = 0; i < this.length; i++) out[i] = Math.round(this.ink[i] * 255);
    return out;
  }

  /** Mean absolute error vs a target gray buffer (Float32 0–1 or bytes). */
  errorAgainst(target) {
    const want = this._quantizeTarget(target);
    let sum = 0;
    for (let i = 0; i < this.length; i++) {
      sum += Math.abs(this.ink[i] - want[i]);
    }
    return sum / this.length;
  }

  _quantizeTarget(target) {
    const gray = toGray01(target, this.length);
    if (this.invert) {
      for (let i = 0; i < this.length; i++) gray[i] = 1 - gray[i];
    }
    const levels = this.grayLevels;
    if (!this.dither) {
      for (let i = 0; i < this.length; i++) {
        gray[i] = quantize(gray[i], levels);
      }
      return gray;
    }
    const w = this.width;
    for (let i = 0; i < this.length; i++) {
      gray[i] = quantizeDither(gray[i], levels, i % w, (i / w) | 0);
    }
    return gray;
  }

  /**
   * Partial update: blend toward the quantized target, keeping ghost residual.
   * Optional rect {x,y,w,h} limits the damage region.
   */
  applyPartial(target, rect) {
    const want = this._quantizeTarget(target);
    const g = this.ghost;
    const { x0, y0, x1, y1 } = this._bounds(rect);
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const i = y * this.width + x;
        this.ink[i] = (1 - g) * want[i] + g * this.ink[i];
      }
    }
    this.flashPhase = null;
  }

  /**
   * Full refresh: optional black → white flash, then snap ink to the quantized
   * target. Clears accumulated ghosting.
   */
  applyFull(target, { flash = true } = {}) {
    const want = this._quantizeTarget(target);
    if (flash) {
      this.flashPhase = "black";
      this.ink.fill(0);
      this.flashPhase = "white";
      this.ink.fill(1);
    }
    this.ink.set(want);
    this.flashPhase = "done";
  }

  async partial(target, rect) {
    if (this.delayMs > 0) await sleep(this.delayMs);
    this.applyPartial(target, rect);
  }

  async full(target, opts) {
    const flash = !opts || opts.flash !== false;
    // Flash steps share the busy time. The ink is settled when this returns.
    if (flash && this.paint) {
      const total = this.delayMs > 0 ? this.delayMs : 180;
      const blackMs = Math.floor(total / 2);
      const whiteMs = total - blackMs;
      this.ink.fill(0);
      this.flashPhase = "black";
      this.paint();
      if (blackMs > 0) await sleep(blackMs);
      this.ink.fill(1);
      this.flashPhase = "white";
      this.paint();
      if (whiteMs > 0) await sleep(whiteMs);
      this.applyFull(target, { flash: false });
      this.flashPhase = "done";
      return;
    }
    if (this.delayMs > 0) await sleep(this.delayMs);
    this.applyFull(target, opts);
  }

  _bounds(rect) {
    if (!rect) {
      return { x0: 0, y0: 0, x1: this.width, y1: this.height };
    }
    const x0 = Math.max(0, rect.x | 0);
    const y0 = Math.max(0, rect.y | 0);
    const x1 = Math.min(this.width, x0 + (rect.w | 0));
    const y1 = Math.min(this.height, y0 + (rect.h | 0));
    return { x0, y0, x1, y1 };
  }

  /** Draw current ink into an ImageData using e-paper ink/paper colors. */
  toImageData(imageData) {
    const data = imageData.data;
    const pr = PAPER[0];
    const pg = PAPER[1];
    const pb = PAPER[2];
    const ir = INK[0];
    const ig = INK[1];
    const ib = INK[2];
    for (let i = 0; i < this.length; i++) {
      const t = this.ink[i];
      const o = i * 4;
      data[o] = (ir + (pr - ir) * t + 0.5) | 0;
      data[o + 1] = (ig + (pg - ig) * t + 0.5) | 0;
      data[o + 2] = (ib + (pb - ib) * t + 0.5) | 0;
      data[o + 3] = 255;
    }
    return imageData;
  }
}

/** Build a solid gray frame (0–1). */
export function solid(panel, value) {
  const buf = new Float32Array(panel.length);
  buf.fill(clamp01(value));
  return buf;
}

/** Checkerboard alternating 0 and 1 in `cell`-pixel blocks. */
export function checkerboard(panel, cell = 8) {
  const buf = new Float32Array(panel.length);
  for (let y = 0; y < panel.height; y++) {
    for (let x = 0; x < panel.width; x++) {
      const on = ((x / cell) | 0) + ((y / cell) | 0);
      buf[y * panel.width + x] = on & 1 ? 0 : 1;
    }
  }
  return buf;
}
