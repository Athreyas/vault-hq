// The tasks view's calls, over api.ts (the one place that invokes commands).
import { api, emptyTaskInput, emptyTaskQuery, on } from "../api.ts";
import type { StatsQuery, TaskInput, TaskPatch, TaskQuery } from "../api.ts";

export const emptyQuery = emptyTaskQuery;
export const emptyInput = emptyTaskInput;

export const tasksQuery = (query: TaskQuery) => api.tasks(query);
export const taskAdd = (input: TaskInput) => api.taskAdd(input);
export const taskUpdate = (id: string, patch: TaskPatch) => api.taskUpdate(id, patch);
export const taskDone = (id: string) => api.taskDone(id);
export const taskCancel = (id: string) => api.taskCancel(id);
export const taskDelete = (id: string) => api.taskDelete(id);
export const taskAnnotate = (id: string, text: string) => api.taskAnnotate(id, text);
/** Starts the task, or stops it when it is already started. */
export const taskStart = (id: string) => api.taskStart(id);
export const tasksStats = (query: StatsQuery) => api.tasksStats(query);
export const tasksImportTaskwarrior = (dryRun: boolean) => api.tasksImportTaskwarrior(dryRun);

/** Call `fn` (debounced) when any note changes on disk; returns the unsubscribe. */
export function onNotesChanged(fn: () => void): () => void {
  let timer: number | undefined;
  const off = on("vault://changed", () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(fn, 250);
  });
  return () => {
    window.clearTimeout(timer);
    off();
  };
}
