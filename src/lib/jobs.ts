import { useSyncExternalStore } from "react";
import { listenTo } from "./bridge";
import { tr } from "./i18n";

export type Job = {
  id: string;
  title: string;
  detail: string;
  icon: string;
  sent?: number;
  total?: number;
  watchServer?: string;
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

type Label = { title: string; icon: string; watch?: boolean };
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
  zipServerPaths: (args) => ({ title: tr("Compressing {0, plural, one {# item} other {# items}}", [count(args[1])]), icon: "archive" }),
  unzipServerPath: () => ({ title: tr("Extracting"), icon: "archive" }),
};

let serial = 0;
export function tracked<T>(name: string, args: unknown[], run: () => Promise<T>): Promise<T> {
  const label = LABELS[name];
  if (!label) return run();
  const made = label(args);
  const first = args[0];
  const server = typeof first === "string" ? first : first && typeof first === "object" && typeof (first as { id?: unknown }).id === "string" ? (first as { id: string }).id : "";
  const id = `job-${++serial}`;
  set([
    ...jobs,
    { id, title: made.title, detail: nameOf(server), icon: made.icon, watchServer: made.watch ? server : undefined },
  ]);
  return run().finally(() => drop(id));
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
  set([]);
}
