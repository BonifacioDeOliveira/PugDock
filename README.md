# PugDock 🐾

> Your developer workspace, synced privately with GitHub.

PugDock is a lightweight desktop workspace for developers to organize files, code,
PDFs, snippets, runbooks and project context in a **private GitHub repo**, with
optional Anthropic-powered AI.

**Powerful, but invisible.** PugDock uses Git, GitHub and a local search index under
the hood — the user only sees "Saved", "Syncing", "Synced" and a clean workspace.
No PugDock account. No PugDock servers. No CLI. Local-first, free.

## How it works

1. Open PugDock → **Continue with GitHub** (device flow, token stored in the OS keychain).
2. Choose the owner (you or an org) and a name → PugDock creates a **private** repo.
3. Choose a local folder → PugDock scaffolds the workspace structure and links it to the repo.
4. Write, drop in files and PDFs, search — everything autosaves locally, checkpoints
   after ~60s of idle, and pushes to GitHub every few minutes and on exit.
5. Optionally connect Anthropic (your own API key) for Organize, Enrich, Summarize,
   Explain error, Build context and Ask PugDock.

## Stack

| Layer | Tech |
|---|---|
| Desktop shell | Tauri v2 |
| Core | Rust |
| UI | Svelte 5 + TypeScript (SvelteKit static, SPA mode) |
| Editor | CodeMirror 6 |
| PDF | PDF.js |
| Search | SQLite + FTS5 (rusqlite, bundled) |
| Sync | system `git` via subprocess + GitHub REST API |
| Secrets | OS keychain (`keyring` crate) |
| AI | Anthropic API, BYOK |

## Architecture

```
src/                          # Svelte frontend
  routes/+page.svelte         # root: onboarding vs workspace
  lib/api.ts                  # typed wrappers for every Tauri command
  lib/state.svelte.ts         # global reactive state (runes)
  lib/sync.ts                 # autosave debounce → idle checkpoint → periodic push
  lib/ai.ts                   # model-tier resolution + prompt templates + AI privacy guard
  lib/components/             # Onboarding, Workspace, FileTree, CodeEditor, PdfViewer,
                              # SearchPanel, AiPanel, SettingsPanel, HistoryPanel, ConflictDialog

src-tauri/src/                # Rust core
  error.rs                    # typed errors → {code, message} for friendly UI messages
  workspace.rs                # app config, workspace scaffold, sandboxed file CRUD
  git_sync.rs                 # git subprocess: checkpoint/push/pull/conflict resolution
  search.rs                   # SQLite FTS5 index + search + AI context retrieval
  secrets.rs                  # OS keychain storage
  update.rs                   # GitHub Releases update check
  integrations/github.rs      # device flow auth + REST (user, orgs, create private repo)
  integrations/anthropic.rs   # key validation, model list, messages proxy
```

Key decisions:

- **Git via subprocess**, not libgit2: less code, robust HTTPS auth. The token is
  injected through an inline credential helper reading an **env var** — it never
  touches disk or the process argument list. macOS ships git; if missing, PugDock
  shows a friendly install message.
- **Sync model**: save to disk (600ms debounce) → `git commit` checkpoint after
  idle (default 60s) → `git push` every ~4 min / on exit / on "Sync now".
  Pull on startup with `git merge`; conflicts surface as "Needs review" with
  per-file *Keep local / Keep GitHub / Compare* — never resolved destructively.
- **AI is a thin proxy**: the Anthropic key lives only in the keychain and Rust;
  the webview never sees it. Secret-looking files (`.env`, keys, credentials) are
  read-only in the editor, git-ignored, and blocked from AI requests.
- **Search index** lives in `.pugdock/index.sqlite` (git-ignored) and is rebuilt
  automatically on startup, so a fresh clone reindexes itself.

## Development

Prereqs: Node 20+, Rust (rustup), Xcode CLT (macOS).

```sh
npm install
npm run tauri dev      # run the app
npm run check          # svelte-check (TS strict)
cargo test             # Rust unit tests (run in src-tauri/)
npm run tauri build    # production bundle
```

### GitHub OAuth app (required for login)

PugDock uses the GitHub **Device Flow**. Create a GitHub OAuth App
(Settings → Developer settings → OAuth Apps):

- Enable **Device Flow**.
- No callback URL is needed for device flow (any placeholder works).
- Scopes requested at runtime: `repo read:org`.

Then build/run with the client id (it is public, safe to embed):

```sh
PUGDOCK_GITHUB_CLIENT_ID=Ov23li... npm run tauri dev
```

Without it, the app boots but "Continue with GitHub" explains that the build has
no GitHub app configured.

## Releases and updates

The in-app update checker reads **GitHub Releases** of the repo set at build time:

```sh
PUGDOCK_UPDATE_REPO=youruser/pugdock npm run tauri build
```

On startup (and via *Settings → Check for updates*) PugDock compares the latest
release tag (e.g. `v0.2.0`) with the app version, and shows a dialog with release
notes and a link — it never updates silently.

### Upgrading to the full Tauri updater (TODO)

The current checker links to the release page. To enable in-place updates with
`tauri-plugin-updater`, you need:

1. `npm run tauri signer generate` → keep the private key out of the repo,
   put the public key in `tauri.conf.json > plugins > updater > pubkey`.
2. Add `tauri-plugin-updater` (Rust + capability) and set `createUpdaterArtifacts: true`
   in the bundle config.
3. In CI, sign artifacts with the private key (`TAURI_SIGNING_PRIVATE_KEY`) and attach
   the generated `latest.json` manifest to each GitHub Release.
4. Point the updater endpoint at
   `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`.

The UI (dialog with *Update now / Later / release notes*) is already in place and
only needs the install action swapped in.

## Privacy

- Your files live in your local folder and your **private** GitHub repo. Nothing else.
- GitHub token and Anthropic key are stored in the OS keychain, never in files.
- `.pugdock/` (index, cache), `.env`, keys and credentials are git-ignored by default.
- AI requests happen only when you trigger them, only with your key, and never include
  secret-pattern files or paths you exclude (*Settings → AI → Excluded from AI*).

## MVP limitations

- Conflict resolution is per-file (keep local / keep GitHub), not line-level merge.
- "Update now" opens the GitHub release page (see updater TODO above).
- PDF search indexing happens after a PDF is opened/summarized (text extraction runs
  in the frontend via PDF.js).
- Single workspace per app instance.
