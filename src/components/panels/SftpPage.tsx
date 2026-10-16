import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type MouseEvent, type PointerEvent, type ReactNode } from "react";
import { onWeb } from "../../lib/bridge";
import type { FileRow, Listing, Side, SshHostView } from "../../lib/bindings";
import { listenTo } from "../../lib/bridge";
import { fileSize, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Icon } from "../ui/Icon";
import { Button, EmptyState, IconButton, Select } from "../primitives";

type Progress = {
  job: string;
  file: string;
  sent: number;
  total: number;
  index: number;
  count: number;
  stage: string;
  note: string;
};

type Job = {
  id: string;
  label: string;
  file: string;
  sent: number;
  total: number;
  index: number;
  count: number;
  startedAt: number;
  lastAt: number;
  lastSent: number;
  speed: number;
  failed: string[];
  skipped: number;
  finished: boolean;
  cancelled: boolean;
  refresh: (() => void) | null;
};

type Ask = {
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  ask: (title: string, label: string, value: string, confirmLabel: string, danger?: boolean) => Promise<string | null>;
};

export function crumbs(dir: string, kind: Side["kind"]): { label: string; path: string }[] {
  const parts = dir.split(/[\\/]+/).filter(Boolean);
  if (kind === "local") {
    const out = [{ label: tr("Drives"), path: "::drives" }];
    if (/^[\\/]{2}[^\\/]/.test(dir) && parts.length >= 2) {
      let share = `\\\\${parts[0]}\\${parts[1]}`;
      out.push({ label: share, path: share });
      for (const part of parts.slice(2)) {
        share = `${share}\\${part}`;
        out.push({ label: part, path: share });
      }
      return out;
    }
    let at = "";
    parts.forEach((part, index) => {
      at = index === 0 ? `${part}\\` : `${at.replace(/\\$/, "")}\\${part}`;
      out.push({ label: part, path: at });
    });
    return out;
  }
  if (kind === "panel") {
    const out = [{ label: "/", path: "" }];
    parts.forEach((part, index) => out.push({ label: part, path: parts.slice(0, index + 1).join("/") }));
    return out;
  }
  if (!dir.startsWith("/")) return [{ label: dir || "~", path: dir }];
  const out = [{ label: "/", path: "/" }];
  parts.forEach((part, index) => out.push({ label: part, path: `/${parts.slice(0, index + 1).join("/")}` }));
  return out;
}

export type SortKey = "name" | "size" | "time";

export function visibleRows(rows: FileRow[], { hidden, query, sort, desc }: { hidden: boolean; query: string; sort: SortKey; desc: boolean }) {
  const words = query.trim().toLowerCase();
  const kept = rows.filter((row) => (hidden || !row.name.startsWith(".")) && (!words || row.name.toLowerCase().includes(words)));
  const order = (a: FileRow, b: FileRow) =>
    sort === "size" ? a.sizeBytes - b.sizeBytes : sort === "time" ? (a.modifiedAt ?? 0) - (b.modifiedAt ?? 0) : a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  return kept.sort((a, b) => (a.directory !== b.directory ? (a.directory ? -1 : 1) : (desc ? -1 : 1) * order(a, b) || a.name.localeCompare(b.name)));
}

function rowIcon(row: FileRow) {
  if (row.directory) return "folder";
  const ext = row.name.toLowerCase().split(".").pop() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"].includes(ext)) return "image";
  if (["zip", "gz", "tgz", "tar", "7z", "rar", "mrpack", "xz"].includes(ext)) return "archive";
  if (["jar", "wasm", "exe", "dll"].includes(ext)) return "package";
  if (["sh", "bat", "cmd", "ps1"].includes(ext)) return "terminal";
  if (["log"].includes(ext)) return "list";
  return "file";
}

function savedHidden() {
  try {
    return localStorage.getItem("monolab-sftp-hidden") === "1";
  } catch {
    return false;
  }
}

const sideKey = (side: Side) => (side.kind === "local" ? "local" : side.kind === "ssh" ? `ssh:${side.host_id}` : `panel:${side.instance_id}`);

