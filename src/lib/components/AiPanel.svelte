<script lang="ts">
  import { api, errorMessage, type Model } from "$lib/api";
  import { app, openFile, refreshTree, settings, saveSettings, replaceTabContent, toast, isTextFile } from "$lib/state.svelte";
  import * as ai from "$lib/ai";

  let question = $state("");
  let modelList = $state<Model[]>([]);

  $effect(() => {
    if (settings().aiEnabled) ai.models().then((m) => (modelList = m)).catch(() => {});
  });
  let chat = $state<{ role: "user" | "pugdock"; text: string; sources?: string[] }[]>([]);
  let busy = $state<string | null>(null);
  let error = $state("");
  let usedFiles = $state<string[]>([]);

  const enabled = $derived(settings().aiEnabled);
  const activeTab = $derived(app.tabs.find((t) => t.path === app.activePath));

  function guardFile(): { path: string; content: string } | null {
    if (!activeTab || activeTab.kind !== "text") {
      error = "Open a text file first.";
      return null;
    }
    if (ai.aiExcluded(activeTab.path)) {
      error = "This file is excluded from AI.";
      return null;
    }
    if (settings().askBeforeSendingCode && !isMarkdown(activeTab.path)) {
      if (!confirm(`Send "${activeTab.path}" to Anthropic?`)) return null;
    }
    return { path: activeTab.path, content: activeTab.content };
  }

  function isMarkdown(p: string) {
    return p.endsWith(".md") || p.endsWith(".txt");
  }

  async function action(name: string, fn: () => Promise<void>) {
    error = "";
    busy = name;
    try {
      await fn();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = null;
    }
  }

  const organize = () =>
    action("organize", async () => {
      const f = guardFile();
      if (!f) return;
      const raw = await ai.suggestLabels(f.path, f.content);
      const labels = JSON.parse(raw.replace(/^```json?\n?|```$/g, ""));
      if (f.path.endsWith(".md")) {
        const fm = `---\ntype: ${labels.type}\ntags: [${(labels.tags ?? []).join(", ")}]\n${labels.project ? `project: ${labels.project}\n` : ""}title: ${labels.title}\n---\n\n`;
        const body = f.content.replace(/^---\n[\s\S]*?\n---\n\n?/, "");
        await api.writeFile(f.path, fm + body);
        replaceTabContent(f.path, fm + body);
      } else {
        await api.writeFile(`${f.path}.pugdock.json`, JSON.stringify(labels, null, 2) + "\n");
        await refreshTree();
      }
      toast(`Labeled as ${labels.type}${labels.tags?.length ? ` · ${labels.tags.join(", ")}` : ""}`);
    });

  const suggestName = () =>
    action("filename", async () => {
      const f = guardFile();
      if (!f) return;
      const suggestion = (await ai.suggestFilename(f.content)).trim();
      if (confirm(`Rename to "${suggestion}"?`)) {
        await api.renamePath(f.path, suggestion);
        const tab = app.tabs.find((t) => t.path === f.path);
        if (tab) {
          tab.path = suggestion;
          tab.name = suggestion.split("/").pop() ?? suggestion;
        }
        if (app.activePath === f.path) app.activePath = suggestion;
        await refreshTree();
      }
    });

  const enrich = () =>
    action("enrich", async () => {
      const f = guardFile();
      if (!f) return;
      const improved = await ai.enrichNote(f.content);
      await api.writeFile(f.path, improved);
      replaceTabContent(f.path, improved);
      toast("Note enriched. Previous version is in history.");
    });

  const explain = () =>
    action("explain", async () => {
      const f = guardFile();
      if (!f) return;
      const explanation = await ai.explainError(f.content);
      const dest = `notes/explained-${(f.path.split("/").pop() ?? "error").replace(/\.[^.]+$/, "")}.md`;
      await api.writeFile(dest, explanation);
      await refreshTree();
      await openFile(dest);
    });

  const summarizeFile = () =>
    action("summarize", async () => {
      if (!activeTab) {
        error = "Open a file first.";
        return;
      }
      let text: string;
      if (activeTab.kind === "pdf") {
        if (settings().askBeforeSendingPdfs && !confirm(`Send the text of "${activeTab.path}" to Anthropic?`)) return;
        const { extractPdfText } = await import("$lib/pdftext");
        text = await extractPdfText(activeTab.content);
        api.indexFile(activeTab.path, text).catch(() => {});
      } else {
        const f = guardFile();
        if (!f) return;
        text = f.content;
      }
      const summary = await ai.summarize(activeTab.name, text);
      const dest = `references/${activeTab.name.replace(/\.[^.]+$/, "")}-summary.md`;
      await api.writeFile(dest, summary);
      await refreshTree();
      await openFile(dest);
    });

  const buildContext = () =>
    action("context", async () => {
      const wsName = app.config?.repo_name ?? "workspace";
      const blocks = await api.folderContents(
        ["notes", "bugs", "adr", "runbooks", "snippets", "commands", "projects", "references"],
        25,
      );
      const eligible = blocks.filter(([p]) => !ai.aiExcluded(p));
      usedFiles = eligible.map(([p]) => p);
      const corpus = eligible.map(([p, t]) => `--- ${p} ---\n${t}`).join("\n\n");
      const result = await ai.buildContext(wsName, corpus || "(workspace is still empty)");
      const dest = `context/${wsName}-context.md`;
      await api.writeFile(dest, result);
      await refreshTree();
      await openFile(dest);
    });

  const ask = () =>
    action("ask", async () => {
      const q = question.trim();
      if (!q) return;
      question = "";
      chat.push({ role: "user", text: q });
      const blocks = (await api.searchContext(q, 10)).filter(([p]) => !ai.aiExcluded(p));
      usedFiles = blocks.map(([p]) => p);
      const answer = await ai.askPugdock(q, blocks.map(([path, text]) => ({ path, text })));
      chat.push({ role: "pugdock", text: answer, sources: usedFiles });
    });

  async function createFromTemplate(kind: "adr" | "runbook") {
    const title = prompt(`${kind === "adr" ? "ADR" : "Runbook"} title:`);
    if (!title) return;
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const dest = `${kind === "adr" ? "adr" : "runbooks"}/${slug}.md`;
    const tpl = (kind === "adr" ? ai.ADR_TEMPLATE : ai.RUNBOOK_TEMPLATE).replace("<title>", title);
    await api.writeFile(dest, tpl);
    await refreshTree();
    await openFile(dest);
  }
