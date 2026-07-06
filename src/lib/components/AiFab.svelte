<script lang="ts">
  import { api, errorMessage } from "$lib/api";
  import { app, openFile, refreshTree, settings, replaceTabContent, toast } from "$lib/state.svelte";
  import * as ai from "$lib/ai";
  import { listen } from "@tauri-apps/api/event";
  import { marked } from "marked";
  import DOMPurify from "dompurify";

  type Msg = { role: "user" | "ai"; text: string; sources?: string[]; streaming?: boolean };

  let open = $state(false);
  let input = $state("");
  let msgs = $state<Msg[]>([]);
  let busy = $state(false);
  let mode = $state<"chat" | "draft">("chat");
  let thread: HTMLDivElement | undefined = $state();

  function md(text: string): string {
    return DOMPurify.sanitize(marked.parse(text, { async: false }));
  }

  // --- persistence: the conversation survives app restarts, per workspace ---
  const chatKey = () => `pugdock-ai-chat:${app.config?.workspace_path ?? ""}`;
  let loadedKey = "";
  $effect(() => {
    const key = chatKey();
    if (key !== loadedKey) {
      loadedKey = key;
      try {
        msgs = JSON.parse(localStorage.getItem(key) ?? "[]");
      } catch {
        msgs = [];
      }
    }
  });
  $effect(() => {
    if (loadedKey) {
      localStorage.setItem(loadedKey, JSON.stringify(msgs.filter((m) => !m.streaming)));
    }
  });

  // --- streaming: append deltas from the backend as they generate ---
  let streamId = 0;
  $effect(() => {
    const un = listen<{ id: string; text: string }>("ai-delta", (e) => {
      const last = msgs[msgs.length - 1];
      if (last?.streaming && e.payload.id === String(streamId)) {
        last.text += e.payload.text;
        scrollDown();
      }
    });
    return () => void un.then((u) => u());
  });

  async function askStreaming(q: string, blocks: [string, string][]) {
    const sources = blocks.map(([p]) => p);
    streamId++;
    const id = String(streamId);
    msgs.push({ role: "ai", text: "", streaming: true });
    const ctx = blocks.map(([p, t]) => `--- ${p} ---\n${t}`).join("\n\n");
    const system =
      "You are PugDock, answering questions about the user's own developer workspace. Use ONLY the provided workspace excerpts. The excerpt marked 'currently open in the editor' is the note the user is looking at right now; treat it as the primary context. Cite the file paths you used. If the answer isn't in the excerpts, say so.";
    const prompt = `Workspace excerpts:\n\n${ctx.slice(0, 100000)}\n\nQuestion: ${q}`;
    try {
      await api.anthropicRunStream(id, settings().model ?? "auto", system, prompt);
      const last = msgs[msgs.length - 1];
      if (last?.streaming) {
        last.streaming = false;
        last.sources = sources;
      }
    } catch (e) {
      const last = msgs[msgs.length - 1];
      if (last?.streaming) msgs.pop();
      // fall back to the non-streaming path (API key / ant CLI providers)
      const answer = await ai.askPugdock(q, blocks.map(([path, text]) => ({ path, text })));
      msgs.push({ role: "ai", text: answer, sources });
      void e;
    }
  }

  const enabled = $derived(settings().aiEnabled);
  const activeTab = $derived(app.tabs.find((t) => t.path === app.activePath));
  const activeIsNote = $derived(!!activeTab && activeTab.kind === "text" && !ai.aiExcluded(activeTab.path));

  function scrollDown() {
    setTimeout(() => thread?.scrollTo({ top: thread.scrollHeight, behavior: "smooth" }), 30);
  }

  async function guarded(fn: () => Promise<void>) {
    busy = true;
    try {
      await fn();
    } catch (e) {
      msgs.push({ role: "ai", text: `⚠️ ${errorMessage(e)}` });
    } finally {
      busy = false;
      scrollDown();
    }
  }

  function submit() {
    const q = input.trim();
    if (!q || busy) return;
    input = "";
    msgs.push({ role: "user", text: q });
    scrollDown();
    if (mode === "draft") {
      mode = "chat";
      guarded(() => createNoteFrom(q));
    } else {
      guarded(async () => {
        let blocks = (await api.searchContext(q, 10)).filter(([p]) => !ai.aiExcluded(p));
        // The open note is always visible to the chat, in its live state
        // (including unsaved edits), listed first as primary context.
        if (activeIsNote && activeTab) {
          blocks = [
            [`${activeTab.path} (currently open in the editor)`, activeTab.content.slice(0, 30000)] as [string, string],
            ...blocks.filter(([p]) => p !== activeTab.path),
          ];
        }
        await askStreaming(q, blocks);
      });
    }
  }

  async function createNoteFrom(request: string) {
    const content = await ai.draftNote(request);
    const title = content.match(/^#\s+(.+)$/m)?.[1] ?? request;
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "note";
    let dest = `notes/${slug}.md`;
    for (let i = 2; app.tabs.some((t) => t.path === dest); i++) dest = `notes/${slug}-${i}.md`;
    await api.writeFile(dest, content);
    await refreshTree();
    await openFile(dest);
    msgs.push({ role: "ai", text: `📝 Created and opened **${dest}**.` });
  }

  function enrichCurrent() {
    if (!activeTab) return;
    const tab = activeTab;
    msgs.push({ role: "user", text: `Enrich "${tab.name}"` });
    guarded(async () => {
      const improved = await ai.enrichNote(tab.content);
      await api.writeFile(tab.path, improved);
      replaceTabContent(tab.path, improved);
      msgs.push({ role: "ai", text: `✨ Enriched **${tab.name}**. The previous version stays in History.` });
    });
  }

  function summarizeCurrent() {
    if (!activeTab) return;
    const tab = activeTab;
    msgs.push({ role: "user", text: `Summarize "${tab.name}"` });
    guarded(async () => {
      const summary = await ai.summarize(tab.name, tab.content);
      msgs.push({ role: "ai", text: summary, sources: [tab.path] });
    });
  }

  function continueWriting() {
    if (!activeTab) return;
    const tab = activeTab;
    msgs.push({ role: "user", text: `Continue writing "${tab.name}"` });
    guarded(async () => {
      const more = await ai.continueWriting(tab.content);
      const updated = tab.content.replace(/\s*$/, "\n\n") + more + "\n";
      await api.writeFile(tab.path, updated);
      replaceTabContent(tab.path, updated);
      msgs.push({ role: "ai", text: `✍️ Continued **${tab.name}**.` });
    });
  }

  async function insertIntoNote(text: string) {
    if (!activeTab || activeTab.kind !== "text") {
      toast("Open a text note first.");
      return;
    }
    const updated = activeTab.content.replace(/\s*$/, "\n\n") + text + "\n";
    await api.writeFile(activeTab.path, updated);
    replaceTabContent(activeTab.path, updated);
    toast(`Inserted into ${activeTab.name}`);
  }

  async function saveAsNote(text: string) {
    const title = text.match(/^#\s+(.+)$/m)?.[1] ?? text.split("\n")[0].slice(0, 50);
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "note";
    const dest = `notes/${slug}.md`;
    await api.writeFile(dest, text.startsWith("#") ? text : `# ${title}\n\n${text}`);
    await refreshTree();
    await openFile(dest);
    toast(`Saved as ${dest}`);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) open = false;
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="ai-pop">
    <div class="ai-head">
      <span class="ai-title">🐾 PugDock AI</span>
      {#if msgs.length > 0}
        <button
          class="ghost new-chat"
          data-tip="Reset the conversation and start a new chat. Current messages are discarded"
          onclick={() => (msgs = [])}
        >
          <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
            <line x1="12" y1="7" x2="12" y2="13" />
            <line x1="9" y1="10" x2="15" y2="10" />
          </svg>
          New chat
        </button>
      {/if}
      <button class="ghost" onclick={() => (open = false)} data-tip="Close (keeps the conversation)">×</button>
    </div>

    {#if !enabled}
      <div class="ai-connect">
        <p>Sign in with Anthropic to ask questions, generate notes and enrich your writing.</p>
        <button class="primary" onclick={() => { open = false; app.panel = "settings"; }}>
          Open AI settings
        </button>
      </div>
    {:else}
      <div class="thread" bind:this={thread}>
        {#if msgs.length === 0}
          <p class="hint">
            Ask anything about your workspace, or use a quick action below.
          </p>
        {/if}
        {#each msgs as m, i (i)}
          <div class="msg {m.role}">
            {#if m.role === "ai"}
              <!-- eslint-disable-next-line svelte/no-at-html-tags (sanitized in md()) -->
              <div class="msg-text md">{@html md(m.text)}</div>
            {:else}
              <div class="msg-text">{m.text}</div>
            {/if}
            {#if m.role === "ai" && !m.text.startsWith("⚠️") && !m.text.startsWith("📝") && !m.text.startsWith("✨") && !m.text.startsWith("✍️")}
              <div class="msg-actions">
                <button class="ghost" onclick={() => insertIntoNote(m.text)}>Insert into note</button>
                <button class="ghost" onclick={() => saveAsNote(m.text)}>Save as note</button>
              </div>
            {/if}
            {#if m.sources?.length}
              <div class="sources">
                {#each m.sources as s (s)}
                  <button class="ghost" onclick={() => openFile(s)}>{s}</button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
        {#if busy}<div class="msg ai"><div class="msg-text pulse">Thinking…</div></div>{/if}
      </div>

      <div class="chips">
        <button class="chip" class:on={mode === "draft"} onclick={() => (mode = mode === "draft" ? "chat" : "draft")}>
          ✍️ Draft a note
        </button>
        {#if activeIsNote}
          <button class="chip" onclick={enrichCurrent} disabled={busy}>✨ Enrich</button>
          <button class="chip" onclick={continueWriting} disabled={busy}>➡️ Continue writing</button>
          <button class="chip" onclick={summarizeCurrent} disabled={busy}>📄 Summarize</button>
        {/if}
      </div>

      {#if activeIsNote && activeTab}
        <div class="seeing" data-tip="The chat reads this note live, including unsaved edits">
          👁 Seeing: {activeTab.name}
        </div>
      {/if}
      <form
        class="ai-input"
        onsubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <input
          bind:value={input}
          autofocus
          disabled={busy}
          placeholder={mode === "draft" ? "What should the note be about?" : "Ask, search, or generate…"}
        />
        <button type="submit" class="send" disabled={busy || !input.trim()}>↑</button>
      </form>
    {/if}
  </div>
{/if}

<button class="ai-fab" class:open onclick={() => (open = !open)} data-tip="PugDock AI" data-tip-pos="top">
  {#if open}
    ×
  {:else}
    <img src="/PugDockAI.png" alt="PugDock AI" />
  {/if}
</button>

<style>
  .ai-fab {
    position: fixed;
    right: 24px;
    bottom: 24px;
    width: 52px;
    height: 52px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    font-size: 22px;
    line-height: 1;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    z-index: 400;
    transition: transform 0.15s ease;
    padding: 0;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .ai-fab img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 50%;
  }
  .ai-fab:hover {
    transform: scale(1.08);
    background: var(--bg-hover);
  }
  .ai-fab.open {
    font-size: 20px;
    color: var(--text-dim);
  }
  .ai-pop {
    position: fixed;
    right: 24px;
    bottom: 84px;
    width: 400px;
    max-width: calc(100vw - 48px);
    height: 540px;
    max-height: calc(100vh - 130px);
    display: flex;
    flex-direction: column;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
    z-index: 399;
    overflow: hidden;
  }
  .ai-head {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 10px 10px 8px 14px;
    border-bottom: 1px solid var(--border);
  }
  .ai-title {
    flex: 1;
    font-weight: 600;
    font-size: 13px;
  }
  .new-chat {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 9px;
  }
  .ai-connect {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 24px;
    text-align: center;
    color: var(--text-dim);
  }
  .thread {
    flex: 1;
    overflow-y: auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .hint {
    color: var(--text-dim);
    font-size: 12.5px;
    text-align: center;
    margin: auto 12px;
  }
  .msg {
    max-width: 92%;
    border-radius: 10px;
    padding: 8px 11px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .msg.user {
    align-self: flex-end;
    background: var(--bg-active);
  }
  .msg.ai {
    align-self: flex-start;
    background: var(--bg);
    border: 1px solid var(--border);
  }
  .msg-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .msg-text.md {
    white-space: normal;
  }
  .msg-text.md :global(p) {
    margin: 0.35em 0;
  }
  .msg-text.md :global(pre) {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    overflow-x: auto;
    font-size: 11.5px;
  }
  .msg-text.md :global(code) {
    font-size: 11.5px;
  }
  .msg-text.md :global(ul),
  .msg-text.md :global(ol) {
    padding-left: 1.4em;
    margin: 0.35em 0;
  }
  .msg-text.md :global(h1),
  .msg-text.md :global(h2),
  .msg-text.md :global(h3) {
    font-size: 1.05em;
    margin: 0.5em 0 0.25em;
  }
  .pulse {
    color: var(--text-dim);
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }
  .msg-actions,
  .sources {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 6px;
  }
  .msg-actions button,
  .sources button {
    font-size: 11px;
    padding: 2px 8px;
    color: var(--accent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
  }
  .chip {
    font-size: 11.5px;
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--bg);
  }
  .chip.on {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg));
    border-color: var(--accent);
  }
  .seeing {
    padding: 6px 14px 0;
    font-size: 11px;
    color: var(--text-dim);
  }
  .ai-input {
    display: flex;
    gap: 8px;
    padding: 8px 12px 12px;
  }
  .ai-input input {
    flex: 1;
    border-radius: 10px;
  }
  .send {
    width: 34px;
    border-radius: 10px;
    background: var(--accent);
    border-color: var(--accent);
    color: #10121a;
    font-weight: 700;
  }
</style>
