import { useSyncExternalStore } from "react";
import { commands as raw } from "./bindings";
import { listenTo } from "./bridge";
import { tr } from "./i18n";

export type Job = {
  id: string;
  title: string;
  detail: string;
  icon: string;
  sent?: number;
  total?: number;
  /** "bytes" (default) or "items": how `sent` and `total` are shown. */
  unit?: "bytes" | "items";
  /** What the command is on right now, as the app reports it. */
  note?: string;
  watchServer?: string;
  /** Command name and server, for progress events and cancelling. */
  kind?: string;
  server?: string;
  cancellable?: boolean;
  cancelling?: boolean;
};

let jobs: Job[] = [];
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((one) => one());
function set(next: Job[]) {
  jobs = next;
  emit();
}
export function useJobs() {
  return useSyncExternalStore(
    (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    () => jobs,
  );
}
function patch(id: string, change: Partial<Job>) {
  set(jobs.map((one) => (one.id === id ? { ...one, ...change } : one)));
}
function drop(id: string) {
  set(jobs.filter((one) => one.id !== id));
}

let nameOf: (id: string) => string = () => "";
export function setServerNames(lookup: (id: string) => string) {
  nameOf = lookup;
}

/** The error a cancelled command ends with; screens show nothing for it. */
export function isCancelled(message: string | null | undefined): boolean {
  return !!message && /(^|: )Cancelled\.$/.test(message.trim());
}

type Label = { title: string; icon: string; watch?: boolean; cancel?: boolean };
const count = (list: unknown) => (Array.isArray(list) ? list.length : 0);

const LABELS: Record<string, (args: unknown[]) => Label> = {
  installModrinth: () => ({ title: tr("Installing mod or plugin"), icon: "import", watch: true }),
  installAddonWithDependencies: () => ({ title: tr("Installing with dependencies"), icon: "import", watch: true }),
  applyAddonUpdates: (args) => ({ title: tr("Updating {0, plural, one {# project} other {# projects}}", [count(args[1])]), icon: "refresh", watch: true }),
  addAddonFiles: (args) => ({ title: tr("Adding {0, plural, one {# file} other {# files}}", [count(args[1])]), icon: "import" }),
  installAddon: () => ({ title: tr("Installing the Monolab add-on"), icon: "package" }),
  importMrpack: () => ({ title: tr("Applying modpack"), icon: "archive", watch: true }),
  importCfpack: () => ({ title: tr("Unpacking modpack configs"), icon: "archive" }),
  exportMrpack: () => ({ title: tr("Exporting modpack"), icon: "archive" }),
  exportMrpackDownload: () => ({ title: tr("Exporting modpack"), icon: "archive" }),
  zipServerPaths: (args) => ({ title: tr("Compressing {0, plural, one {# item} other {# items}}", [count(args[1])]), icon: "archive", cancel: true }),
  unzipServerPath: () => ({ title: tr("Extracting"), icon: "archive", cancel: true }),
  backupContents: () => ({ title: tr("Comparing backup"), icon: "backup", cancel: true }),
  createBackup: () => ({ title: tr("Backing up"), icon: "backup", cancel: true }),
  restoreBackupPaths: (args) => ({ title: tr("Restoring {0, plural, one {# item} other {# items}}", [count(args[2])]), icon: "backup" }),
};

/** Progress notes the app sends, shown in the job's language. */
function noteText(note: string): string {
  switch (note) {
    case "Reading the backup":
      return tr("Reading the backup");
    case "Listing the server's files":
      return tr("Listing the server's files");
    case "Comparing files":
      return tr("Comparing files");
    default:
      return note;
  }
}

let serial = 0;
/** Tracked calls already running, by command and arguments. */
const running = new Map<string, Promise<unknown>>();

function keyOf(name: string, args: unknown[]): string | null {
  try {
    return `${name}:${JSON.stringify(args)}`;
  } catch {
    return null;
  }
}

function serverOf(args: unknown[]): string {
  const first = args[0];
  if (typeof first === "string") return first;
  if (first && typeof first === "object" && typeof (first as { id?: unknown }).id === "string") return (first as { id: string }).id;
  return "";
}

/**
 * Runs a long command and lists it as a job. The same command with the same arguments while one is
 * still running shares that run, so a screen asking twice (React runs effects twice in development,
 * a double click) shows one job and does the work once.
 */
export function tracked<T>(name: string, args: unknown[], run: () => Promise<T>): Promise<T> {
  const label = LABELS[name];
  if (!label) return run();
  const key = keyOf(name, args);
  const same = key ? running.get(key) : undefined;
  if (same) return same as Promise<T>;
  const made = label(args);
  const server = serverOf(args);
  const id = `job-${++serial}`;
  set([
    ...jobs,
    {
      id,
      title: made.title,
      detail: nameOf(server),
      icon: made.icon,
      watchServer: made.watch ? server : undefined,
      kind: name,
      server,
      cancellable: !!made.cancel && !!server,
    },
  ]);
  const work = run().finally(() => {
    drop(id);
    if (key) running.delete(key);
  });
  if (key) running.set(key, work);
  return work;
}

/** Asks the app to stop a job; it ends with a "Cancelled." error that screens ignore. */
export function cancelJob(id: string) {
  const one = jobs.find((job) => job.id === id);
  if (!one?.kind || !one.cancellable || one.cancelling) return;
  patch(id, { cancelling: true });
  void raw.cancelTask(one.kind, one.server ?? "").catch(() => patch(id, { cancelling: false }));
}

let started = false;
export function startJobEvents() {
  if (started) return;
  started = true;
  void listenTo<{ serverId: string; file: string; received: number; total: number | null }>("addon-download", (event) => {
    const one = [...jobs].reverse().find((job) => job.watchServer === event.serverId);
    if (!one) return;
    const server = nameOf(event.serverId);
    patch(one.id, {
      detail: server ? `${server} · ${event.file}` : event.file,
      sent: event.received,
      total: event.total ?? undefined,
    });
  });
  void listenTo<{ kind: string; serverId: string; done: number; total: number; unit: string; note: string }>("task-progress", (event) => {
    const one = [...jobs].reverse().find((job) => job.kind === event.kind && job.server === event.serverId);
    if (!one) return;
    patch(one.id, {
      sent: event.done,
      total: event.total > 0 ? event.total : undefined,
      unit: event.unit === "items" ? "items" : "bytes",
      note: noteText(event.note),
    });
  });
  void listenTo<{ stage: string; major: number; received?: number; total?: number | null }>("java-download", (event) => {
    const id = `java-${event.major}`;
    if (event.stage === "complete" || event.stage === "failed") {
      drop(id);
      return;
    }
    const detail = event.stage === "extracting" ? tr("Extracting") : tr("Downloading from Adoptium");
    const change = {
      title: tr("Downloading Java {0}", [event.major]),
      detail,
      icon: "import",
      sent: event.stage === "downloading" ? event.received : undefined,
      total: event.stage === "downloading" ? (event.total ?? undefined) : undefined,
    };
    if (jobs.some((one) => one.id === id)) patch(id, change);
    else set([...jobs, { id, ...change }]);
  });
}

export function resetJobs() {
  running.clear();
  set([]);
}
