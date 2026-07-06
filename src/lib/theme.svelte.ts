import { api, type ThemeMeta, type TokenColor } from "./api";
import { saveSettings } from "./state.svelte";
import { EditorView } from "codemirror";
import type { Extension } from "@codemirror/state";
import { syntaxHighlighting, HighlightStyle } from "@codemirror/language";
import { tags as t, type Tag } from "@lezer/highlight";

export interface ResolvedTheme {
  id: string;
  name: string;
  dark: boolean;
  /** our CSS variables, fully resolved */
  vars: Record<string, string>;
  tokenColors: TokenColor[];
}

// ---- Built-in themes ----

const DARK_VARS: Record<string, string> = {
  "--bg": "#16181d",
  "--bg-panel": "#1c1f26",
  "--bg-hover": "#262a33",
  "--bg-active": "#2e3440",
  "--border": "#2a2e37",
  "--text": "#d8dce4",
  "--text-dim": "#8b93a1",
  "--accent": "#7aa2f7",
  "--danger": "#f7768e",
  "--ok": "#9ece6a",
  "--warn": "#e0af68",
};

const LIGHT_VARS: Record<string, string> = {
  "--bg": "#ffffff",
  "--bg-panel": "#f4f5f7",
  "--bg-hover": "#e9ebef",
  "--bg-active": "#dde1e8",
  "--border": "#d5d9e0",
  "--text": "#24292f",
  "--text-dim": "#6e7781",
  "--accent": "#2563eb",
  "--danger": "#cf222e",
  "--ok": "#1a7f37",
  "--warn": "#9a6700",
};

const tokens = (pairs: [string, string, string?][]): TokenColor[] =>
  pairs.map(([scope, foreground, fontStyle]) => ({ scope, settings: { foreground, fontStyle } }));

export const BUILTIN_THEMES: ResolvedTheme[] = [
  {
    id: "builtin:dark",
    name: "PugDock Dark",
    dark: true,
    vars: DARK_VARS,
    tokenColors: tokens([
      ["comment", "#636b78", "italic"],
      ["string", "#9ece6a"],
      ["constant.numeric", "#ff9e64"],
      ["constant.language", "#ff9e64"],
      ["keyword", "#bb9af7"],
      ["storage", "#bb9af7"],
      ["entity.name.function", "#7aa2f7"],
      ["entity.name.type", "#2ac3de"],
      ["entity.name.tag", "#f7768e"],
      ["entity.other.attribute-name", "#e0af68"],
      ["variable", "#d8dce4"],
      ["support.type.property-name", "#7dcfff"],
      ["markup.heading", "#7aa2f7", "bold"],
      ["markup.bold", "#d8dce4", "bold"],
      ["markup.italic", "#d8dce4", "italic"],
      ["markup.underline.link", "#7dcfff"],
    ]),
  },
  {
    id: "builtin:light",
    name: "PugDock Light",
    dark: false,
    vars: LIGHT_VARS,
    tokenColors: tokens([
      ["comment", "#6e7781", "italic"],
      ["string", "#0a3069"],
      ["constant.numeric", "#953800"],
      ["constant.language", "#953800"],
      ["keyword", "#cf222e"],
      ["storage", "#cf222e"],
      ["entity.name.function", "#8250df"],
      ["entity.name.type", "#0550ae"],
      ["entity.name.tag", "#116329"],
      ["entity.other.attribute-name", "#0550ae"],
      ["variable", "#24292f"],
      ["support.type.property-name", "#0550ae"],
      ["markup.heading", "#0550ae", "bold"],
      ["markup.bold", "#24292f", "bold"],
      ["markup.italic", "#24292f", "italic"],
      ["markup.underline.link", "#0a3069"],
    ]),
  },
];

// ---- VSCode workbench colors → our CSS variables ----

function pick(colors: Record<string, string>, keys: string[], fallback: string): string {
  for (const k of keys) {
    if (colors[k]) return colors[k];
  }
  return fallback;
}

export function vscodeToVars(colors: Record<string, string>, dark: boolean): Record<string, string> {
  const base = dark ? DARK_VARS : LIGHT_VARS;
  return {
    "--bg": pick(colors, ["editor.background"], base["--bg"]),
    "--bg-panel": pick(colors, ["sideBar.background", "activityBar.background", "editor.background"], base["--bg-panel"]),
    "--bg-hover": pick(colors, ["list.hoverBackground", "editor.lineHighlightBackground"], base["--bg-hover"]),
    "--bg-active": pick(colors, ["list.activeSelectionBackground", "editor.selectionBackground"], base["--bg-active"]),
    "--border": pick(colors, ["panel.border", "editorGroup.border", "sideBar.border", "contrastBorder"], base["--border"]),
    "--text": pick(colors, ["editor.foreground", "foreground"], base["--text"]),
    "--text-dim": pick(colors, ["descriptionForeground", "sideBar.foreground", "editorLineNumber.foreground"], base["--text-dim"]),
    "--accent": pick(colors, ["textLink.foreground", "button.background", "focusBorder"], base["--accent"]),
    "--danger": pick(colors, ["errorForeground", "editorError.foreground"], base["--danger"]),
    "--ok": pick(colors, ["gitDecoration.addedResourceForeground", "terminal.ansiGreen"], base["--ok"]),
    "--warn": pick(colors, ["editorWarning.foreground", "terminal.ansiYellow"], base["--warn"]),
    // editor-specific (consumed by the CodeMirror theme, not app.css)
    "--ed-selection": pick(colors, ["editor.selectionBackground"], dark ? "#364154" : "#b6d7ff"),
    "--ed-line": pick(colors, ["editor.lineHighlightBackground"], "transparent"),
    "--ed-cursor": pick(colors, ["editorCursor.foreground"], base["--text"]),
    "--ed-gutter-fg": pick(colors, ["editorLineNumber.foreground"], base["--text-dim"]),
  };
}

