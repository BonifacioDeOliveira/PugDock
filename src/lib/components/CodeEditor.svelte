<script lang="ts">
  import { untrack } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState, Compartment } from "@codemirror/state";
  import { languages } from "@codemirror/language-data";
  import { scheduleSave } from "$lib/sync";
  import { api } from "$lib/api";
  import { refreshTree, toast, isNoteFile } from "$lib/state.svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { themeState, editorExtensions } from "$lib/theme.svelte";
  import type { Tab } from "$lib/state.svelte";

  let { tab }: { tab: Tab } = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  const langCompartment = new Compartment();
  const themeCompartment = new Compartment();

  const isMd = $derived(isNoteFile(tab.path));

  /** Wrap the selection (or insert placeholder) with before/after markers. */
  function wrap(before: string, after = before, placeholder = "") {
    if (!view) return;
    const { from, to } = view.state.selection.main;
    const sel = view.state.sliceDoc(from, to) || placeholder;
    view.dispatch({
      changes: { from, to, insert: `${before}${sel}${after}` },
      selection: { anchor: from + before.length, head: from + before.length + sel.length },
    });
    view.focus();
  }

  /** Insert a block on its own line(s) at the cursor. */
  function insertBlock(text: string, cursorOffset?: number) {
    if (!view) return;
    const { from } = view.state.selection.main;
    const line = view.state.doc.lineAt(from);
    const prefix = line.length > 0 ? "\n" : "";
    const insert = `${prefix}${text}\n`;
    view.dispatch({
      changes: { from: line.to, to: line.to, insert },
      selection: { anchor: line.to + prefix.length + (cursorOffset ?? text.length) },
    });
    view.focus();
  }

  async function insertImage() {
    const picked = await openDialog({
      title: "Insert image",
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg"] }],
    });
    if (typeof picked !== "string") return;
    const name = picked.split(/[/\\]/).pop() ?? "image.png";
    try {
      await api.importFile(picked, `attachments/${name}`);
      await refreshTree();
      insertBlock(`![${name}](attachments/${name})`);
    } catch {
      toast("Could not import the image.");
    }
  }

  const TABLE = `| Column 1 | Column 2 |\n| --- | --- |\n|  |  |`;

  const sensitive = $derived(
    /^\.env($|\.)(?!.*example)|\.(pem|key)$|^id_rsa|^id_ed25519|^credentials\.|^secrets\./.test(tab.name),
  );

  async function langFor(name: string) {
    // extensionless notes are markdown
    const ext = name.includes(".") ? (name.split(".").pop()?.toLowerCase() ?? "") : "md";
    const desc =
      languages.find((l) => l.extensions.includes(ext)) ??
      languages.find((l) => l.alias.includes(ext));
    return desc ? await desc.load() : null;
  }

  $effect(() => {
    const path = tab.path; // track: rebuild editor per file
    // untrack: the editor OWNS the content while open. Tracking it would
    // rebuild the editor (and lose cursor/focus) on every keystroke.
    const initial = untrack(() => tab.content);
    const state = EditorState.create({
      doc: initial,
      extensions: [
        basicSetup,
        themeCompartment.of(editorExtensions(themeState.current)),
        EditorView.lineWrapping,
        EditorState.readOnly.of(sensitive),
        langCompartment.of([]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            tab.content = u.state.doc.toString();
            tab.dirty = true;
            scheduleSave(path, () => u.state.doc.toString());
          }
        }),
        EditorView.theme({
          "&": { height: "100%", fontSize: "13px" },
          ".cm-scroller": { fontFamily: '"SF Mono", Menlo, Consolas, monospace' },
        }),
      ],
    });
    view = new EditorView({ state, parent: host });
    langFor(tab.name).then((lang) => {
      if (lang && view) view.dispatch({ effects: langCompartment.reconfigure(lang) });
    });
    return () => {
      view?.destroy();
      view = null;
    };
  });

  // Live mirror: when the same document is open in another group (or an
  // external rewrite lands), reflect tab.content into this editor. The
  // editor that produced the change is a no-op (doc already matches).
  $effect(() => {
    const content = tab.content;
    if (view && view.state.doc.toString() !== content) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: content } });
    }
  });

  // Live theme switching for already-open editors.
  $effect(() => {
    const theme = themeState.current;
    view?.dispatch({ effects: themeCompartment.reconfigure(editorExtensions(theme)) });
  });
</script>

{#if sensitive}
  <div class="banner">This file looks like it contains secrets. It is opened read-only and is never synced or sent to AI.</div>
{/if}
{#if isMd && !sensitive}
  <div class="md-toolbar">
    <button class="ghost" data-tip="Heading" onclick={() => insertBlock("## Heading", 3)}>H</button>
    <button class="ghost b" data-tip="Bold" onclick={() => wrap("**", "**", "bold")}>B</button>
    <button class="ghost i" data-tip="Italic" onclick={() => wrap("*", "*", "italic")}>I</button>
    <span class="sep"></span>
    <button class="ghost mono" data-tip="Inline code" onclick={() => wrap("\`", "\`", "code")}>{"<>"}</button>
    <button class="ghost mono" data-tip="Code block" onclick={() => insertBlock("\`\`\`\n\n\`\`\`", 4)}>{"{ }"}</button>
    <span class="sep"></span>
    <button class="ghost" data-tip="Bulleted list" onclick={() => insertBlock("- ")}>•</button>
    <button class="ghost" data-tip="Numbered list" onclick={() => insertBlock("1. ")}>1.</button>
    <button class="ghost" data-tip="Task list" onclick={() => insertBlock("- [ ] ")}>☑</button>
    <span class="sep"></span>
    <button class="ghost" data-tip="Table" onclick={() => insertBlock(TABLE)}>▦</button>
    <button class="ghost" data-tip="Image (imports into attachments/)" onclick={insertImage}>🖼</button>
    <button class="ghost" data-tip="Link" onclick={() => wrap("[", "](url)", "text")}>🔗</button>
    <span class="sep"></span>
    <button class="ghost" data-tip="Quote" onclick={() => insertBlock("> ")}>"</button>
    <button class="ghost" data-tip="Divider" onclick={() => insertBlock("---")}>—</button>
  </div>
{/if}
<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    overflow: hidden;
  }
  .md-toolbar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
    flex: 0 0 auto !important;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .md-toolbar::-webkit-scrollbar {
    display: none;
  }
  .md-toolbar button {
    min-width: 28px;
    padding: 3px 7px;
    font-size: 12.5px;
    line-height: 1.2;
  }
  .md-toolbar .b {
    font-weight: 800;
  }
  .md-toolbar .i {
    font-style: italic;
  }
  .md-toolbar .mono {
    font-family: "SF Mono", Menlo, monospace;
    font-size: 11px;
  }
  .md-toolbar .sep {
    width: 1px;
    height: 14px;
    background: var(--border);
    margin: 0 4px;
    flex-shrink: 0;
  }
  .banner {
    background: color-mix(in srgb, var(--warn) 15%, var(--bg));
    color: var(--warn);
    padding: 6px 12px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }
</style>
