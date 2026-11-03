// The onboarding wizard's calls into the app (SPEC 10 "onboarding"), plus the few commands
// of other streams it drives (vaults, tasks import, Ollama, bookmarks). Those may not be
// in this build yet: they then fail with kind "Unavailable" and the step says so.
// When the shell's api.ts lands, `call` can become its `invoke` wrapper.

import { invoke } from "@tauri-apps/api/core";
import type { Agent } from "../bindings/Agent.ts";
import type { ConfigApplied } from "../bindings/ConfigApplied.ts";
import type { ConfigDiff } from "../bindings/ConfigDiff.ts";
import type { ImportAnalysis } from "../bindings/ImportAnalysis.ts";
import type { ImportPlan } from "../bindings/ImportPlan.ts";
import type { LocationSuggestion } from "../bindings/LocationSuggestion.ts";
import type { OnboardingState } from "../bindings/OnboardingState.ts";
import type { RestructureReport } from "../bindings/RestructureReport.ts";
import type { ScaffoldReport } from "../bindings/ScaffoldReport.ts";
import type { TaskImportReport } from "../bindings/TaskImportReport.ts";
import type { VaultInfo } from "../bindings/VaultInfo.ts";

export type {
  Agent,
  ConfigApplied,
  ConfigDiff,
  ImportAnalysis,
  ImportPlan,
  LocationSuggestion,
  OnboardingState,
  RestructureReport,
  ScaffoldReport,
  TaskImportReport,
  VaultInfo,
};

export type ErrorKind = "NotFound" | "Conflict" | "Invalid" | "Denied" | "Unavailable" | "Internal";
const KINDS: readonly ErrorKind[] = ["NotFound", "Conflict", "Invalid", "Denied", "Unavailable", "Internal"];

export class OnboardingError extends Error {
  readonly kind: ErrorKind;
  constructor(kind: ErrorKind, message: string) {
    super(message);
    this.name = "OnboardingError";
    this.kind = kind;
  }
}

/** Whatever `invoke` rejected with, as an OnboardingError. */
export function toError(raw: unknown, cmd: string): OnboardingError {
  if (raw instanceof OnboardingError) return raw;
  if (raw && typeof raw === "object" && "kind" in raw && "message" in raw) {
    const r = raw as { kind: unknown; message: unknown };
    return new OnboardingError(KINDS.find((k) => k === r.kind) ?? "Internal", String(r.message));
  }
  const message = typeof raw === "string" ? raw : raw instanceof Error ? raw.message : String(raw);
  // Tauri's own refusal for a command this build doesn't have, or no Tauri at all (a browser)
  if (/command \S+ not found|not allowed by ACL|__TAURI|reading 'invoke'/i.test(message)) {
    return new OnboardingError("Unavailable", `${cmd} is not in this build of Vault HQ yet.`);
  }
  return new OnboardingError("Internal", message);
}

export const unavailable = (e: unknown): boolean => e instanceof OnboardingError && e.kind === "Unavailable";

export const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toError(e, cmd);
  }
}

// ---- onboarding (stream E) ----------------------------------------------------------------

export const STEPS = ["welcome", "brain", "agents", "tasks", "ai", "bookmarks", "done"] as const;
export type Step = (typeof STEPS)[number];

export const onboardingState = () => call<OnboardingState>("onboarding_state");
export const setStep = (step: Step) => call<OnboardingState>("onboarding_set_step", { step });
export const locations = () => call<LocationSuggestion[]>("onboarding_locations");
export const scaffoldVault = (vaultId: string, welcome: boolean) =>
  call<ScaffoldReport>("onboarding_scaffold", { vaultId, welcome });
export const agentsCatalog = () => call<Agent[]>("agents_catalog");
export const agentPrompt = (agentId: string, vaultId: string) => call<string>("agent_prompt", { agentId, vaultId });
export const configPreview = (agentId: string) => call<ConfigDiff>("agent_config_preview", { agentId });
export const configApply = (agentId: string, baseHash: string | null) =>
  call<ConfigApplied>("agent_config_apply", { agentId, baseHash });
export const importAnalyze = (path: string) => call<ImportAnalysis>("import_analyze", { path });
export const importApply = (plan: ImportPlan) => call<RestructureReport>("import_apply", { plan });
export const importUndo = (path: string, journal: string) => call<RestructureReport>("import_undo", { path, journal });

// ---- other streams, by their SPEC names. Shapes are kept loose on purpose: only the
// fields the wizard shows, all optional beyond the essentials. -------------------------------

export const vaultsList = () => call<VaultInfo[]>("vaults_list");
export const vaultCreate = (path: string, name: string) =>
  call<VaultInfo>("vault_create", { path, name, layout: "default" });
export const vaultAdd = (path: string) => call<VaultInfo>("vault_add", { path });
export const vaultSetPrimary = (id: string) => call<unknown>("vault_set_primary", { id });

export interface ImportCounts {
  dryRun?: boolean;
  created?: number;
  added?: number;
  updated?: number;
  skipped?: number;
  total?: number;
  sample?: string[];
  messages?: string[];
  [key: string]: unknown;
}
/** Stream B's Taskwarrior import into the primary vault; `dryRun` first, then for real. */
export const tasksImport = (dryRun: boolean) => call<TaskImportReport>("tasks_import_taskwarrior", { dryRun });

/** "12 tasks: 9 pending, 1 waiting, 2 done · 3 already in the vault". */
export function taskReportLine(r: TaskImportReport): string {
  const parts = [`${r.total} tasks: ${r.pending} pending, ${r.waiting} waiting, ${r.completed} done`];
  if (r.deleted) parts.push(`${r.deleted} deleted (not imported)`);
  if (r.duplicates) parts.push(`${r.duplicates} already in the vault`);
  parts.push(`${r.written} ${r.dryRun ? "would be written" : "written"}`);
  return parts.join(" · ");
}

export interface OllamaStatus {
  url: string;
  up: boolean;
  binary?: string | boolean | null;
  models: string[];
  chatModel?: string;
  embedModel?: string;
  problem?: string | null;
}
export interface Recommendation {
  memoryGb: number;
  embed: string;
  chat: string[];
  commands?: string[];
  pulls?: string[];
}
export const ollamaStatus = () => call<OllamaStatus>("ollama_status");
export const ollamaRecommend = () => call<Recommendation>("ollama_recommend");
export const settingsUpdate = (patch: Record<string, unknown>) => call<unknown>("settings_update", { patch });

export interface BrowserSource {
  id: string;
  browser?: string;
  label: string;
  allowed: boolean;
  count?: number | null;
  /** macOS refused the read (Safari without Full Disk Access). */
  denied?: boolean;
  problem?: string | null;
  [key: string]: unknown;
}
export const browsersDetect = () => call<BrowserSource[]>("browsers_detect");
export const browserAllow = (sourceId: string, allowed: boolean) => call<unknown>("browser_allow", { sourceId, allowed });
export const bookmarksImport = (sourceId: string, dryRun: boolean) =>
  call<ImportCounts>("bookmarks_import", { sourceId, dryRun });
export const bookmarksEnrich = () => call<unknown>("bookmarks_enrich", { ids: null });

/** A one-line summary of an import report whose exact shape belongs to another stream. */
export function countsLine(r: ImportCounts | null | undefined): string {
  if (!r) return "";
  const parts: string[] = [];
  for (const [k, v] of Object.entries(r)) {
    if (typeof v === "number") parts.push(`${v} ${k}`);
  }
  return parts.join(" · ") || "done";
}

// ---- clipboard ----------------------------------------------------------------------------

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // older webviews: a hidden textarea and the copy command
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  }
}
