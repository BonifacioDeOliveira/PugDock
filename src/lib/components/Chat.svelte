<script lang="ts">
  import { api, errorMessage, type Model } from "$lib/api";
  import { app, openFile, refreshTree, settings, saveSettings, replaceTabContent, toast } from "$lib/state.svelte";
  import * as ai from "$lib/ai";
  import { chat, chatTitle, loadConversations, openConversation, startNewChat, deleteConversation, askStreaming } from "$lib/chat.svelte";
  import { marked } from "marked";
  import DOMPurify from "dompurify";

  let input = $state("");
  let thread: HTMLDivElement | undefined = $state();
  let inputEl: HTMLTextAreaElement | undefined = $state();
  let modelList = $state<Model[]>([]);

  $effect(() => {
    if (settings().aiEnabled) ai.models().then((m) => (modelList = m)).catch(() => {});
  });

  // autoscroll as streamed text lands
  $effect(() => {
    void chat.tick;
    void chat.msgs.length;
    scrollDown();
  });

  function md(text: string): string {
    return DOMPurify.sanitize(marked.parse(text, { async: false }));
  }

  function toggleHistory() {
    if (chat.view === "list") chat.view = "chat";
    else {
      loadConversations();
      chat.view = "list";
    }
  }

  const enabled = $derived(settings().aiEnabled);
  const activeTab = $derived(app.tabs.find((t) => t.path === app.activePath));
  const activeIsNote = $derived(!!activeTab && activeTab.kind === "text" && !ai.aiExcluded(activeTab.path));

  function scrollDown() {
    setTimeout(() => thread?.scrollTo({ top: thread.scrollHeight }), 20);
  }

  async function guarded(fn: () => Promise<void>) {
    chat.busy = true;
    try {
      await fn();
    } catch (e) {
      chat.msgs.push({ role: "ai", text: `⚠️ ${errorMessage(e)}` });
    } finally {
      chat.busy = false;
      scrollDown();
    }
  }

  function submit() {
    const q = input.trim();
    if (!q || chat.busy) return;
    input = "";
    if (inputEl) inputEl.style.height = "auto";
    chat.msgs.push({ role: "user", text: q });
    chat.view = "chat";
    scrollDown();
    if (chat.mode === "draft") {
      chat.mode = "chat";
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
    chat.msgs.push({ role: "ai", text: `📝 Created and opened **${dest}**.` });
  }

  function enrichCurrent() {
    if (!activeTab) return;
    const tab = activeTab;
    chat.msgs.push({ role: "user", text: `Enrich "${tab.name}"` });
    guarded(async () => {
      const improved = await ai.enrichNote(tab.content);
      await api.writeFile(tab.path, improved);
      replaceTabContent(tab.path, improved);
      chat.msgs.push({ role: "ai", text: `✨ Enriched **${tab.name}**. The previous version stays in History.` });
    });
  }

  function summarizeCurrent() {
    if (!activeTab) return;
    const tab = activeTab;
    chat.msgs.push({ role: "user", text: `Summarize "${tab.name}"` });
    guarded(async () => {
      const summary = await ai.summarize(tab.name, tab.content);
      chat.msgs.push({ role: "ai", text: summary, sources: [tab.path] });
    });
  }

  function continueWriting() {
    if (!activeTab) return;
    const tab = activeTab;
    chat.msgs.push({ role: "user", text: `Continue writing "${tab.name}"` });
    guarded(async () => {
      const more = await ai.continueWriting(tab.content);
      const updated = tab.content.replace(/\s*$/, "\n\n") + more + "\n";
      await api.writeFile(tab.path, updated);
      replaceTabContent(tab.path, updated);
      chat.msgs.push({ role: "ai", text: `✍️ Continued **${tab.name}**.` });
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

  function autoGrow() {
    if (!inputEl) return;
    inputEl.style.height = "auto";
    inputEl.style.height = `${Math.min(inputEl.scrollHeight, 120)}px`;
  }

  function onInputKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }
</script>

<div class="chat">
  {#if !enabled}
    <div class="connect">
      <p>Sign in with Anthropic to ask questions, generate notes and enrich your writing.</p>
      <button class="primary" onclick={() => (app.panel = "settings")}>Open AI settings</button>
    </div>
  {:else}
    <div class="chat-bar">
      <span class="bar-title">{chat.view === "list" ? "Conversations" : chatTitle(chat.msgs)}</span>
      <button
        class="ghost bar-btn"
        aria-label="Past conversations"
        data-tip="Past conversations"
        data-tip-align="end"
        class:on={chat.view === "list"}
        onclick={toggleHistory}
      >
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="10" />
          <polyline points="12 6 12 12 16 14" />
        </svg>
      </button>
      <button
        class="ghost bar-btn"
        aria-label="New chat"
        data-tip="New chat. This conversation stays in History"
        data-tip-align="end"
        onclick={startNewChat}
      >
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          <line x1="12" y1="7" x2="12" y2="13" />
          <line x1="9" y1="10" x2="15" y2="10" />
        </svg>
      </button>
    </div>

    {#if chat.view === "list"}
      <div class="thread">
        {#if chat.conversations.length === 0}
          <p class="hint">No past conversations yet.</p>
        {/if}
        {#each chat.conversations as c (c.id)}
          <div class="conv" class:current={c.id === chat.chatId}>
            <button class="conv-open" onclick={() => openConversation(c.id)}>
              <span class="conv-title">{c.title}</span>
              <span class="conv-date">{new Date(c.updated).toLocaleString()}</span>
            </button>
            <button
              class="ghost conv-del"
              aria-label="Delete conversation"
              data-tip="Delete conversation"
              data-tip-align="end"
              onclick={() => deleteConversation(c.id)}
            >×</button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="thread" bind:this={thread}>
        {#if chat.msgs.length === 0}
          <p class="hint">Ask anything about your workspace, or use a quick action below.</p>
        {/if}
        {#each chat.msgs as m, i (i)}
          {@const actionable = m.role === "ai" && !m.streaming && !m.text.startsWith("⚠️") && !m.text.startsWith("📝") && !m.text.startsWith("✨") && !m.text.startsWith("✍️")}
          <div class="msg {m.role}">
            {#if m.role === "ai" && m.streaming && !m.text}
              <div class="thinking">
                <span class="throbber" aria-hidden="true"></span>
                <span class="pulse">{chat.activity ?? "Thinking…"}</span>
              </div>
            {:else if m.role === "ai"}
              <!-- eslint-disable-next-line svelte/no-at-html-tags (sanitized in md()) -->
              <div class="msg-text mdv">{@html md(m.text)}</div>
              {#if m.streaming && chat.activity}
                <div class="activity">🛠 {chat.activity}</div>
              {/if}
            {:else}
              <div class="msg-text">{m.text}</div>
            {/if}
            {#if actionable || m.sources?.length}
              <div class="msg-foot">
                <div class="sources">
                  {#each m.sources ?? [] as s (s)}
                    <button class="ghost" onclick={() => openFile(s)}>{s}</button>
                  {/each}
                </div>
                {#if actionable}
                  <div class="msg-actions">
                    <button
                      class="ghost icon-action"
                      aria-label="Insert into the open note"
                      data-tip="Insert into the open note"
                      data-tip-pos="top"
                      data-tip-align="end"
                      onclick={() => insertIntoNote(m.text)}
                    >
                      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                        <polyline points="14 2 14 8 20 8" />
                        <line x1="12" y1="11" x2="12" y2="17" />
                        <polyline points="9 14 12 17 15 14" />
                      </svg>
                    </button>
                    <button
                      class="ghost icon-action"
                      aria-label="Save as a new note"
                      data-tip="Save as a new note"
                      data-tip-pos="top"
                      data-tip-align="end"
                      onclick={() => saveAsNote(m.text)}
                    >
                      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                        <polyline points="14 2 14 8 20 8" />
                        <line x1="12" y1="18" x2="12" y2="12" />
                        <line x1="9" y1="15" x2="15" y2="15" />
                      </svg>
                    </button>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
        {#if chat.busy && !chat.msgs[chat.msgs.length - 1]?.streaming}
          <div class="msg ai thinking">
            <span class="throbber" aria-hidden="true"></span>
            <span class="pulse">Thinking…</span>
          </div>
        {/if}
      </div>
    {/if}

    <div class="chips">
      <button class="chip" class:on={chat.mode === "draft"} onclick={() => (chat.mode = chat.mode === "draft" ? "chat" : "draft")}>
        ✍️ Draft a note
      </button>
      {#if activeIsNote}
        <button class="chip" onclick={enrichCurrent} disabled={chat.busy}>✨ Enrich</button>
        <button class="chip" onclick={continueWriting} disabled={chat.busy}>➡️ Continue</button>
        <button class="chip" onclick={summarizeCurrent} disabled={chat.busy}>📄 Summarize</button>
      {/if}
    </div>

    <div class="input-box">
      <textarea
        bind:this={inputEl}
        bind:value={input}
        rows="1"
        disabled={chat.busy}
        placeholder={chat.mode === "draft" ? "What should the note be about?" : "Ask, search, or generate… (Enter to send)"}
        oninput={autoGrow}
        onkeydown={onInputKeydown}
      ></textarea>
      <div class="input-row">
        {#if activeIsNote && activeTab}
          <span class="seeing" data-tip="The chat reads this note live, including unsaved edits" data-tip-pos="top">
            👁 {activeTab.name}
          </span>
        {/if}
        <select
          class="model-pick"
          data-tip="Model used by the chat and every AI action"
          data-tip-pos="top"
          value={settings().model ?? "auto"}
          onchange={(e) => saveSettings({ model: e.currentTarget.value })}
        >
          <option value="auto">Auto</option>
          {#each modelList as m (m.id)}
            <option value={m.id}>{m.display_name}</option>
          {/each}
        </select>
        <div class="grow"></div>
        <button class="send" aria-label="Send" disabled={chat.busy || !input.trim()} onclick={submit}>↑</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .connect {
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
  .chat-bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 8px 6px 14px;
    border-bottom: 1px solid var(--border);
    flex: 0 0 auto;
  }
  .bar-title {
    flex: 1;
    font-size: 12px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar-btn {
    display: flex;
    align-items: center;
    padding: 4px 7px;
    border-radius: 5px;
  }
  .bar-btn.on {
    background: var(--bg-active);
    color: var(--text);
  }
  .thread {
    flex: 1;
    overflow-y: auto;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }
  .hint {
    color: var(--text-dim);
    font-size: 12.5px;
    text-align: center;
    margin: auto 12px;
  }
  .msg {
    max-width: 94%;
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
    align-self: stretch;
    max-width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
  }
  .msg-text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .msg-text.mdv {
    white-space: normal;
  }
  .msg-text.mdv :global(p) {
    margin: 0.35em 0;
  }
  .msg-text.mdv :global(pre) {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    overflow-x: auto;
    font-size: 11.5px;
  }
  .msg-text.mdv :global(code) {
    font-size: 11.5px;
  }
  .msg-text.mdv :global(ul),
  .msg-text.mdv :global(ol) {
    padding-left: 1.4em;
    margin: 0.35em 0;
  }
  .msg-text.mdv :global(h1),
  .msg-text.mdv :global(h2),
  .msg-text.mdv :global(h3) {
    font-size: 1.05em;
    margin: 0.5em 0 0.25em;
  }
  .activity {
    margin-top: 6px;
    font-size: 11px;
    color: var(--text-dim);
    font-style: italic;
  }
  .thinking {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .throbber {
    width: 14px;
    height: 14px;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    flex-shrink: 0;
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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
  .msg-foot {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    margin-top: 6px;
  }
  .sources {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    min-width: 0;
  }
  .sources button {
    font-size: 11px;
    padding: 2px 8px;
    color: var(--accent);
  }
  .msg-actions {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .icon-action {
    display: flex;
    align-items: center;
    padding: 3px 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--accent);
  }
  .icon-action:hover {
    border-color: var(--accent);
  }
  .conv {
    display: flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 2px 4px 2px 0;
  }
  .conv.current {
    border-color: var(--accent);
  }
  .conv-open {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    background: none;
    border: none;
    padding: 7px 10px;
    text-align: left;
    min-width: 0;
  }
  .conv-title {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .conv-date {
    font-size: 10.5px;
    color: var(--text-dim);
  }
  .conv-del:hover {
    color: var(--danger);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 8px 12px 0;
    flex: 0 0 auto;
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
  .input-box {
    margin: 10px 12px 12px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--bg);
    display: flex;
    flex-direction: column;
    flex: 0 0 auto;
  }
  .input-box:focus-within {
    border-color: var(--accent);
  }
  textarea {
    border: none;
    background: none;
    resize: none;
    padding: 10px 12px 4px;
    font-size: 12.5px;
    line-height: 1.45;
    max-height: 120px;
  }
  textarea:focus {
    border: none;
    outline: none;
  }
  .input-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px 8px 12px;
  }
  .seeing {
    font-size: 11px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 40%;
  }
  .model-pick {
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 6px;
    background: var(--bg-panel);
    color: var(--text-dim);
    max-width: 150px;
  }
  .grow {
    flex: 1;
  }
  .send {
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: var(--accent);
    border-color: var(--accent);
    color: #10121a;
    font-weight: 700;
    padding: 0;
    flex-shrink: 0;
  }
  .send:disabled {
    opacity: 0.4;
  }
</style>
