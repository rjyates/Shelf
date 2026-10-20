/**
 * Render damage the container is serving at /guest/ through the panel simulator.
 * Live pixels always come from the docker viewer's HTTP, never host overlay paths.
 */
import { applyDamage, parseDamage } from "../../../emulator/viewer/damage.js";
import { Panel } from "../../../emulator/viewer/panel.js";
import { fetchFromViewer, requireContainer, viewerReachable } from "./docker.mjs";
import { rgbToPng } from "./png.mjs";

export async function readDamageMetaFromContainer() {
  await requireContainer();
  const raw = await fetchFromViewer("guest/damage.json", { as: "json" });
  return parseDamage(raw);
}

export async function renderDamagePng({ mode, forceFull = false } = {}) {
  await requireContainer();
  const viewer = await viewerReachable();
  if (!viewer.ok) {
    throw new Error(`viewer not reachable: ${viewer.error || viewer.status}`);
  }

  const metaIn = parseDamage(await fetchFromViewer("guest/damage.json", { as: "json" }));
  let meta = metaIn;
  if (mode) meta = { ...meta, mode };
  if (forceFull) {
    meta = {
      ...meta,
      mode: "full",
      x: 0,
      y: 0,
      w: meta.panel_w,
      h: meta.panel_h,
    };
  }

  const pixelsName = meta.pixels || "frame.gray";
  const pixels = await fetchFromViewer(`guest/${pixelsName}`, { as: "bytes" });
  const need = meta.panel_w * meta.panel_h;
  if (pixels.length < need) {
    throw new Error(`frame from container too short (${pixels.length} < ${need})`);
  }
  const gray = pixels.subarray(0, need);

  const panel = new Panel({
    width: meta.panel_w,
    height: meta.panel_h,
    delayMs: 0,
    dither: false,
    invert: false,
  });
  await applyDamage(panel, meta, gray);

  const rgba = { data: new Uint8ClampedArray(need * 4) };
  panel.toImageData(rgba);
  const rgb = Buffer.alloc(need * 3);
  for (let i = 0; i < need; i++) {
    const o = i * 4;
    const d = i * 3;
    rgb[d] = rgba.data[o];
    rgb[d + 1] = rgba.data[o + 1];
    rgb[d + 2] = rgba.data[o + 2];
  }
  const png = rgbToPng(meta.panel_w, meta.panel_h, rgb);
  return { png, meta, panel, source: "docker:8006/guest" };
}
