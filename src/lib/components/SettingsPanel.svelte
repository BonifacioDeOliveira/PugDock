<script lang="ts">
  import { api, errorMessage, type Model, type UpdateInfo } from "$lib/api";
  import { app, settings, saveSettings, toast } from "$lib/state.svelte";
  import { clearModelCache } from "$lib/ai";
  import { themeState, BUILTIN_THEMES, applyTheme, refreshImportedThemes } from "$lib/theme.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";

  const s = $derived(settings());

  let models = $state<Model[]>([]);
  let anthropicConnected = $state(false);
  let newKey = $state("");
  let error = $state("");
  let indexing = $state(false);
  let update = $state<UpdateInfo | null | "none" | "checking">(null);

  $effect(() => {
    api.hasSecret("anthropic_api_key").then((v) => {
      anthropicConnected = v;
      if (v) api.anthropicModels().then((m) => (models = m)).catch(() => {});
    });
    refreshImportedThemes();
  });

  async function importTheme() {
    error = "";
    const picked = await openDialog({
      title: "Import VSCode theme",
      filters: [{ name: "VSCode extension", extensions: ["vsix"] }],
    });
    if (typeof picked !== "string") return;
    try {
      const imported = await api.importVsixTheme(picked);
      await refreshImportedThemes();
      await applyTheme(`custom:${imported[0].id}`);
      toast(
        imported.length === 1
          ? `Theme "${imported[0].name}" imported and applied`
          : `${imported.length} themes imported — "${imported[0].name}" applied`,
      );
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function deleteTheme() {
    const id = themeState.current.id;
    if (!id.startsWith("custom:")) return;
    await api.deleteImportedTheme(id.replace("custom:", ""));
    await refreshImportedThemes();
    await applyTheme("builtin:dark");
  }

  async function connectKey() {
    error = "";
    try {
      models = await api.anthropicConnect(newKey.trim());
      anthropicConnected = true;
      newKey = "";
      clearModelCache();
      await saveSettings({ aiEnabled: true });
    } catch (e) {
      error = errorMessage(e);
    }
  }

  async function disconnectAi() {
    await api.deleteSecret("anthropic_api_key");
    anthropicConnected = false;
    models = [];
    clearModelCache();
    await saveSettings({ aiEnabled: false });
  }

  async function rebuild() {
    indexing = true;
    try {
      const n = await api.rebuildIndex();
      toast(`Search ready — ${n} files indexed`);
    } finally {
      indexing = false;
    }
  }

  async function checkNow() {
    update = "checking";
    try {
      update = (await api.checkUpdates(s.includePrereleases)) ?? "none";
    } catch (e) {
      error = errorMessage(e);
      update = null;
    }
  }

  async function reconnectGithub() {
    await api.deleteSecret("github_token");
    const cfg = app.config;
    if (cfg) {
      cfg.onboarding_done = false;
      await api.setConfig($state.snapshot(cfg));
      location.reload();
    }
  }
</script>

<div class="panel">
  <section>
    <h3>Account</h3>
    <div class="row"><span>GitHub</span><span>Connected as {s.githubLogin ?? "—"}</span></div>
    <div class="row">
      <span>Repository</span>
      <span>{app.config?.repo_owner}/{app.config?.repo_name} (private)</span>
    </div>
    <div class="btns">
      {#if s.repoHtmlUrl}
        <button onclick={() => s.repoHtmlUrl && openUrl(s.repoHtmlUrl)}>Open repo on GitHub</button>
      {/if}
      <button onclick={reconnectGithub}>Reconnect GitHub</button>
    </div>
  </section>

  <section>
    <h3>Workspace</h3>
    <div class="row"><span>Local folder</span><code>{app.config?.workspace_path}</code></div>
    <div class="btns">
      <button onclick={() => api.reveal("")}>Open local folder</button>
      <button onclick={rebuild} disabled={indexing}>{indexing ? "Indexing workspace…" : "Rebuild search index"}</button>
    </div>
  </section>

  <section>
    <h3>Appearance</h3>
    <label class="row">
      <span>Theme</span>
      <select value={themeState.current.id} onchange={(e) => applyTheme(e.currentTarget.value)}>
        <optgroup label="Built-in">
          {#each BUILTIN_THEMES as theme (theme.id)}
            <option value={theme.id}>{theme.name}</option>
          {/each}
        </optgroup>
        {#if themeState.imported.length}
          <optgroup label="Imported">
            {#each themeState.imported as theme (theme.id)}
              <option value={`custom:${theme.id}`}>{theme.name}</option>
            {/each}
          </optgroup>
        {/if}
      </select>
    </label>
    <div class="btns">
      <button onclick={importTheme}>Import VSCode theme (.vsix)</button>
      {#if themeState.current.id.startsWith("custom:")}
        <button onclick={deleteTheme}>Remove current theme</button>
      {/if}
    </div>
    <p class="dim">
      Any VSCode color theme works: download the extension's .vsix from the Marketplace
      or open-vsx.org and import it here.
    </p>
  </section>

  <section>
    <h3>Sync</h3>
    <label class="row">
      <span>Sync mode</span>
      <select value={s.syncMode} onchange={(e) => saveSettings({ syncMode: e.currentTarget.value as never })}>
        <option value="smart">Smart sync (default)</option>
        <option value="frequent">More frequent</option>
        <option value="manual">Manual only</option>
      </select>
    </label>
    <label class="row">
      <span>Auto-checkpoint after</span>
      <select
        value={String(s.checkpointSeconds)}
        onchange={(e) => saveSettings({ checkpointSeconds: Number(e.currentTarget.value) })}
      >
        <option value="30">30 seconds idle</option>
        <option value="60">60 seconds idle</option>
        <option value="180">3 minutes idle</option>
      </select>
    </label>
    <label class="row">
      <span>Pull on startup</span>
      <input type="checkbox" checked={s.pullOnStartup} onchange={(e) => saveSettings({ pullOnStartup: e.currentTarget.checked })} />
    </label>
    <label class="row">
      <span>Push on exit</span>
      <input type="checkbox" checked={s.pushOnExit} onchange={(e) => saveSettings({ pushOnExit: e.currentTarget.checked })} />
    </label>
  </section>

  <section>
    <h3>AI</h3>
    {#if anthropicConnected}
      <div class="row"><span>Anthropic</span><span>Connected</span></div>
      <label class="row">
        <span>Model mode</span>
        <select value={s.modelMode} onchange={(e) => saveSettings({ modelMode: e.currentTarget.value as never })}>
          <option value="auto">Auto (recommended)</option>
          <option value="fast">Fast</option>
          <option value="balanced">Balanced</option>
          <option value="deep">Deep</option>
          <option value="custom">Custom</option>
        </select>
      </label>
      {#if s.modelMode === "custom"}
        {#each [["fast", "Fast tasks"], ["default", "Default tasks"], ["deep", "Deep tasks"]] as [key, label] (key)}
          <label class="row">
            <span>{label}</span>
            <select
              value={s.customModels?.[key as "fast"] ?? ""}
              onchange={(e) => saveSettings({ customModels: { ...s.customModels, [key]: e.currentTarget.value } })}
            >
              <option value="">(auto)</option>
              {#each models as m (m.id)}
                <option value={m.id}>{m.display_name}</option>
              {/each}
            </select>
          </label>
        {/each}
      {/if}
      <label class="row">
        <span>Ask before sending code</span>
        <input type="checkbox" checked={s.askBeforeSendingCode} onchange={(e) => saveSettings({ askBeforeSendingCode: e.currentTarget.checked })} />
      </label>
      <label class="row">
        <span>Ask before sending PDFs</span>
        <input type="checkbox" checked={s.askBeforeSendingPdfs} onchange={(e) => saveSettings({ askBeforeSendingPdfs: e.currentTarget.checked })} />
      </label>
      <label class="row">
        <span>Excluded from AI</span>
        <input
          placeholder="paths, comma-separated"
          value={s.aiExcluded.join(", ")}
          onchange={(e) => saveSettings({ aiExcluded: e.currentTarget.value.split(",").map((p) => p.trim()).filter(Boolean) })}
        />
      </label>
      <div class="btns"><button onclick={disconnectAi}>Disconnect Anthropic</button></div>
    {:else}
      <div class="btns" style="align-items:center">
        <input type="password" placeholder="Anthropic API key (sk-ant-…)" bind:value={newKey} style="flex:1" />
        <button class="primary" onclick={connectKey} disabled={!newKey.trim()}>Connect</button>
      </div>
    {/if}
  </section>

  <section>
    <h3>Updates</h3>
    <label class="row">
      <span>Automatically check for updates</span>
      <input type="checkbox" checked={s.autoCheckUpdates} onchange={(e) => saveSettings({ autoCheckUpdates: e.currentTarget.checked })} />
    </label>
    <label class="row">
      <span>Include beta releases</span>
      <input type="checkbox" checked={s.includePrereleases} onchange={(e) => saveSettings({ includePrereleases: e.currentTarget.checked })} />
    </label>
    <div class="btns">
      <button onclick={checkNow} disabled={update === "checking"}>
        {update === "checking" ? "Checking…" : "Check for updates"}
      </button>
      {#if update === "none"}<span class="dim">You're up to date.</span>{/if}
      {#if update && typeof update === "object"}
        <span class="dim">v{update.latest} available —</span>
        <button class="primary" onclick={() => update && typeof update === "object" && openUrl(update.url)}>View release</button>
      {/if}
    </div>
  </section>

  <section>
    <h3>Privacy</h3>
    <p class="dim">
      PugDock has no backend. Your workspace is stored locally and synced to your private
      GitHub repo. AI features use your Anthropic API key only when enabled.
    </p>
  </section>

  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  h3 {
    margin: 0 0 2px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-dim);
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    font-size: 12.5px;
  }
  .row > span:first-child {
    color: var(--text-dim);
    flex-shrink: 0;
  }
  code {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .btns {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .dim {
    color: var(--text-dim);
    font-size: 12px;
    margin: 0;
    line-height: 1.5;
  }
  .error {
    color: var(--danger);
    font-size: 12px;
  }
  select,
  input:not([type="checkbox"]) {
    max-width: 220px;
  }
</style>
