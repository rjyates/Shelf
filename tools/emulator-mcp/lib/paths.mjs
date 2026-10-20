import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));

/** Repo root (oseink/). */
export const ROOT = resolve(HERE, "../../..");

export const VIEWER = join(ROOT, "emulator/viewer");
export const EMULATOR_DIR = join(ROOT, "emulator");

export const VIEWER_URL =
  process.env.OSEINK_VIEWER_URL || "http://127.0.0.1:8006/";

export function composeProject() {
  return process.env.OSEINK_COMPOSE_PROJECT || "oseink";
}
