/**
 * Browser entry for localhost:8006. Reads the guest through qemux's VNC
 * websocket and paints via Panel (quantize, delay, ghost, full flash).
 *
 * shortcut: VNC full-frame ingest until issue 7's damage protocol replaces it.
 */
import RFB from "./core/rfb.js";
import { Panel } from "./panel.js";

const screen = document.getElementById("screen");
const canvas = document.getElementById("panel");
const status = document.getElementById("status");
const fullBtn = document.getElementById("full");
const ctx = canvas.getContext("2d", { willReadFrequently: true });

let panel = null;
let image = null;
let lastRgba = null;
let busy = false;
let pending = false;
let updates = 0;
let rfb = null;

/** Guest frames between automatic full refreshes (clears ghosting). */
const FULL_EVERY = 24;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function websockifyUrl() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const path = window.location.pathname.replace(/[^/]*$/, "").replace(/\/$/, "");
  return `${protocol}//${window.location.host}${path}/websockify`;
}

function ensurePanel(width, height) {
  if (panel && panel.width === width && panel.height === height) {
    return;
  }
  panel = new Panel({
    width,
    height,
    grayLevels: 4,
    ghost: 0.45,
    delayMs: 90,
    dither: true,
    invert: true,
  });
  canvas.width = width;
  canvas.height = height;
  image = ctx.createImageData(width, height);
  lastRgba = null;
  updates = 0;
}

function paint() {
  if (!panel || !image) return;
  panel.toImageData(image);
  ctx.putImageData(image, 0, 0);
}

async function showFlash() {
  if (!panel) return;
  panel.ink.fill(0);
  paint();
  await sleep(90);
  panel.ink.fill(1);
  paint();
  await sleep(90);
}

async function ingest(forceFull = false) {
  if (!rfb) return;
  if (busy) {
    pending = true;
    return;
  }
  busy = true;
  try {
    const frame = rfb.getImageData();
    ensurePanel(frame.width, frame.height);
    lastRgba = frame.data;
    updates += 1;

    const wantFull = forceFull || updates % FULL_EVERY === 0;
    if (wantFull) {
      await sleep(panel.delayMs);
      await showFlash();
      panel.applyFull(lastRgba, { flash: false });
    } else {
      await panel.partial(lastRgba);
    }
    paint();
  } catch (err) {
    status.textContent = String(err.message || err);
  } finally {
    busy = false;
    if (pending) {
      pending = false;
      void ingest();
    }
  }
}

function hookFlip() {
  // noVNC has no public "frame ready" event; wrap the display flip.
  const display = rfb._display;
  const flip = display.flip.bind(display);
  display.flip = function hookedFlip() {
    flip();
    void ingest();
  };
}

fullBtn.addEventListener("click", () => {
  void ingest(true);
});

status.textContent = "Connecting to the guest…";
fullBtn.disabled = true;

rfb = new RFB(screen, websockifyUrl());
rfb.scaleViewport = true;
rfb.focusOnClick = true;

rfb.addEventListener("connect", () => {
  status.textContent = "Partial updates leave ghosts. Full refresh clears them.";
  fullBtn.disabled = false;
  hookFlip();
  void ingest(true);
});

rfb.addEventListener("disconnect", (event) => {
  fullBtn.disabled = true;
  if (!event.detail.clean) {
    status.textContent = "Disconnected. Refresh when the guest is up.";
  }
});

rfb.addEventListener("securityfailure", () => {
  status.textContent = "VNC security failure.";
});