export function SftpPage({
  sshHosts,
  panelServers,
  onError,
  onNotice,
  onSshChanged,
  confirm,
  ask,
}: {
  sshHosts: SshHostView[];
  panelServers: { id: string; name: string }[];
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onSshChanged?: (hostId: string) => void;
} & Ask) {
  const remotes = useMemo(
    () => [
      ...sshHosts.map((one) => ({ value: `ssh:${one.id}`, label: `SSH · ${one.name}`, short: one.name, side: { kind: "ssh", host_id: one.id } as Side })),
      ...panelServers.map((one) => ({ value: `panel:${one.id}`, label: `${tr("Panel")} · ${one.name}`, short: one.name, side: { kind: "panel", instance_id: one.id } as Side })),
    ],
    [sshHosts, panelServers],
  );
  const places = useMemo(() => [{ value: "local", label: tr("Local"), short: tr("Local"), side: { kind: "local" } as Side }, ...remotes], [remotes]);
  const remember = (key: string, fallback: string) => {
    try {
      return localStorage.getItem(key) ?? fallback;
    } catch {
      return fallback;
    }
  };
  const [leftKey, setLeftKey] = useState(() => remember("monolab-sftp-left", "local"));
  const [remoteKey, setRemoteKey] = useState(() => remember("monolab-sftp-remote", ""));
  const left = places.find((one) => one.value === leftKey) ?? places[0];
  const remote = places.find((one) => one.value === remoteKey) ?? remotes[0];
  useEffect(() => {
    try {
      localStorage.setItem("monolab-sftp-left", left.value);
      if (remote) localStorage.setItem("monolab-sftp-remote", remote.value);
    } catch {}
  }, [left, remote]);

  const [jobs, setJobs] = useState<Job[]>([]);
  const jobsRef = useRef<Job[]>([]);
  jobsRef.current = jobs;
  const leftRef = useRef<PaneHandle | null>(null);
  const rightRef = useRef<PaneHandle | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [over, setOver] = useState<string | null>(null);
  const dropRef = useRef<(target: string | null, drag: Drag) => void>(() => {});
  const paneAt = (x: number, y: number) =>
    (document.elementFromPoint(x, y)?.closest("[data-sftp-pane]") as HTMLElement | null)?.dataset.sftpPane ?? null;
  useEffect(() => {
    if (!drag) return;
    const move = (event: globalThis.PointerEvent) => {
      setDrag((now) => (now ? { ...now, x: event.clientX, y: event.clientY } : now));
      const under = paneAt(event.clientX, event.clientY);
      setOver(under && under !== drag.from ? under : null);
    };
    const up = (event: globalThis.PointerEvent) => {
      const target = paneAt(event.clientX, event.clientY);
      dropRef.current(target, drag);
      setDrag(null);
      setOver(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drag?.from, drag?.names.join("/")]);
  const osDropRef = useRef<(paths: string[], x: number, y: number) => void>(() => {});
  const osOverRef = useRef<(x: number, y: number) => void>(() => {});
  useEffect(() => {
    if (onWeb) return;
    let stop: (() => void) | undefined;
    let gone = false;
    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        const at = "position" in event.payload ? event.payload.position : null;
        const x = at ? at.x / window.devicePixelRatio : 0;
        const y = at ? at.y / window.devicePixelRatio : 0;
        if (event.payload.type === "over") osOverRef.current(x, y);
        else if (event.payload.type === "drop") osDropRef.current(event.payload.paths, x, y);
        else setOver(null);
      });
      if (gone) unlisten();
      else stop = unlisten;
    })();
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  useEffect(() => {
    const stop = listenTo<Progress>("sftp-progress", (payload) => {
      setJobs((list) =>
        list.map((job) => {
          if (job.id !== payload.job) return job;
          const now = performance.now();
          if (payload.stage === "done" || payload.stage === "cancelled") {
            job.refresh?.();
            if (payload.note) onError(payload.note);
            const failed = payload.note && !job.failed.length ? [payload.note] : job.failed;
            return { ...job, failed, finished: true, cancelled: payload.stage === "cancelled" };
          }
          if (payload.stage === "failed") return { ...job, failed: [...job.failed, `${payload.file}: ${payload.note}`] };
          if (payload.stage === "skipped") return { ...job, skipped: job.skipped + 1 };
          const sameFile = payload.file === job.file;
          const gained = sameFile ? payload.sent - job.lastSent : payload.sent;
          const span = (now - job.lastAt) / 1000;
          const speed = span > 0.25 ? gained / span : job.speed;
          return {
            ...job,
            file: payload.file,
            sent: payload.sent,
            total: payload.total,
            index: payload.index,
            count: payload.count,
            speed: span > 0.25 ? job.speed * 0.5 + speed * 0.5 : job.speed,
            lastAt: span > 0.25 ? now : job.lastAt,
            lastSent: span > 0.25 || !sameFile ? payload.sent : job.lastSent,
          };
        }),
      );
    });
    return () => {
      void stop.then((off) => off());
    };
  }, [onError]);

  const send = useCallback(
    async (from: Side, fromDir: string, names: string[], to: Side, toDir: string, refresh: () => void) => {
      if (!names.length) return;
      const id = `sftp-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      const verb = from.kind === "local" ? tr("Upload") : to.kind === "local" ? tr("Download") : tr("Move");
      const label = `${verb} · ${names.length === 1 ? names[0] : tr("count.items", [names.length])}`;
      const now = performance.now();
      setJobs((list) => [
        { id, label, file: "", sent: 0, total: 0, index: 0, count: 0, startedAt: now, lastAt: now, lastSent: 0, speed: 0, failed: [], skipped: 0, finished: false, cancelled: false, refresh },
        ...list,
      ]);
      try {
        const result = await commands.sftpTransfer(id, from, fromDir, names, to, toDir);
        const parts = [tr("{0, plural, one {# moved} other {# moved}}", [result.sent])];
        if (result.skipped) parts.push(tr("{0} skipped (same name)", [result.skipped]));
        if (result.failed) parts.push(tr("{0} failed", [result.failed]));
        if (result.cancelled) parts.push(tr("cancelled"));
        onNotice(parts.join(" · "));
        if (to.kind === "ssh" && result.sent > 0) onSshChanged?.(to.host_id);
        if (result.errors.length) onError(result.errors.join("\n"));
      } catch (cause) {
        onError(String(cause));
        setJobs((list) => list.map((job) => (job.id === id ? { ...job, failed: [...job.failed, String(cause)] } : job)));
      } finally {
        refresh();
        setJobs((list) => list.map((job) => (job.id === id ? { ...job, finished: true } : job)));
      }
    },
    [onError, onNotice, onSshChanged],
  );

  if (!remote) {
    return (
      <EmptyState
        icon="plug"
        title={tr("No connections available")}
        hint={tr("Add an SSH connection or Hydrodactyl panel using Add session above the server list. Panel servers appear here once they're in your list.")}
      />
    );
  }

  const paneOf = (key: string | null) => (key === "left" ? leftRef.current : key === "right" ? rightRef.current : null);
  const dropOn = (target: string | null, drag: Drag) => {
    if (!target || target === drag.from) return;
    const from = paneOf(drag.from);
    const to = paneOf(target);
    if (!from || !to) return;
    void send(from.side, drag.dir, drag.names, to.side, to.dir(), () => {
      from.reload();
      to.reload();
    });
  };
  dropRef.current = dropOn;
  osOverRef.current = (x, y) => {
    const under = paneAt(x, y);
    setOver(under && paneOf(under)?.side.kind !== "local" ? under : null);
  };
  osDropRef.current = (paths, x, y) => {
    setOver(null);
    const to = paneOf(paneAt(x, y));
    if (!to || to.side.kind === "local") {
      onError(tr("Drop files from File Explorer onto the SSH or panel server pane."));
      return;
    }
    const groups = new Map<string, string[]>();
    for (const path of paths) {
      const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
      if (cut < 0) continue;
      const folder = path.slice(0, cut) || path.slice(0, cut + 1);
      groups.set(folder, [...(groups.get(folder) ?? []), path.slice(cut + 1)]);
    }
    for (const [folder, names] of groups) {
      void send({ kind: "local" }, folder.endsWith(":") ? folder + "\\" : folder, names, to.side, to.dir(), () => to.reload());
    }
  };
  const transferFrom = (fromLeft: boolean) => {
    const from = fromLeft ? leftRef.current : rightRef.current;
    const to = fromLeft ? rightRef.current : leftRef.current;
    if (!from || !to) return;
    const names = from.selected();
    if (!names.length) {
      onError(tr("Select items to transfer first."));
      return;
    }
    void send(from.side, from.dir(), names, to.side, to.dir(), () => {
      from.reload();
      to.reload();
    });
  };

  return (
    <div className="sftp-page">
      <div className="sftp-panes">
        <Pane
          key={`left-${left.value}`}
          ref={leftRef}
          title={<Select value={left.value} options={places.map((one) => ({ value: one.value, label: one.label }))} onChange={(value: string) => setLeftKey(value)} />}
          side={left.side}
          paneKey="left"
          highlight={over === "left"}
          sendTo={remote.short}
          onSend={() => transferFrom(true)}
          onDragOut={setDrag}
          onError={onError}
          onNotice={onNotice}
          confirm={confirm}
          ask={ask}
        />
        <Pane
          key={`right-${remote.value}`}
          ref={rightRef}
          title={
            <Select
              value={remote.value}
              options={places.map((one) => ({ value: one.value, label: one.label }))}
              onChange={(value: string) => setRemoteKey(value)}
            />
          }
          side={remote.side}
          paneKey="right"
          highlight={over === "right"}
          sendTo={left.short}
          onSend={() => transferFrom(false)}
          onDragOut={setDrag}
          onError={onError}
          onNotice={onNotice}
          confirm={confirm}
          ask={ask}
        />
      </div>
      {drag && (
        <div className="sftp-ghost" style={{ left: drag.x + 14, top: drag.y + 10 }}>
          <Icon name="file" />
          {drag.names.length === 1 ? drag.names[0] : tr("count.items", [drag.names.length])}
        </div>
      )}
      {jobs.length > 0 && (
        <div className="sftp-jobs">
          <div className="sftp-jobs-head">
            <strong>{tr("Transfers")} <span className="muted">{jobs.filter((job) => !job.finished).length > 0 ? tr("{0} in progress", [jobs.filter((job) => !job.finished).length]) : ""}</span></strong>
            {jobs.some((job) => job.finished) && (
              <Button variant="ghost" size="sm" onClick={() => setJobs((list) => list.filter((job) => !job.finished))}>
                {tr("Clear completed")}
              </Button>
            )}
          </div>
          {jobs.map((job) => {
            const ratio = job.total > 0 ? Math.min(1, job.sent / job.total) : job.finished && !job.failed.length ? 1 : 0;
            return (
              <div className={`sftp-job ${job.finished ? "done" : ""} ${job.failed.length ? "bad" : ""}`} key={job.id}>
                <div className="sftp-job-top">
                  <strong>{job.label}</strong>
                  <span className="muted mono">
                    {job.count > 0 && `${job.index} / ${job.count}`}
                    {!job.finished && job.speed > 0 && ` · ${fileSize(job.speed)}/s`}
                    {job.skipped > 0 && ` · ${tr("Skipped {0}", [job.skipped])}`}
                  </span>
                  {!job.finished ? (
                    <IconButton label={tr("Cancel")} icon="close" onClick={() => void commands.sftpCancel(job.id)} />
                  ) : (
                    <span className="muted">{job.cancelled ? tr("Cancelled") : job.failed.length ? tr("Some actions failed") : tr("Completed")}</span>
                  )}
                </div>
                <div className="sftp-bar">
                  <i style={{ width: `${ratio * 100}%` }} />
                </div>
                <small className="muted sftp-file">
                  {job.file ? `${job.file} · ${fileSize(job.sent)}${job.total ? ` / ${fileSize(job.total)}` : ""}` : tr("Getting ready…")}
                </small>
                {job.failed.map((line) => (
                  <small className="sftp-fail" key={line}>
                    {line}
                  </small>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

type PaneHandle = {
  side: Side;
  dir: () => string;
  selected: () => string[];
  reload: () => void;
};

type Drag = { from: string; dir: string; names: string[]; x: number; y: number };

const Pane = forwardRef<
  PaneHandle,
  {
    title: ReactNode;
    side: Side;
    paneKey: string;
    highlight: boolean;
    sendTo: string;
    onSend: () => void;
    onDragOut: (drag: Drag) => void;
    onError: (message: string) => void;
    onNotice: (message: string) => void;
  } & Ask
>(function Pane({ title, side, paneKey, highlight, sendTo, onSend, onDragOut, onError, onNotice, confirm, ask }, ref) {
  const [listing, setListing] = useState<Listing | null>(null);
  const [loading, setLoading] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [hovered, setHovered] = useState<FileRow | null>(null);
  const [typing, setTyping] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hidden, setHidden] = useState(savedHidden);
  const [sort, setSort] = useState<SortKey>("name");
  const [desc, setDesc] = useState(false);
  const lastPick = useRef<number | null>(null);
  const asked = useRef(0);
  const myKey = sideKey(side);

  const open = useCallback(
    async (path: string) => {
      const turn = ++asked.current;
      setLoading(true);
      try {
        const got = await commands.sftpList(side, path);
        if (turn !== asked.current) return;
        setListing(got);
        setQuery("");
        setPicked(new Set());
        lastPick.current = null;
      } catch (cause) {
        if (turn === asked.current) onError(String(cause));
      } finally {
        if (turn === asked.current) setLoading(false);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [myKey, onError],
  );

  useEffect(() => {
    setListing(null);
    void open("");
  }, [open]);

  const dir = listing?.path ?? "";
  useImperativeHandle(ref, () => ({
    side,
    dir: () => dir,
    selected: () => [...picked],
    reload: () => void open(dir),
  }));

  const all = useMemo(() => listing?.entries ?? [], [listing]);
  const rows = useMemo(() => visibleRows(all, { hidden, query, sort, desc }), [all, hidden, query, sort, desc]);
  const shownHover = hovered && rows.includes(hovered) ? hovered : null;
  const hiddenCount = hidden ? 0 : all.filter((row) => row.name.startsWith(".")).length;
  const sortBy = (key: SortKey) => {
    if (sort === key) setDesc(!desc);
    else {
      setSort(key);
      setDesc(key !== "name");
    }
  };
  const flipHidden = () => {
    setHidden(!hidden);
    try {
      localStorage.setItem("monolab-sftp-hidden", hidden ? "0" : "1");
    } catch {}
  };
  const goUp = () => {
    if (listing?.parent === null || listing?.parent === undefined) return;
    void open(listing.parent === "" && side.kind === "local" ? "::drives" : listing.parent);
  };
  const toggle = (name: string, index: number) => {
    const next = new Set(picked);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    setPicked(next);
    lastPick.current = index;
  };
  const click = (row: FileRow, index: number, event: MouseEvent) => {
    if (event.shiftKey && lastPick.current !== null) {
      const [a, b] = [lastPick.current, index].sort((x, y) => x - y);
      setPicked(new Set([...picked, ...rows.slice(a, b + 1).map((one) => one.name)]));
      return;
    }
    if (event.ctrlKey || event.metaKey) {
      toggle(row.name, index);
      return;
    }
    setPicked(new Set([row.name]));
    lastPick.current = index;
  };
  const enter = (row: FileRow) => {
    if (!row.directory) return;
    const sep = side.kind === "local" ? "\\" : "/";
    const next = side.kind === "local" && !dir ? row.name : dir ? `${dir.replace(/[\\/]+$/, "")}${sep}${row.name}` : row.name;
    void open(next);
  };

  const pressRow = (row: FileRow, event: PointerEvent) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest("input")) return;
    const startX = event.clientX;
    const startY = event.clientY;
    const names = picked.has(row.name) ? [...picked] : [row.name];
    const move = (moved: globalThis.PointerEvent) => {
      if (Math.abs(moved.clientX - startX) + Math.abs(moved.clientY - startY) < 6) return;
      stop();
      if (!picked.has(row.name)) setPicked(new Set([row.name]));
      onDragOut({ from: paneKey, dir, names, x: moved.clientX, y: moved.clientY });
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  };

  const one = picked.size === 1 ? [...picked][0] : null;
  const canWrite = !(side.kind === "local" && !dir);
  const allPicked = rows.length > 0 && rows.every((row) => picked.has(row.name));
  const pickedBytes = rows.filter((row) => picked.has(row.name) && !row.directory).reduce((sum, row) => sum + row.sizeBytes, 0);

  const makeFolder = async () => {
    const name = await ask(tr("New folder"), tr("Name"), "", tr("Create"));
    if (!name?.trim()) return;
    try {
      await commands.sftpMakeDir(side, dir, name.trim());
      void open(dir);
    } catch (cause) {
      onError(String(cause));
    }
  };
  const renameOne = async () => {
    if (!one) return;
    const name = await ask(tr("Rename"), tr("New name"), one, tr("Change"));
    if (!name?.trim() || name === one) return;
    try {
      await commands.sftpRename(side, dir, one, name.trim());
      void open(dir);
    } catch (cause) {
      onError(String(cause));
    }
  };
  const removePicked = async () => {
    const names = [...picked];
    if (!names.length) return;
    if (!(await confirm(tr("Delete?"), tr("Deletes {0}. There's no trash here, so this can't be undone.", [names.length === 1 ? names[0] : tr("count.items", [names.length])]), tr("Delete"), true))) return;
    try {
      await commands.sftpDelete(side, dir, names);
      onNotice(tr("Deleted."));
      void open(dir);
    } catch (cause) {
      onError(String(cause));
    }
  };
  const onKey = (event: React.KeyboardEvent) => {
    if ((event.target as HTMLElement).closest("input")) return;
    if (event.key === "Backspace") {
      event.preventDefault();
      goUp();
    } else if (event.key === "Enter" && one) {
      const row = rows.find((item) => item.name === one);
      if (row) enter(row);
    } else if (event.key === "Delete" && picked.size && canWrite) {
      void removePicked();
    } else if (event.key === "F2" && one) {
      void renameOne();
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "a") {
      event.preventDefault();
      setPicked(new Set(rows.map((row) => row.name)));
    } else if (event.key === "Escape") {
      setPicked(new Set());
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const at = one ? rows.findIndex((row) => row.name === one) : -1;
      const next = Math.max(0, Math.min(rows.length - 1, at + (event.key === "ArrowDown" ? 1 : -1)));
      if (rows[next]) {
        setPicked(new Set([rows[next].name]));
        lastPick.current = next;
        document.getElementById(`${paneKey}-row-${next}`)?.scrollIntoView({ block: "nearest" });
      }
    }
  };
  const head = (key: SortKey, label: string) => (
    <button type="button" className={`sftp-sort ${sort === key ? "on" : ""} ${desc ? "desc" : ""}`} onClick={() => sortBy(key)}>
      {label}
      {sort === key && <Icon name="chevron" />}
    </button>
  );

  return (
    <section className={`sftp-pane ${highlight ? "dropping" : ""}`} data-sftp-pane={paneKey}>
      <header className="sftp-pane-head">
        <span className="sftp-kind" aria-hidden>
          <Icon name={side.kind === "local" ? "monitor" : side.kind === "ssh" ? "plug" : "cloud"} />
        </span>
        <div className="sftp-title">{typeof title === "string" ? <strong>{title}</strong> : title}</div>
        <label className="sftp-search">
          <Icon name="search" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => event.key === "Escape" && setQuery("")}
            placeholder={tr("Find in this folder")}
            spellCheck={false}
          />
          {query && <IconButton label={tr("Remove")} icon="close" onClick={() => setQuery("")} />}
        </label>
        <div className="sftp-tools">
          <IconButton
            label={hidden ? tr("Hide hidden files") : tr("Show hidden files")}
            icon={hidden ? "eye" : "eye-off"}
            className={hidden ? "on" : ""}
            aria-pressed={hidden}
            onClick={flipHidden}
          />
          <IconButton label={tr("Refresh")} icon="refresh" onClick={() => void open(dir)} />
          <IconButton label={tr("New folder")} icon="plus" disabled={!canWrite} onClick={() => void makeFolder()} />
        </div>
      </header>
      <div className="sftp-path">
        <IconButton label={tr("Up (Backspace)")} icon="arrow-up" disabled={listing?.parent === null || listing?.parent === undefined} onClick={goUp} />
        {typing !== null ? (
          <input
            autoFocus
            className="mono"
            onFocus={(event) => event.target.select()}
            value={typing}
            onChange={(event) => setTyping(event.target.value)}
            onBlur={() => setTyping(null)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                void open(typing);
                setTyping(null);
              }
              if (event.key === "Escape") setTyping(null);
            }}
          />
        ) : (
          <div
            className="sftp-crumbs"
            onClick={(event) => (event.target === event.currentTarget || (event.target as HTMLElement).classList.contains("sftp-crumb-track")) && setTyping(dir)}
          >
            <div className="sftp-crumb-track" ref={(box) => { if (box) box.scrollLeft = box.scrollWidth; }}>
              {crumbs(dir, side.kind).map((one, index, all) =>
                index === all.length - 1 ? (
                  <span key={one.path + index} className="sftp-crumb now">{one.label}</span>
                ) : (
                  <span key={one.path + index} className="sftp-crumb-step">
                    <button type="button" className="sftp-crumb" onClick={() => void open(one.path)}>
                      {one.label}
                    </button>
                    <Icon name="chevron" />
                  </span>
                ),
              )}
            </div>
            <IconButton label={tr("Click to enter a path")} icon="pencil" onClick={() => setTyping(dir)} />
          </div>
        )}
      </div>
      <div className="sftp-list" tabIndex={0} onKeyDown={onKey} onClick={(event) => event.target === event.currentTarget && setPicked(new Set())}>
        {rows.length > 0 && (
          <div className="sftp-row sftp-head">
            <input type="checkbox" checked={allPicked} aria-label={tr("Select all")} onChange={() => setPicked(allPicked ? new Set() : new Set(rows.map((row) => row.name)))} />
            <span />
            {head("name", tr("Name"))}
            {head("size", tr("Size"))}
            {head("time", tr("Modified"))}
          </div>
        )}
        {loading && !listing ? (
          <p className="muted sftp-empty">{tr("Opening…")}</p>
        ) : !rows.length ? (
          <p className="muted sftp-empty">
            <Icon name={highlight ? "import" : "folder"} />
            {highlight ? tr("Drop here to transfer") : query ? tr("Nothing matches ‘{0}’", [query]) : hiddenCount ? tr("Only hidden files in this folder") : tr("It's empty")}
            {!highlight && !query && <small>{side.kind === "local" ? tr("Drag from the other side to download here") : tr("Drag from the other side or File Explorer to upload")}</small>}
          </p>
        ) : (
          rows.map((row, index) => (
            <div
              key={row.name}
              id={`${paneKey}-row-${index}`}
              className={`sftp-row ${row.directory ? "dir" : ""} ${picked.has(row.name) ? "picked" : ""}`}
              onPointerDown={(event) => pressRow(row, event)}
              onClick={(event) => click(row, index, event)}
              onDoubleClick={() => enter(row)}
              onPointerEnter={() => setHovered(row)}
              onPointerLeave={() => setHovered((was) => (was === row ? null : was))}
            >
              <input
                type="checkbox"
                checked={picked.has(row.name)}
                onClick={(event) => event.stopPropagation()}
                onChange={() => toggle(row.name, index)}
                aria-label={row.name}
              />
              <Icon name={rowIcon(row)} />
              <span className="sftp-name">{row.name}</span>
              <span className="muted mono">{row.directory ? "" : fileSize(row.sizeBytes)}</span>
              <span className="muted">{row.modifiedAt ? relativeTime(row.modifiedAt) : ""}</span>
            </div>
          ))
        )}
      </div>
      {picked.size ? (
        <footer className="sftp-foot picking">
          <span className="sftp-foot-count">
            <b>{picked.size === 1 ? [...picked][0] : tr("{0} selected", [picked.size])}</b>
            {pickedBytes > 0 && <span className="muted"> · {fileSize(pickedBytes)}</span>}
          </span>
          <IconButton label={tr("Clear selection (Esc)")} icon="close" onClick={() => setPicked(new Set())} />
          <IconButton label={tr("Rename (F2)")} icon="pencil" disabled={!one} onClick={() => void renameOne()} />
          <IconButton label={tr("Delete (Delete)")} icon="trash" className="danger" disabled={!canWrite} onClick={() => void removePicked()} />
          <Button variant="primary" size="sm" icon="arrow" className="sftp-send" onClick={onSend}>
            {tr("Send to {0}", [sendTo])}
          </Button>
        </footer>
      ) : (
        <footer className="sftp-foot muted">
          {shownHover ? (
            <span className="sftp-foot-count">
              {shownHover.name}
              {!shownHover.directory && ` · ${fileSize(shownHover.sizeBytes)}`}
            </span>
          ) : (
            <span>
              {tr("{0, plural, one {# item} other {# items}}", [rows.length])}
              {hiddenCount > 0 && ` · ${tr("{0} hidden", [hiddenCount])}`}
            </span>
          )}
          <span>{tr("Double-click to open · drag to the other side")}</span>
        </footer>
      )}
    </section>
  );
});