</script>

<div class="panel">
  {#if !enabled}
    <p class="dim">Connect Anthropic to use AI features.</p>
    <button class="primary" onclick={() => (app.panel = "settings")}>Open AI settings</button>
    <div class="templates">
      <p class="dim">Templates work without AI:</p>
      <button onclick={() => createFromTemplate("adr")}>Create ADR</button>
      <button onclick={() => createFromTemplate("runbook")}>Create runbook</button>
    </div>
  {:else}
    <label class="model-row">
      <span>Model</span>
      <select
        value={settings().model ?? "auto"}
        onchange={(e) => saveSettings({ model: e.currentTarget.value })}
      >
        <option value="auto">Auto</option>
        {#each modelList as m (m.id)}
          <option value={m.id}>{m.display_name}</option>
        {/each}
      </select>
    </label>
    <div class="actions">
      <button onclick={organize} disabled={!!busy}>{busy === "organize" ? "…" : "Organize"}</button>
      <button onclick={suggestName} disabled={!!busy}>{busy === "filename" ? "…" : "Suggest name"}</button>
      <button onclick={enrich} disabled={!!busy}>{busy === "enrich" ? "…" : "Enrich"}</button>
      <button onclick={explain} disabled={!!busy}>{busy === "explain" ? "…" : "Explain error"}</button>
      <button onclick={summarizeFile} disabled={!!busy}>{busy === "summarize" ? "…" : "Summarize"}</button>
      <button onclick={buildContext} disabled={!!busy}>{busy === "context" ? "…" : "Build context"}</button>
      <button onclick={() => createFromTemplate("adr")}>Create ADR</button>
      <button onclick={() => createFromTemplate("runbook")}>Create runbook</button>
    </div>
    {#if usedFiles.length && busy}
      <p class="dim">Using: {usedFiles.join(", ")}</p>
    {/if}
    <div class="chat">
      {#each chat as msg, i (i)}
        <div class="msg {msg.role}">
          <div class="text">{msg.text}</div>
          {#if msg.sources?.length}
            <div class="sources">
              {#each msg.sources as s (s)}
                <button class="ghost" onclick={() => openFile(s)}>{s}</button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
      {#if busy === "ask"}<p class="dim">Thinking…</p>{/if}
    </div>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        ask();
      }}
    >
      <input placeholder="Ask PugDock about your workspace…" bind:value={question} disabled={!!busy} />
    </form>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .panel {
    height: 100%;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .model-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-dim);
  }
  .model-row select {
    flex: 1;
    font-size: 12px;
    padding: 4px 8px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .actions button {
    font-size: 12px;
    padding: 4px 10px;
  }
  .chat {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .msg {
    border-radius: var(--radius);
    padding: 8px 10px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .msg.user {
    background: var(--bg-active);
    align-self: flex-end;
    max-width: 85%;
  }
  .msg.pugdock {
    background: var(--bg);
    border: 1px solid var(--border);
  }
  .text {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .sources {
    margin-top: 6px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .sources button {
    font-size: 11px;
    color: var(--accent);
  }
  .templates {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
  }
  .dim {
    color: var(--text-dim);
    font-size: 12px;
    margin: 0;
  }
  .error {
    color: var(--danger);
    font-size: 12px;
    margin: 0;
  }
</style>
