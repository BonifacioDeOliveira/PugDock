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

Create a GitHub OAuth App at <https://github.com/settings/applications/new>:

- **Authorization callback URL**: `http://127.0.0.1/callback`
  (the port is ignored by GitHub for loopback URLs — PugDock picks a free one at runtime)
- Also check **Enable Device Flow** (used as fallback).
- Scopes requested at runtime: `repo read:org`.

PugDock supports two OAuth flows, chosen automatically:

| Env vars set | Flow |
|---|---|
| `PUGDOCK_GITHUB_CLIENT_ID` + `PUGDOCK_GITHUB_CLIENT_SECRET` | **Browser flow** — opens GitHub, redirects back to a loopback listener; no code typing |
| only `PUGDOCK_GITHUB_CLIENT_ID` | **Device flow** — user enters a short code on github.com |

```sh
PUGDOCK_GITHUB_CLIENT_ID=Ov23li... PUGDOCK_GITHUB_CLIENT_SECRET=... npm run tauri dev
```

Both vars are read at runtime (dev) and compile time (release builds embed them).
Note: GitHub doesn't support PKCE, so the browser flow requires embedding the
client secret — same trade-off GitHub Desktop makes. The secret only identifies
the OAuth app; user tokens stay in the OS keychain. If you prefer not to embed
it, ship with only the client id and PugDock uses the device flow.

Without any of it, the app boots but "Continue with GitHub" explains that the
build has no GitHub app configured.

## Releases and updates (CI/CD)

Releases are fully automated by `.github/workflows/release.yml`:

1. Bump `version` in `src-tauri/tauri.conf.json` (and `package.json`), commit.
2. Tag and push:

   ```sh
   git tag v0.2.0 && git push origin main --tags
   ```

3. GitHub Actions builds **macOS (Apple Silicon + Intel)** and **Ubuntu Linux**
   (`.dmg`/`.app`, `.AppImage`, `.deb`), signs the updater artifacts, and
   publishes everything — including the `latest.json` update manifest — to the
   tag's **GitHub Release**.

Installed apps check for updates on startup (and via *Settings → Check for
updates*). When a new version exists, the in-app dialog shows the release notes
and **Update now** downloads, verifies the signature, installs, and relaunches —
one click, never silent. Dev builds without updater artifacts fall back to a
"View release" link.

### One-time repository setup

Add these **Actions secrets** (repo → Settings → Secrets → Actions):

| Secret | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.pugdock-updater.key` (generated with `npx tauri signer generate`; **never commit it** — losing it means future updates can't be signed) |
| `PUGDOCK_GITHUB_CLIENT_ID` | Your GitHub OAuth app client id |
| `PUGDOCK_GITHUB_CLIENT_SECRET` | Your GitHub OAuth app client secret |

The matching public key is committed in `tauri.conf.json > plugins > updater`.
The updater endpoint points at this repo's releases
(`.../releases/latest/download/latest.json`) — the repo (or at least its
releases) must be publicly accessible for installed apps to see updates.

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
