<script lang="ts">
  import { api, errorMessage, type DeviceCode, type GithubUser, type CreatedRepo } from "$lib/api";
  import { app, saveSettings, settings } from "$lib/state.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { documentDir, join } from "@tauri-apps/api/path";

  let { onDone }: { onDone: () => void } = $props();

  let step = $state(1);
  let error = $state("");
  let localOnly = $state(false);

  function skipGithub() {
    localOnly = true;
    error = "";
    step = 3;
    defaultFolder();
  }

  // --- Step 1: GitHub login (browser OAuth, device flow as fallback) ---
  let device = $state<DeviceCode | null>(null);
  let authState = $state<"idle" | "waiting" | "browser" | "authorized" | "failed" | "expired">("idle");
  let copied = $state(false);

  async function startGithub() {
    error = "";
    device = null;
    try {
      const mode = await api.githubAuthMode();
      if (mode === "unconfigured") {
        error = "No GitHub app configured. Set PUGDOCK_GITHUB_CLIENT_ID (see README).";
        return;
      }
      if (mode === "browser") {
        authState = "browser";
        await api.githubOauthStart();
        authState = "authorized";
        await loadAccounts();
        step = 2;
        return;
      }
      device = await api.githubDeviceStart();
      authState = "waiting";
      openUrl(device.verification_uri).catch(() => {});
      poll();
    } catch (e) {
      error = errorMessage(e);
      authState = "failed";
    }
  }

  async function poll() {
    if (!device) return;
    let interval = device.interval * 1000;
    const deadline = Date.now() + device.expires_in * 1000;
    while (authState === "waiting") {
      if (Date.now() > deadline) {
        authState = "expired";
        return;
      }
      await new Promise((r) => setTimeout(r, interval));
      try {
        const status = await api.githubDevicePoll(device.device_code);
        if (status === "ok") {
          authState = "authorized";
          await loadAccounts();
          step = 2;
        } else if (status === "slow_down") {
          interval += 5000;
        } else if (status === "expired" || status === "denied") {
          authState = status === "expired" ? "expired" : "failed";
        }
      } catch (e) {
        error = errorMessage(e);
        authState = "failed";
      }
    }
  }

  function copyCode() {
    if (!device) return;
    navigator.clipboard.writeText(device.user_code);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  // --- Step 2: repo owner + name ---
  let user = $state<GithubUser | null>(null);
  let orgs = $state<string[]>([]);
  let owner = $state("");
  let repoName = $state("PugDockNotes");
  let creating = $state(false);
  let existingRepo = $state(false);
  let repo = $state<CreatedRepo | null>(null);

  async function loadAccounts() {
    user = await api.githubUser();
    owner = user.login;
    orgs = (await api.githubOrgs().catch(() => [])).map((o) => o.login);
  }

  async function createRepo() {
    if (!user) return;
    error = "";
    creating = true;
    try {
      const name = repoName.trim();
      if (await api.githubRepoExists(owner, name)) {
        // The repo already exists (probably created by PugDock on another
        // device). Reuse it: that is how notes sync across devices.
        existingRepo = true;
        repo = {
          full_name: `${owner}/${name}`,
          clone_url: `https://github.com/${owner}/${name}.git`,
          html_url: `https://github.com/${owner}/${name}`,
          private: true,
        };
        step = 3;
        defaultFolder();
        return;
      }
      repo = await api.githubCreateRepo(owner, name, owner !== user.login);
      step = 3;
      defaultFolder();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      creating = false;
    }
  }

  // --- Step 3: local folder ---
  let folder = $state("");
  let folderWarning = $state("");
  let settingUp = $state(false);
  let setupDone = $state(false);

  async function defaultFolder() {
    try {
      // When reconnecting GitHub, always link the SYNC ROOT, never a
      // sub-workspace: the whole tree syncs as one.
      const root = await api.syncRoot().catch(() => null);
      folder = root ?? app.config?.workspace_path ?? (await join(await documentDir(), "PugDock"));
      await inspect();
    } catch {
      folder = "";
    }
  }

  async function chooseFolder() {
    const picked = await openDialog({ directory: true, title: "Choose PugDock folder" });
    if (typeof picked === "string") {
      folder = await join(picked, "PugDock");
      await inspect();
    }
  }

  async function inspect() {
    folderWarning = "";
    try {
      const info = await api.inspectFolder(folder);
      if (info.is_git_repo) {
        folderWarning = "This folder is already a Git repository. PugDock will reuse it and connect it to your new workspace repo.";
      } else if (info.exists && !info.is_empty) {
        folderWarning = "This folder is not empty. Existing files will become part of your workspace and sync to GitHub.";
      }
    } catch {
      /* folder may not exist yet - fine */
    }
  }

  async function setUpWorkspace() {
    if (!localOnly && (!repo || !user)) return;
    error = "";
    settingUp = true;
    try {
      await api.createWorkspace(folder);
      if (localOnly) {
        // Local checkpoints still work without GitHub; ignore if git is missing.
        await api.gitInitWorkspace(null, "PugDock", "pugdock@local").catch(() => {});
        app.config = await api.getConfig();
      } else if (repo && user) {
        await api.gitInitWorkspace(
          repo.clone_url,
          user.name ?? user.login,
          `${user.id}+${user.login}@users.noreply.github.com`,
        );
        const cfg = await api.getConfig();
        cfg.repo_owner = owner;
        cfg.repo_name = repoName.trim();
        cfg.settings = { ...cfg.settings, repoHtmlUrl: repo.html_url, githubLogin: user.login };
        await api.setConfig(cfg);
        app.config = cfg;
      }
      api.rebuildIndex().catch(() => {});
      setupDone = true;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      settingUp = false;
    }
  }

  // --- Step 4: optional AI ---
  let connectingAi = $state(false);
  let aiStep = $state<string | null>(null);
  let anthropicAuth = $state<"claude" | "key" | "oauth" | "ant" | "none">("none");

  $effect(() => {
    if (step === 4) {
      api.anthropicAuthStatus().then((s) => {
        anthropicAuth = s;
        if (s === "none") {
          // Pre-warm the one-time CLI setup while the user reads the screen.
          api.anthropicInstallCli().then(() => (anthropicAuth = "ant")).catch(() => {});
        }
      });
    }
  });

  async function anthropicOauth() {
    error = "";
    connectingAi = true;
    try {
      if (anthropicAuth === "claude" || anthropicAuth === "key") {
        await saveSettings({ aiEnabled: true, model: "auto" });
        finish();
        return;
      }
      if (anthropicAuth === "none") {
        aiStep = "Setting up (one time)…";
        await api.anthropicInstallCli();
        anthropicAuth = "ant";
      }
      aiStep = "Waiting for browser sign-in…";
      if (anthropicAuth !== "oauth") {
        await api.anthropicOauthLogin();
      }
      await saveSettings({ aiEnabled: true, model: "auto" });
      finish();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      connectingAi = false;
      aiStep = null;
    }
  }

  async function finish() {
    const cfg = await api.getConfig();
    cfg.onboarding_done = true;
    await api.setConfig(cfg);
    app.config = cfg;
    onDone();
  }
</script>

<div class="onboarding">
  <div class="card">
    {#if step === 1}
      <h1>Welcome to PugDock</h1>
      <p>
        PugDock keeps your developer workspace synced in a private GitHub repository.
        No PugDock account. No PugDock servers. Your files stay local and in your GitHub.
      </p>
      {#if authState === "idle" || authState === "failed" || authState === "expired"}
        {#if authState === "expired"}
          <p class="warn">The code expired. Start again.</p>
        {:else if authState === "failed"}
          <p class="warn">Authorization was not completed. Try again.</p>
        {/if}
        <button class="github-btn" onclick={startGithub}>
          <svg viewBox="0 0 16 16" width="20" height="20" aria-hidden="true">
            <path
              fill="currentColor"
              d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"
            />
          </svg>
          Sign in with GitHub
        </button>
        <p class="dim">
          PugDock asks for permission to create and sync one private repository.
          It never touches your other repos' content.
        </p>
        <button class="ghost" onclick={skipGithub}>Skip and use PugDock locally, without sync</button>
      {:else if authState === "browser"}
        <p>Finish signing in with GitHub in your browser…</p>
        <p class="dim">PugDock will continue automatically once you authorize.</p>
        <button class="ghost" onclick={() => (authState = "idle")}>Cancel</button>
      {:else if device}
        <p>Enter this code at <strong>{device.verification_uri}</strong>:</p>
        <div class="code-row">
          <span class="user-code">{device.user_code}</span>
          <button onclick={copyCode}>{copied ? "Copied" : "Copy"}</button>
          <button onclick={() => device && openUrl(device.verification_uri)}>Open browser</button>
        </div>
        <p class="dim">Waiting for authorization…</p>
      {/if}
    {:else if step === 2}
      <h1>Create your private workspace</h1>
      <p class="dim">PugDock will create a private GitHub repository to sync your files.</p>
      <label>
        Owner
        <select bind:value={owner}>
          {#if user}<option value={user.login}>{user.login} (you)</option>{/if}
          {#each orgs as org (org)}
            <option value={org}>{org}</option>
          {/each}
        </select>
      </label>
      <label>
        Workspace name
        <input bind:value={repoName} spellcheck="false" />
      </label>
      <label>
        Visibility
        <input value="Private" disabled />
      </label>
      <button class="primary" onclick={createRepo} disabled={creating || !repoName.trim()}>
        {creating ? "Creating…" : "Create private workspace"}
      </button>
    {:else if step === 3}
      <h1>Where should PugDock store files on this computer?</h1>
      <div class="code-row">
        <input bind:value={folder} onchange={inspect} spellcheck="false" style="flex:1" />
        <button onclick={chooseFolder}>Choose folder</button>
      </div>
      {#if existingRepo && repo}
        <p class="dim">
          Using your existing {repo.full_name} repository. Your notes from other
          devices will sync into this folder.
        </p>
      {/if}
      {#if folderWarning}<p class="warn">{folderWarning}</p>{/if}
      {#if setupDone}
        <div class="summary">
          <div><span class="dim">Local folder</span> <code>{folder}</code></div>
          {#if repo}
            <div><span class="dim">GitHub repo</span> <code>{repo.full_name}</code> (private)</div>
            <div><span class="dim">Sync</span> Enabled</div>
          {:else}
            <div><span class="dim">Sync</span> Off. Connect GitHub anytime in Settings</div>
          {/if}
        </div>
        <button class="primary" onclick={() => (settings().aiEnabled ? finish() : (step = 4))}>
          Continue
        </button>
      {:else}
        <button class="primary" onclick={setUpWorkspace} disabled={settingUp || !folder}>
          {settingUp ? "Setting up workspace…" : "Use this folder"}
        </button>
      {/if}
    {:else if step === 4}
      <h1>Enable AI features?</h1>
      <p>
        PugDock works without AI. If you connect Anthropic, PugDock can organize, label,
        summarize and enrich your workspace using your own Anthropic API key.
      </p>
      <button class="primary" onclick={anthropicOauth} disabled={connectingAi}>
        {aiStep ?? (anthropicAuth === "claude" ? "Enable AI using your Claude Code sign-in" : "Sign in with Anthropic")}
      </button>
      <p class="dim">
        {anthropicAuth === "claude"
          ? "Claude Code is installed and signed in with your Anthropic account. Nothing else to set up."
          : "Opens your browser to sign in with your Anthropic account, nothing to copy. PugDock sets up what it needs automatically."}
      </p>
      <div class="code-row">
        <button onclick={finish} disabled={connectingAi}>Skip for now</button>
      </div>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  </div>
</div>

<style>
  .onboarding {
    height: 100vh;
    display: grid;
    place-items: center;
  }
  .card {
    width: 460px;
    max-width: 90vw;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 32px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
  }
  p {
    margin: 0;
    line-height: 1.5;
  }
  .dim {
    color: var(--text-dim);
    font-size: 12px;
  }
  .warn {
    color: var(--warn);
    font-size: 12px;
  }
  .error {
    color: var(--danger);
    font-size: 12px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--text-dim);
    font-size: 12px;
  }
  .code-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .github-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 10px 16px;
    font-size: 14px;
    font-weight: 600;
    color: #ffffff;
    background: #24292f;
    border: 1px solid rgba(240, 246, 252, 0.1);
    border-radius: 6px;
  }
  .github-btn:hover {
    background: #32383f;
  }
  .user-code {
    font-family: "SF Mono", Menlo, monospace;
    font-size: 22px;
    letter-spacing: 3px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 14px;
  }
  .summary {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
