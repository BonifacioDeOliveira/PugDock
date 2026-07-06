import { api, type Model } from "./api";
import { settings } from "./state.svelte";

export type Tier = "fast" | "balanced" | "deep";

let modelCache: Model[] | null = null;

export async function models(): Promise<Model[]> {
  if (!modelCache) modelCache = await api.anthropicModels();
  return modelCache;
}

export function clearModelCache() {
  modelCache = null;
}

/** Pick a model id for a tier from the live model list (family-name heuristic,
 *  with the newest hardcoded families as fallback if the list is unavailable). */
export function resolveModel(tier: Tier, available: Model[]): string {
  const s = settings();
  // Single model picker wins over everything when a concrete model is chosen.
  if (s.model && s.model !== "auto") return s.model;
  if (s.modelMode === "custom") {
    const custom = { fast: s.customModels?.fast, balanced: s.customModels?.default, deep: s.customModels?.deep }[tier];
    if (custom) return custom;
  }
  if (s.modelMode !== "auto" && s.modelMode !== "custom") tier = s.modelMode as Tier;
  const families: Record<Tier, string[]> = {
    fast: ["haiku"],
    balanced: ["sonnet"],
    deep: ["fable", "opus", "sonnet"],
  };
  for (const fam of families[tier]) {
    const hit = available.find((m) => m.id.includes(fam));
    if (hit) return hit.id;
  }
  return available[0]?.id ?? { fast: "claude-haiku-4-5", balanced: "claude-sonnet-5", deep: "claude-opus-4-8" }[tier];
}

async function run(tier: Tier, system: string, prompt: string, maxTokens?: number): Promise<string> {
  const model = resolveModel(tier, await models().catch(() => []));
  return api.anthropicRun(model, system, prompt, maxTokens);
}

const WORKSPACE_FOLDERS =
  "inbox, notes, snippets, commands, bugs, adr, runbooks, pdfs, references, projects, context, templates";

// ---- AI actions ----

export function suggestLabels(path: string, content: string) {
  return run(
    "fast",
    `You label files in a developer workspace. Reply with ONLY a JSON object: {"type": one of [bug, adr, runbook, snippet, command, reference, meeting, learning, pdf, project-context, note], "tags": string[], "project": string|null, "language": string|null, "title": string}. No prose.`,
    `File: ${path}\n\n${content.slice(0, 8000)}`,
    500,
  );
}

export function suggestFilename(content: string) {
  return run(
    "fast",
    `Suggest a clean workspace-relative file path for this content. Folders available: ${WORKSPACE_FOLDERS}. Use kebab-case, .md extension for notes. Reply with ONLY the path, nothing else. Example: bugs/twilio-webhook-timeout-duplicate-messages.md`,
    content.slice(0, 6000),
    100,
  );
}

export function enrichNote(content: string) {
  return run(
    "balanced",
    `You turn rough developer notes into well-structured Markdown with these sections when relevant: Summary, Context, Steps, Commands, Related files, Follow-up. Preserve all original information and code. Reply with ONLY the improved Markdown document.`,
    content,
  );
}

export function explainError(text: string) {
  return run(
    "balanced",
    `You are a senior engineer explaining an error or log excerpt. Structure your answer as Markdown with sections: Likely cause, Where to look, Next tests, Possible fix, Useful commands. Be concrete and brief.`,
    text.slice(0, 20000),
  );
}

export function summarize(name: string, text: string) {
  return run(
    "balanced",
    `Summarize the following document as Markdown: a short overview paragraph, key points as bullets, and an action checklist if the content calls for one. Start with "# Summary: ${name}".`,
    text.slice(0, 100000),
  );
}

export function buildContext(name: string, corpus: string) {
  return run(
    "deep",
    `You build a project context file that a developer will paste into an AI coding assistant. From the workspace excerpts provided, produce Markdown with sections: Overview, Architecture notes, Important files, Decisions, Known bugs, Commands, Snippets, Open questions, References. Only include sections with real content. Start with "# ${name} — context".`,
    corpus.slice(0, 150000),
  );
}

export function askPugdock(question: string, contextBlocks: { path: string; text: string }[]) {
  const ctx = contextBlocks.map((b) => `--- ${b.path} ---\n${b.text}`).join("\n\n");
  return run(
    "deep",
    `You are PugDock, answering questions about the user's own developer workspace. Use ONLY the provided workspace excerpts. Cite the file paths you used. If the answer isn't in the excerpts, say so.`,
    `Workspace excerpts:\n\n${ctx.slice(0, 100000)}\n\nQuestion: ${question}`,
  );
}

/** Continue an unfinished note from where it stops. */
export function continueWriting(content: string) {
  return run(
    "balanced",
    `You continue a developer's Markdown note from where it stops, matching its tone, structure and language. Reply with ONLY the continuation text — do not repeat existing content.`,
    content.slice(-12000),
  );
}

/** Generate a complete note from a request ("write a note about…"). */
export function draftNote(request: string) {
  return run(
    "balanced",
    `You write a complete, well-structured Markdown note for a developer's personal workspace, based on their request. Start with a single "# " title line. Be practical and concrete. Reply with ONLY the note content.`,
    request,
  );
}

export const ADR_TEMPLATE = `# ADR: <title>

## Status
Proposed

## Context

## Decision

## Consequences

## Alternatives considered
`;

export const RUNBOOK_TEMPLATE = `# Runbook: <title>

## When to use

## Prerequisites

## Steps

## Checks

## Rollback

## Notes
`;

/** Files that must never be sent to the AI, plus user-excluded paths. */
export function aiExcluded(path: string): boolean {
  const base = path.split("/").pop() ?? path;
  if (/^\.env($|\.)|\.(pem|key|token)$|^id_rsa|^id_ed25519|^credentials\.|^secrets\./.test(base) && !base.endsWith(".example")) {
    return true;
  }
  return settings().aiExcluded.some((ex) => path === ex || path.startsWith(ex + "/"));
}
