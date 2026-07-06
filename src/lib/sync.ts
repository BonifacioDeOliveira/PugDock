import { api, errorCode } from "./api";
import { app, applyStatus, settings, syncEnabled } from "./state.svelte";

// Debounced local save → idle checkpoint → periodic push.
// The user never waits on the network to type or save.

let saveTimers = new Map<string, ReturnType<typeof setTimeout>>();
let checkpointTimer: ReturnType<typeof setTimeout> | null = null;
let pushInterval: ReturnType<typeof setInterval> | null = null;

/** Debounced write-to-disk for one file (call on every editor change). */
export function scheduleSave(path: string, getContent: () => string) {
  app.syncState = "saving";
  clearTimeout(saveTimers.get(path));
  saveTimers.set(
    path,
    setTimeout(async () => {
      saveTimers.delete(path);
      try {
        await api.writeFile(path, getContent());
        api.indexFile(path).catch(() => {});
        const tab = app.tabs.find((t) => t.path === path);
        if (tab) tab.dirty = false;
        if (app.syncState === "saving") app.syncState = syncEnabled() ? "saved-locally" : "saved";
        scheduleCheckpoint();
      } catch {
        app.syncState = "needs-review";
      }
    }, 600),
  );
}

/** Flush all pending saves immediately (used on tab close / app exit). */
export async function flushSaves() {
  const pending = [...saveTimers.keys()];
  for (const path of pending) {
    clearTimeout(saveTimers.get(path));
    saveTimers.delete(path);
    const tab = app.tabs.find((t) => t.path === path);
    if (tab?.kind === "text") {
      await api.writeFile(path, tab.content);
      tab.dirty = false;
    }
  }
}

function scheduleCheckpoint() {
  if (settings().syncMode === "manual") return;
  if (checkpointTimer) clearTimeout(checkpointTimer);
  checkpointTimer = setTimeout(checkpoint, settings().checkpointSeconds * 1000);
}

async function checkpoint() {
  if (app.conflicts.length > 0) return;
  try {
    app.syncState = "checkpointing";
    await api.gitCheckpoint();
    applyStatus(await api.gitStatus());
  } catch {
    // stay quiet; next cycle retries
    app.syncState = "saved-locally";
  }
}

/** Full sync: checkpoint pending edits, pull, then push. */
export async function syncNow() {
  if (!syncEnabled()) {
    // Local-only mode: checkpoint for history, nothing to push.
    await flushSaves();
    await api.gitCheckpoint().catch(() => {});
    app.syncState = "saved";
    return;
  }
  try {
    await flushSaves();
    app.syncState = "checkpointing";
    await api.gitCheckpoint();
    app.syncState = "syncing";
    const st = await api.gitPull();
    if (st.conflicts.length > 0) {
      applyStatus(st);
      return;
    }
    await api.gitPush();
    applyStatus(await api.gitStatus());
  } catch (e) {
    if (errorCode(e) === "offline") {
      const st = await api.gitStatus().catch(() => null);
      app.pendingChanges = st?.ahead ?? app.pendingChanges;
      app.syncState = "offline";
    } else if (errorCode(e) === "sync_conflict") {
      applyStatus(await api.gitStatus());
    } else {
      throw e;
    }
  }
}

/** Start background schedulers. Call once when the workspace opens. */
export function startSync() {
  if (pushInterval) clearInterval(pushInterval);
  if (syncEnabled() && settings().syncMode !== "manual") {
    const secs = settings().syncMode === "frequent" ? 60 : settings().pushSeconds;
    pushInterval = setInterval(async () => {
      const st = await api.gitStatus().catch(() => null);
      if (st && (st.ahead > 0 || st.dirty)) await syncNow().catch(() => {});
    }, secs * 1000);
  }
  window.addEventListener("online", () => {
    if (app.syncState === "offline") syncNow().catch(() => {});
  });
}

export async function pushOnExit() {
  await flushSaves().catch(() => {});
  await api.gitCheckpoint().catch(() => {});
  if (syncEnabled() && settings().pushOnExit) {
    await api.gitPush().catch(() => {});
  }
}
