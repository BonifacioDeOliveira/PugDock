import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api } from "./api";

export interface AvailableUpdate {
  version: string;
  notes: string;
  url: string | null;
  /** Present when the signed in-place updater can install it. */
  install: (() => Promise<void>) | null;
}

/**
 * Native signed updater first (downloads, installs, relaunches); falls back
 * to the plain GitHub Releases check (link to the release page) when the
 * updater isn't available - e.g. dev builds or unsigned artifacts.
 */
export async function checkForUpdate(includePrereleases = false): Promise<AvailableUpdate | null> {
  try {
    const update = await check();
    if (update) {
      return {
        version: update.version,
        notes: update.body ?? "",
        url: null,
        install: async () => {
          await update.downloadAndInstall();
          await relaunch();
        },
      };
    }
    return null;
  } catch {
    // updater not configured for this build - fall through
  }
  const info = await api.checkUpdates(includePrereleases).catch(() => null);
  return info ? { version: info.latest, notes: info.notes, url: info.url, install: null } : null;
}
