// Laufende Aufgaben für Pet und Startseite. Nutzt dieselbe Job-Warteschlange wie alles
// Langlaufende, es gibt also keine zweite Statusquelle.
import { listJobs, onJobsChanged } from "../ipc";
import type { JobInfo } from "../types";

export const jobs = $state<{ list: JobInfo[] }>({ list: [] });

let started = false;

export async function initJobsStore(): Promise<void> {
  if (started) return;
  started = true;
  try { jobs.list = await listJobs(); } catch (reason) { console.error(reason); }
  try { await onJobsChanged((list) => { jobs.list = list; }); } catch (reason) { console.error(reason); }
}

/** Aktuelle Aufgabe: der laufende Job, sonst `null`; dazu die Zahl der wartenden. */
export function currentTask(list: JobInfo[]): { label: string; waiting: number } | null {
  const running = list.find((job) => job.status === "running");
  const waiting = list.filter((job) => job.status === "waiting").length;
  if (!running) return waiting > 0 ? { label: list.find((job) => job.status === "waiting")?.label ?? "", waiting: waiting - 1 } : null;
  return { label: running.label, waiting };
}
