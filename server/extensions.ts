// Per-Mac extras. server/personal.ts exists only on the personal Mac and is never part of
// an exported copy (scripts/export.sh), so a copy without it runs with core features only.
import fs from "node:fs";
import type { Card, Need, Phase, Status } from "./types.ts";

export interface Metric {
  key: string; // daily-note property, written by the vault's daily-metrics.py
  label: string;
  unit?: string;
}

export interface Extension {
  metrics: Metric[];
  // where the extension's card kinds sit in the home carousel, per time of day
  order: Record<Phase, string[]>;
  cards(today: string): Promise<Card[]>;
  needs(today: string): Promise<Need[]>;
  status(): Promise<Status["external"]>;
  // a reason this Mac must never open the vault at `dir`, or null if it may
  vaultAllowed?(dir: string): string | null;
}

// a variable specifier, so a copy without the file still type-checks
const spec = "./personal.ts";
// HQ_CORE_ONLY=1 (the demo) runs without it even where it exists
export const extension: Extension | null = !process.env.HQ_CORE_ONLY && fs.existsSync(new URL(spec, import.meta.url))
  ? ((await import(spec)) as { default: Extension }).default
  : null;
