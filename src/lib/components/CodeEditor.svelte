<script lang="ts">
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState, Compartment } from "@codemirror/state";
  import { languages } from "@codemirror/language-data";
  import { scheduleSave } from "$lib/sync";
  import { themeState, editorExtensions } from "$lib/theme.svelte";
  import type { Tab } from "$lib/state.svelte";

  let { tab }: { tab: Tab } = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  const langCompartment = new Compartment();
  const themeCompartment = new Compartment();

  const sensitive = $derived(
    /^\.env($|\.)(?!.*example)|\.(pem|key)$|^id_rsa|^id_ed25519|^credentials\.|^secrets\./.test(tab.name),
  );

  async function langFor(name: string) {
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    const desc =
      languages.find((l) => l.extensions.includes(ext)) ??
      languages.find((l) => l.alias.includes(ext));
    return desc ? await desc.load() : null;
  }

  $effect(() => {
    const path = tab.path; // track: rebuild editor per file
    const state = EditorState.create({
      doc: tab.content,
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

  // Live theme switching for already-open editors.
  $effect(() => {
    const theme = themeState.current;
    view?.dispatch({ effects: themeCompartment.reconfigure(editorExtensions(theme)) });
  });
</script>

{#if sensitive}
  <div class="banner">This file looks like it contains secrets. It is opened read-only and is never synced or sent to AI.</div>
{/if}
<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    overflow: hidden;
  }
  .banner {
    background: color-mix(in srgb, var(--warn) 15%, var(--bg));
    color: var(--warn);
    padding: 6px 12px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }
</style>