// ---- TextMate scopes → Lezer highlight tags ----

const SCOPE_TAGS: [string, Tag][] = [
  ["comment", t.comment],
  ["string", t.string],
  ["constant.numeric", t.number],
  ["constant.language", t.atom],
  ["constant.character", t.literal],
  ["constant", t.atom],
  ["keyword.operator", t.operator],
  ["keyword", t.keyword],
  ["storage.type", t.keyword],
  ["storage", t.keyword],
  ["entity.name.function", t.function(t.variableName)],
  ["support.function", t.function(t.variableName)],
  ["entity.name.type", t.typeName],
  ["entity.name.class", t.className],
  ["support.type.property-name", t.propertyName],
  ["support.type", t.typeName],
  ["support.class", t.className],
  ["entity.name.tag", t.tagName],
  ["entity.other.attribute-name", t.attributeName],
  ["variable.parameter", t.variableName],
  ["variable", t.variableName],
  ["markup.heading", t.heading],
  ["markup.bold", t.strong],
  ["markup.italic", t.emphasis],
  ["markup.underline.link", t.link],
  ["markup.inserted", t.inserted],
  ["markup.deleted", t.deleted],
  ["punctuation.definition", t.punctuation],
  ["punctuation", t.punctuation],
  ["invalid", t.invalid],
  ["meta.link", t.link],
];

function highlightFromTokens(tokenColors: TokenColor[]): Extension {
  const byTag = new Map<Tag, { color?: string; fontStyle?: string }>();
  for (const entry of tokenColors ?? []) {
    if (!entry?.settings) continue;
    const scopes = typeof entry.scope === "string" ? entry.scope.split(",").map((s) => s.trim()) : (entry.scope ?? []);
    for (const scope of scopes) {
      for (const [prefix, tag] of SCOPE_TAGS) {
        if (scope === prefix || scope.startsWith(prefix + ".") || scope.startsWith(prefix + " ")) {
          byTag.set(tag, { color: entry.settings.foreground, fontStyle: entry.settings.fontStyle });
          break;
        }
      }
    }
  }
  const specs = [...byTag.entries()].map(([tag, s]) => ({
    tag,
    ...(s.color ? { color: s.color } : {}),
    ...(s.fontStyle?.includes("italic") ? { fontStyle: "italic" } : {}),
    ...(s.fontStyle?.includes("bold") ? { fontWeight: "bold" } : {}),
    ...(s.fontStyle?.includes("underline") ? { textDecoration: "underline" } : {}),
  }));
  return syntaxHighlighting(HighlightStyle.define(specs));
}

export function editorExtensions(theme: ResolvedTheme): Extension {
  const v = theme.vars;
  return [
    EditorView.theme(
      {
        "&": { backgroundColor: v["--bg"], color: v["--text"] },
        ".cm-content": { caretColor: v["--ed-cursor"] ?? v["--text"] },
        ".cm-cursor, .cm-dropCursor": { borderLeftColor: v["--ed-cursor"] ?? v["--text"] },
        "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection":
          { backgroundColor: (v["--ed-selection"] ?? v["--bg-active"]) + " !important" },
        ".cm-activeLine": { backgroundColor: v["--ed-line"] ?? "transparent" },
        ".cm-gutters": {
          backgroundColor: v["--bg"],
          color: v["--ed-gutter-fg"] ?? v["--text-dim"],
          borderRight: `1px solid ${v["--border"]}`,
        },
        ".cm-activeLineGutter": { backgroundColor: v["--bg-hover"] },
        ".cm-panels": { backgroundColor: v["--bg-panel"], color: v["--text"] },
        ".cm-searchMatch": { backgroundColor: v["--warn"] + "55" },
        ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: v["--warn"] + "99" },
        ".cm-tooltip": { backgroundColor: v["--bg-panel"], border: `1px solid ${v["--border"]}` },
      },
      { dark: theme.dark },
    ),
    highlightFromTokens(theme.tokenColors),
  ];
}

// ---- Theme state ----

export const themeState = $state({
  current: BUILTIN_THEMES[0],
  imported: [] as ThemeMeta[],
});

export async function refreshImportedThemes() {
  themeState.imported = await api.listImportedThemes().catch(() => []);
}

async function resolveTheme(id: string): Promise<ResolvedTheme> {
  const builtin = BUILTIN_THEMES.find((b) => b.id === id);
  if (builtin) return builtin;
  const raw = await api.getImportedTheme(id.replace(/^custom:/, ""));
  return {
    id,
    name: raw.name,
    dark: raw.dark,
    vars: vscodeToVars(raw.colors ?? {}, raw.dark),
    tokenColors: raw.tokenColors ?? [],
  };
}

export async function applyTheme(id: string, persist = true) {
  const theme = await resolveTheme(id).catch(() => BUILTIN_THEMES[0]);
  themeState.current = theme;
  const root = document.documentElement;
  for (const [key, value] of Object.entries(theme.vars)) {
    root.style.setProperty(key, value);
  }
  root.style.colorScheme = theme.dark ? "dark" : "light";
  if (persist) await saveSettings({ themeId: theme.id }).catch(() => {});
}
