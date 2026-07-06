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

/** Compact palette spec — expanded into vars + token colors by mk(). */
interface Palette {
  id: string;
  name: string;
  dark: boolean;
  bg: string;
  panel: string;
  hover: string;
  active: string;
  border: string;
  text: string;
  dim: string;
  accent: string;
  danger: string;
  ok: string;
  warn: string;
  /** [comment, string, number, keyword, function, type, tag] */
  t: [string, string, string, string, string, string, string];
}

function mk(p: Palette): ResolvedTheme {
  const [comment, str, num, kw, fn, type, tag] = p.t;
  const pairs: [string, string, string?][] = [
    ["comment", comment, "italic"],
    ["string", str],
    ["constant.numeric", num],
    ["constant.language", num],
    ["keyword", kw],
    ["storage", kw],
    ["entity.name.function", fn],
    ["support.function", fn],
    ["entity.name.type", type],
    ["support.type.property-name", type],
    ["entity.name.tag", tag],
    ["entity.other.attribute-name", num],
    ["variable", p.text],
    ["markup.heading", p.accent, "bold"],
    ["markup.bold", p.text, "bold"],
    ["markup.italic", p.text, "italic"],
    ["markup.underline.link", p.accent],
  ];
  return {
    id: `builtin:${p.id}`,
    name: p.name,
    dark: p.dark,
    vars: {
      "--bg": p.bg,
      "--bg-panel": p.panel,
      "--bg-hover": p.hover,
      "--bg-active": p.active,
      "--border": p.border,
      "--text": p.text,
      "--text-dim": p.dim,
      "--accent": p.accent,
      "--danger": p.danger,
      "--ok": p.ok,
      "--warn": p.warn,
      "--ed-selection": p.active,
      "--ed-cursor": p.accent,
      "--ed-gutter-fg": p.dim,
    },
    tokenColors: pairs.map(([scope, foreground, fontStyle]) => ({ scope, settings: { foreground, fontStyle } })),
  };
}

const dk = { danger: "#f7768e", ok: "#9ece6a", warn: "#e0af68" };
const lt = { danger: "#cf222e", ok: "#1a7f37", warn: "#9a6700" };

