// Task commands (SPEC 10). The only place the tasks UI talks to the Rust core.
import { invoke } from "@tauri-apps/api/core";
import type { HqError } from "../bindings/HqError.ts";
import type { StatsQuery } from "../bindings/StatsQuery.ts";
import type { Task } from "../bindings/Task.ts";
import type { TaskGroups } from "../bindings/TaskGroups.ts";
import type { TaskImportReport } from "../bindings/TaskImportReport.ts";
import type { TaskInput } from "../bindings/TaskInput.ts";
import type { TaskPatch } from "../bindings/TaskPatch.ts";
import type { TaskQuery } from "../bindings/TaskQuery.ts";
import type { TaskStats } from "../bindings/TaskStats.ts";

/** A rejected command carries `{kind, message}`; turn it into an Error with that message. */
async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    const err = e as Partial<HqError> | string;
    const message = typeof err === "string" ? err : (err.message ?? `${cmd} failed`);
    throw Object.assign(new Error(message), { kind: typeof err === "string" ? "Internal" : err.kind });
  }
}

export function emptyQuery(): TaskQuery {
  return {
    status: [],
    project: null,
    tagsAll: [],
    tagsAny: [],
    tagsNone: [],
    priority: [],
    noPriority: false,
    dueFrom: null,
    dueTo: null,
    scheduledFrom: null,
    scheduledTo: null,
    text: null,
    vaults: [],
    includeHidden: false,
    blocked: null,
    sort: "urgency",
    group: "none",
    limit: null,
    today: null,
  };
}

export function emptyInput(): TaskInput {
  return {
    description: "",
    tags: [],
    project: null,
    note: null,
    vault: null,
    priority: null,
    start: null,
    scheduled: null,
    due: null,
    recur: null,
    depends: [],
    waiting: false,
  };
}

export const tasksQuery = (query: TaskQuery) => call<TaskGroups>("tasks_query", { query });
export const taskAdd = (input: TaskInput) => call<Task>("task_add", { input });
export const taskUpdate = (id: string, patch: TaskPatch) => call<Task>("task_update", { id, patch });
export const taskDone = (id: string) => call<Task>("task_done", { id });
export const taskCancel = (id: string) => call<Task>("task_cancel", { id });
export const taskDelete = (id: string) => call<null>("task_delete", { id });
export const taskAnnotate = (id: string, text: string) => call<Task>("task_annotate", { id, text });
/** Starts the task, or stops it when it is already started. */
export const taskStart = (id: string) => call<Task>("task_start", { id });
export const tasksStats = (query: StatsQuery) => call<TaskStats>("tasks_stats", { query });
export const tasksImportTaskwarrior = (dryRun: boolean) => call<TaskImportReport>("tasks_import_taskwarrior", { dryRun });
