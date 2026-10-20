/** Re-exports docker-backed emulator helpers (kept for older imports). */
export {
  composeLogs,
  composePs,
  publishDamageInContainer as publishDamage,
  requireContainer,
  viewerReachable,
} from "./docker.mjs";