// prettier-ignore
export const BUILTIN_THEMES: ResolvedTheme[] = [
  // ---- Standard ----
  mk({ id: "dark", name: "PugDock Dark", dark: true, bg: "#16181d", panel: "#1c1f26", hover: "#262a33", active: "#2e3440", border: "#2a2e37", text: "#d8dce4", dim: "#8b93a1", accent: "#7aa2f7", ...dk,
    t: ["#636b78", "#9ece6a", "#ff9e64", "#bb9af7", "#7aa2f7", "#2ac3de", "#f7768e"] }),
  mk({ id: "light", name: "PugDock Light", dark: false, bg: "#ffffff", panel: "#f4f5f7", hover: "#e9ebef", active: "#dde1e8", border: "#d5d9e0", text: "#24292f", dim: "#6e7781", accent: "#2563eb", ...lt,
    t: ["#6e7781", "#0a3069", "#953800", "#cf222e", "#8250df", "#0550ae", "#116329"] }),
  mk({ id: "midnight", name: "Midnight", dark: true, bg: "#0d1117", panel: "#11151c", hover: "#1c2128", active: "#262c36", border: "#21262d", text: "#c9d1d9", dim: "#768390", accent: "#58a6ff", ...dk,
    t: ["#6a737d", "#a5d6a7", "#f0883e", "#ff7b72", "#d2a8ff", "#79c0ff", "#7ee787"] }),
  mk({ id: "graphite", name: "Graphite", dark: true, bg: "#1a1a1a", panel: "#212121", hover: "#2a2a2a", active: "#333333", border: "#2e2e2e", text: "#d4d4d4", dim: "#8a8a8a", accent: "#6cb6ff", ...dk,
    t: ["#7f7f7f", "#b5cea8", "#d7ba7d", "#c586c0", "#dcdcaa", "#4ec9b0", "#e06c75"] }),
  mk({ id: "ocean-dark", name: "Ocean Dark", dark: true, bg: "#282c34", panel: "#21252b", hover: "#2c313a", active: "#3a3f4b", border: "#3b4048", text: "#abb2bf", dim: "#7f848e", accent: "#61afef", ...dk,
    t: ["#5c6370", "#98c379", "#d19a66", "#c678dd", "#61afef", "#56b6c2", "#e06c75"] }),
  mk({ id: "deep-sea", name: "Deep Sea", dark: true, bg: "#002b36", panel: "#073642", hover: "#0a4552", active: "#0e5261", border: "#0a4552", text: "#93a1a1", dim: "#586e75", accent: "#2aa198", ...dk,
    t: ["#586e75", "#859900", "#d33682", "#cb4b16", "#268bd2", "#b58900", "#dc322f"] }),
  mk({ id: "solar-dawn", name: "Solar Dawn", dark: false, bg: "#fdf6e3", panel: "#eee8d5", hover: "#e6dfc8", active: "#dcd4bc", border: "#d9d2bd", text: "#586e75", dim: "#93a1a1", accent: "#268bd2", ...lt,
    t: ["#93a1a1", "#859900", "#d33682", "#cb4b16", "#268bd2", "#b58900", "#dc322f"] }),
  mk({ id: "paper", name: "Paper", dark: false, bg: "#fbfaf8", panel: "#f3f1ec", hover: "#ebe8e1", active: "#e2ded4", border: "#e0dcd2", text: "#3d3a34", dim: "#8a857a", accent: "#4a7a63", ...lt,
    t: ["#9a948a", "#5a7d43", "#b06c3b", "#8a4a5e", "#4a7a63", "#3f6d8e", "#a04b3f"] }),

  // ---- Neon ----
  mk({ id: "neon-city", name: "Neon City", dark: true, bg: "#0a0a12", panel: "#0f0f1a", hover: "#181828", active: "#222238", border: "#232338", text: "#e0e6ff", dim: "#6f76a0", accent: "#00f0ff", ...dk,
    t: ["#4a5178", "#39ff88", "#ffe14d", "#ff2ec4", "#00f0ff", "#8f7bff", "#ff5370"] }),
  mk({ id: "synthwave", name: "Synthwave", dark: true, bg: "#241b2f", panel: "#2a2139", hover: "#342a46", active: "#413555", border: "#3b2f4d", text: "#f0eff1", dim: "#8d84a5", accent: "#ff7edb", ...dk,
    t: ["#6f6685", "#72f1b8", "#f97e72", "#fede5d", "#36f9f6", "#ff8b39", "#fe4450"] }),
  mk({ id: "matrix", name: "Matrix", dark: true, bg: "#030a03", panel: "#071207", hover: "#0c1d0c", active: "#122a12", border: "#0f240f", text: "#00ff41", dim: "#0a7a2a", accent: "#00ff41", danger: "#ff4141", ok: "#00ff41", warn: "#a8ff00",
    t: ["#0a6a24", "#7fff00", "#c8ff5e", "#00e53a", "#5eff8f", "#38d95e", "#baffc9"] }),
  mk({ id: "laser-grid", name: "Laser Grid", dark: true, bg: "#05010f", panel: "#0a0518", hover: "#140c28", active: "#1f143c", border: "#1a1030", text: "#dcd6ff", dim: "#6b5f96", accent: "#4d7cff", ...dk,
    t: ["#4a3f70", "#00e5a0", "#ffd166", "#b967ff", "#4d7cff", "#38d1ff", "#ff4d8f"] }),
  mk({ id: "vaporwave", name: "Vaporwave", dark: true, bg: "#1a1030", panel: "#211540", hover: "#2b1c52", active: "#372465", border: "#2e1e56", text: "#f2e9ff", dim: "#8d7bb5", accent: "#ff71ce", ...dk,
    t: ["#6d5c96", "#05ffa1", "#fffb96", "#ff71ce", "#01cdfe", "#b967ff", "#ff6b9d"] }),

  // ---- Pastel ----
  mk({ id: "pastel-dream", name: "Pastel Dream", dark: false, bg: "#fdf7ff", panel: "#f6ecfa", hover: "#efe1f5", active: "#e6d3ee", border: "#e8dbef", text: "#4a3d52", dim: "#9a89a5", accent: "#b57edc", ...lt,
    t: ["#a598ad", "#77c3a4", "#e6a57e", "#d291bc", "#9a86d9", "#7fa7cc", "#d98a9c"] }),
  mk({ id: "mint-breeze", name: "Mint Breeze", dark: false, bg: "#f4fbf7", panel: "#e8f5ec", hover: "#dcefe3", active: "#cde6d7", border: "#d7ebde", text: "#31473a", dim: "#7d998a", accent: "#3eb489", ...lt,
    t: ["#8fa89b", "#2e8b57", "#c98a4b", "#5b8dbf", "#3eb489", "#4a9d9c", "#c46a76"] }),
  mk({ id: "peach-sorbet", name: "Peach Sorbet", dark: false, bg: "#fff7f2", panel: "#ffece2", hover: "#fbe0d1", active: "#f5d2be", border: "#f2ddd0", text: "#54382a", dim: "#a58573", accent: "#ff9166", ...lt,
    t: ["#b39485", "#7aa05c", "#d98e3a", "#e0709a", "#e07b4f", "#c46a4a", "#b8556f"] }),
  mk({ id: "lavender-mist", name: "Lavender Mist", dark: false, bg: "#f7f5ff", panel: "#edeafd", hover: "#e2ddf8", active: "#d4cdf2", border: "#ddd7f0", text: "#3f3a5c", dim: "#8d87ad", accent: "#8a7cf0", ...lt,
    t: ["#9d97b8", "#6aa88a", "#d09355", "#8a7cf0", "#6f8fd9", "#5ba3b8", "#c76a8e"] }),

  // ---- Cozy ----
  mk({ id: "cozy-cabin", name: "Cozy Cabin", dark: true, bg: "#241c17", panel: "#2b221b", hover: "#362b21", active: "#433529", border: "#3a2e23", text: "#e8dcc8", dim: "#a08e75", accent: "#d9a05b", ...dk,
    t: ["#8a7860", "#a8c080", "#e0a870", "#cf8d6d", "#d9a05b", "#b8a06a", "#d97e6a"] }),
  mk({ id: "espresso", name: "Espresso", dark: true, bg: "#1f1712", panel: "#261c15", hover: "#31251b", active: "#3e2f22", border: "#342820", text: "#dccbb8", dim: "#96826d", accent: "#b98a5e", ...dk,
    t: ["#7d6a56", "#9cb37a", "#d49a5e", "#c07a56", "#b98a5e", "#a89468", "#c66a5a"] }),
  mk({ id: "autumn-glow", name: "Autumn Glow", dark: true, bg: "#26180e", panel: "#2e1e12", hover: "#3a2717", active: "#4a321d", border: "#3e2a18", text: "#f0dcc0", dim: "#ab8f6c", accent: "#e8833a", ...dk,
    t: ["#8f7454", "#9caf5f", "#e8a83a", "#d1603d", "#e8833a", "#c09550", "#cc5f45"] }),
  mk({ id: "nordic-fog", name: "Nordic Fog", dark: true, bg: "#2e3440", panel: "#353c4a", hover: "#3f4758", active: "#4c566a", border: "#434c5e", text: "#d8dee9", dim: "#8792a8", accent: "#88c0d0", ...dk,
    t: ["#616e88", "#a3be8c", "#b48ead", "#81a1c1", "#88c0d0", "#8fbcbb", "#bf616a"] }),
  mk({ id: "rosewood", name: "Rosewood", dark: true, bg: "#2a1e22", panel: "#32242a", hover: "#3e2c34", active: "#4c3640", border: "#423038", text: "#ecdae0", dim: "#a98995", accent: "#e08e9b", ...dk,
    t: ["#8c6f7a", "#a4c28a", "#e0a878", "#c97a94", "#e08e9b", "#b892c0", "#d96a78"] }),
];

// ---- VSCode workbench colors → our CSS variables ----

function pick(colors: Record<string, string>, keys: string[], fallback: string): string {
  for (const k of keys) {
    if (colors[k]) return colors[k];
  }
  return fallback;
}

export function vscodeToVars(colors: Record<string, string>, dark: boolean): Record<string, string> {
  // Fallback base for imported themes that omit workbench colors.
  const base = (dark ? BUILTIN_THEMES[0] : BUILTIN_THEMES[1]).vars;
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
