import { api } from "./api";
import { app, settings, toast, refreshTree, refreshOpenTabs } from "./state.svelte";
import * as ai from "./ai";
import { listen } from "@tauri-apps/api/event";

export type Msg = { role: "user" | "ai"; text: string; sources?: string[]; streaming?: boolean };
export type ChatMeta = { id: string; title: string; updated: number };

/** One shared conversation for the whole app (side panel + PugAI popup).
 *  It survives closing the popup; only New chat starts a fresh one. */
export const chat = $state({
  msgs: [] as Msg[],
  chatId: null as string | null,
  view: "chat" as "chat" | "list",
  conversations: [] as ChatMeta[],
  busy: false,
  mode: "chat" as "chat" | "draft",
  /** bumped on every streamed append so surfaces can autoscroll */
  tick: 0,
  /** what the agent is doing right now ("Writing notes/x.md") */
  activity: null as string | null,
});

// Conversations are files in .chats/ inside the workspace: hidden from the
// tree, synced like any note.
const CHAT_DIR = ".chats";

export function chatTitle(list: Msg[]): string {
  return list.find((m) => m.role === "user")?.text.slice(0, 48) ?? "New chat";
}

let streamSeq = 0;
let pending = "";
let drainTimer: ReturnType<typeof setInterval> | null = null;

/** Typewriter drain: incoming deltas arrive in coarse chunks; render them
 *  a few characters at a time so the reply reads like natural writing. */
function ensureDrain() {
  if (drainTimer) return;
  drainTimer = setInterval(() => {
    const last = chat.msgs[chat.msgs.length - 1];
    if (!last?.streaming) {
      pending = "";
      stopDrain();
      return;
    }
    if (!pending.length) return;
    // adaptive: catch up faster when the buffer grows
    const n = Math.max(2, Math.round(pending.length / 12));
    last.text += pending.slice(0, n);
    pending = pending.slice(n);
    chat.tick++;
  }, 24);
}

function stopDrain() {
  if (drainTimer) clearInterval(drainTimer);
  drainTimer = null;
}

function flushPending() {
  const last = chat.msgs[chat.msgs.length - 1];
  if (last?.streaming && pending.length) {
    last.text += pending;
    chat.tick++;
  }
  pending = "";
  stopDrain();
}

$effect.root(() => {
  // reset when the workspace changes (paths are workspace-relative)
  let wsPath = "";
  $effect(() => {
    const current = app.config?.workspace_path ?? "";
    if (current !== wsPath) {
      wsPath = current;
      chat.chatId = null;
      chat.msgs = [];
      chat.view = "chat";
    }
  });

  // debounced save of the active conversation
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const clean = chat.msgs.filter((m) => !m.streaming).map((m) => ({ ...m }));
    if (!clean.length) return;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      chat.chatId ??= String(Date.now());
      const data = { id: chat.chatId, title: chatTitle(clean), updated: Date.now(), msgs: clean };
      api.writeFile(`${CHAT_DIR}/${chat.chatId}.json`, JSON.stringify(data, null, 1)).catch(() => {});
    }, 800);
  });

  // single global stream listener (surfaces just render chat.msgs)
  listen<{ id: string; text: string }>("ai-delta", (e) => {
    const last = chat.msgs[chat.msgs.length - 1];
    if (last?.streaming && e.payload.id === String(streamSeq)) {
      pending += e.payload.text;
      ensureDrain();
    }
  });
  listen<{ id: string; text: string }>("ai-activity", (e) => {
    if (e.payload.id === String(streamSeq)) {
      chat.activity = e.payload.text;
      chat.tick++;
    }
  });
});

export async function loadConversations() {
  const names = await api.listFiles(CHAT_DIR).catch(() => [] as string[]);
  const metas: ChatMeta[] = [];
  for (const n of names.filter((n) => n.endsWith(".json"))) {
    try {
      const d = JSON.parse(await api.readFile(`${CHAT_DIR}/${n}`));
      metas.push({ id: d.id ?? n.replace(/\.json$/, ""), title: d.title ?? "Chat", updated: d.updated ?? 0 });
    } catch {
      /* skip corrupt file */
    }
  }
  chat.conversations = metas.sort((a, b) => b.updated - a.updated);
}

export async function openConversation(id: string) {
  try {
    const d = JSON.parse(await api.readFile(`${CHAT_DIR}/${id}.json`));
    chat.msgs = d.msgs ?? [];
    chat.chatId = id;
    chat.view = "chat";
    chat.tick++;
  } catch {
    toast("Could not open that conversation.");
  }
}

export function startNewChat() {
  chat.chatId = null;
  chat.msgs = [];
  chat.view = "chat";
}

export async function deleteConversation(id: string) {
  await api.deletePath(`${CHAT_DIR}/${id}.json`).catch(() => {});
  chat.conversations = chat.conversations.filter((c) => c.id !== id);
  if (chat.chatId === id) startNewChat();
}

export async function askStreaming(q: string, blocks: [string, string][]) {
  const sources = blocks.map(([p]) => p);
  streamSeq++;
  const id = String(streamSeq);
  pending = "";
  chat.msgs.push({ role: "ai", text: "", streaming: true });
  const ctx = blocks.map(([p, t]) => `--- ${p} ---\n${t}`).join("\n\n");
  const system =
    "You are PugDock, the AI agent inside the user's notes workspace. Your working directory IS the workspace: use your file tools (Read, Write, Edit, Glob, Grep) to act, not just talk. " +
    "When the user asks you to create a note, folder, table, image reference, or to change content, DO IT with tools (Write/Edit), using workspace-relative paths and Markdown (.md) for notes; then reply with a short summary of what you changed. " +
    "Never touch files outside the working directory, never edit .chats/ or dotfiles, and prefer creating notes under an appropriate folder (notes/ by default). " +
    "For pure questions, answer from the provided excerpts and cite the file paths you used; if the answer isn't there, say so. " +
    "The excerpt marked 'currently open in the editor' is the note the user is looking at right now; treat it as the primary context.";
  const prompt = `Workspace excerpts:\n\n${ctx.slice(0, 100000)}\n\nUser request: ${q}`;
  try {
    await api.anthropicRunStream(id, settings().model ?? "auto", system, prompt);
    flushPending();
    const last = chat.msgs[chat.msgs.length - 1];
    if (last?.streaming) {
      last.streaming = false;
      last.sources = sources;
    }
    chat.activity = null;
    // the agent may have created or edited files: refresh the visible world
    refreshTree().catch(() => {});
    refreshOpenTabs().catch(() => {});
  } catch (e) {
    flushPending();
    chat.activity = null;
    const last = chat.msgs[chat.msgs.length - 1];
    if (last?.streaming) chat.msgs.pop();
    // fall back to the non-streaming path (API key / ant CLI providers)
    const answer = await ai.askPugdock(q, blocks.map(([path, text]) => ({ path, text })));
    chat.msgs.push({ role: "ai", text: answer, sources });
    void e;
  }
}
