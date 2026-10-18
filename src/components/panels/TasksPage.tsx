import { useState } from "react";
import type { Task as GenTask } from "../../lib/bindings";
import { commands } from "../../lib/ipc";
import { relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import type { Sent } from "../../lib/sent";
import { EmptyState, SegmentedControl, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

export type Task = Sent<GenTask>;
type Kind = "command" | "backup" | "restart" | "start" | "stop" | "notify";
type Server = { id: string; name: string; core: string; version: string };

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MARKS = [1800, 900, 600, 300, 180, 60, 30, 10, 5];

function legacyMarks(minutes: number) {
  const out: number[] = [];
  let left = Math.min(30, minutes);
  while (left > 0) {
    out.push(left * 60);
    left -= left > 5 ? left - 5 : left > 1 ? left - 1 : 1;
  }
  return out;
}

export function warnMarks(task: Task) {
  return task.warnAt.length ? task.warnAt : legacyMarks(task.warnMinutes);
}

function markLabel(seconds: number) {
  return seconds >= 60 && seconds % 60 === 0 ? tr("{0} min", [seconds / 60]) : tr("{0} sec", [seconds]);
}

function warnPreview(task: Task, seconds: number) {
  const fallback = task.kind === "stop" ? tr("The server stops in {time}.") : tr("The server restarts in {time}.");
  const template = task.message.trim() || fallback;
  return template
    .replaceAll("{time}", markLabel(seconds))
    .replaceAll("{minutes}", String(Math.ceil(seconds / 60)))
    .replaceAll("{seconds}", String(seconds));
}
const POWER = new Set<string>(["restart", "start", "stop"]);

export function blankTask(change: Partial<Task> = {}): Task {
  return {
    id: `task-${Date.now()}`,
    name: "",
    enabled: true,
    serverId: "",
    kind: "command",
    command: "",
    message: "",
    schedule: "daily",
    intervalMinutes: 60,
    dailyTime: "04:00",
    lastRunAt: 0,
    weekdays: [],
    warnMinutes: 0,
    warnAt: [300, 60, 10],
    warnStyle: "say",
    finalMessage: "",
    timeWords: [],
    lastResult: "",
    lastOk: false,
    ...change,
  };
}

const TEMPLATES: { label: string; make: () => Partial<Task> }[] = [
  { label: "Back up daily at 4 AM", make: () => ({ name: tr("Nightly backup"), kind: "backup", schedule: "daily", dailyTime: "04:00" }) },
  { label: "Restart daily at 5 AM", make: () => ({ name: tr("Daily restart"), kind: "restart", schedule: "daily", dailyTime: "05:00", warnAt: [300, 60, 10] }) },
  { label: "Save every 30 min", make: () => ({ name: tr("Auto save"), kind: "command", command: "save-all", schedule: "interval", intervalMinutes: 30 }) },
  { label: "Start on weekend mornings", make: () => ({ name: tr("Start on weekends"), kind: "start", schedule: "weekly", weekdays: [5, 6], dailyTime: "09:00" }) },
];

function minutesOf(time: string) {
  const [hour, minute] = time.split(":").map(Number);
  return Number.isFinite(hour) && Number.isFinite(minute) ? hour * 60 + minute : null;
}

export function nextRun(task: Task, now = new Date()): Date | null {
  if (!task.enabled) return null;
  if (task.schedule === "interval") {
    const last = task.lastRunAt ? task.lastRunAt * 1000 : now.getTime();
    return new Date(Math.max(now.getTime(), last + task.intervalMinutes * 60000));
  }
  const at = minutesOf(task.dailyTime);
  if (at === null) return null;
  for (let ahead = 0; ahead < 8; ahead += 1) {
    const day = new Date(now);
    day.setDate(now.getDate() + ahead);
    day.setHours(Math.floor(at / 60), at % 60, 0, 0);
    const weekday = (day.getDay() + 6) % 7;
    if (task.schedule === "weekly" && !task.weekdays.includes(weekday)) continue;
    if (day.getTime() > now.getTime()) return day;
  }
  return null;
}

function whenText(task: Task) {
  if (task.schedule === "interval") return tr("Every {0} min", [task.intervalMinutes]);
  if (task.schedule === "weekly") {
    const days = task.weekdays.length === 7 ? tr("Daily") : task.weekdays.map((one) => tr(DAYS[one])).join("·");
    return tr("Every {0} at {1}", [days, task.dailyTime]);
  }
  return tr("Daily at {0}", [task.dailyTime]);
}

function whatText(task: Task, server: string) {
  switch (task.kind) {
    case "command":
      return tr("/{1} on {0}", [server, task.command]);
    case "backup":
      return tr("{0} backup", [server]);
    case "restart":
      return warnMarks(task).length ? tr("Restart {0} · warns {1} before", [server, warnMarks(task).map(markLabel).join(", ")]) : tr("Restart {0}", [server]);
    case "start":
      return tr("Start {0}", [server]);
    case "stop":
      return warnMarks(task).length ? tr("Stop {0} · warns {1} before", [server, warnMarks(task).map(markLabel).join(", ")]) : tr("Stop {0}", [server]);
    default:
      return task.message;
  }
}

function nextText(date: Date | null) {
  if (!date) return tr("Off");
  const today = new Date();
  const time = date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (date.toDateString() === today.toDateString()) return tr("Next: today {0}", [time]);
  const tomorrow = new Date(today);
  tomorrow.setDate(today.getDate() + 1);
  if (date.toDateString() === tomorrow.toDateString()) return tr("Next: tomorrow {0}", [time]);
  return tr("Next: {0}", [date.toLocaleDateString(undefined, { month: "short", day: "numeric", weekday: "short" }) + " " + time]);
}

function TaskForm({
  draft,
  servers,
  onChange,
  onCancel,
  onSave,
}: {
  draft: Task;
  servers: Server[];
  onChange: (task: Task) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const set = (change: Partial<Task>) => onChange({ ...draft, ...change });
  const kind = draft.kind as Kind;
  return (
    <div className="task-form">
      <label className="field-block">
        <span className="field-label">{tr("Name")}</span>
        <input autoFocus value={draft.name} placeholder={tr("e.g. Nightly autosave")} onChange={(event) => set({ name: event.target.value })} />
      </label>
      <div className="field-block">
        <span className="field-label">{tr("What would you like to do?")}</span>
        <SegmentedControl
          className="task-kinds"
          value={kind}
          onChange={(value) => set({ kind: value })}
          options={[
            { value: "command", label: tr("Command") },
            { value: "backup", label: tr("Backups") },
            { value: "restart", label: tr("Restart") },
            { value: "start", label: tr("Start") },
            { value: "stop", label: tr("Stop server") },
            { value: "notify", label: tr("Alerts") },
          ]}
        />
      </div>
      {kind !== "notify" && (
        <label className="field-block">
          <span className="field-label">{tr("Server")}</span>
          <Dropdown
            value={draft.serverId}
            placeholder={tr("Choose a server")}
            onChange={(serverId) => set({ serverId })}
            options={servers.map((one) => ({ value: one.id, label: one.name, hint: `${one.core} ${one.version}` }))}
          />
        </label>
      )}
      {kind === "command" && (
        <label className="field-block">
          <span className="field-label">{tr("Command to send")}</span>
          <input value={draft.command} placeholder="save-all" onChange={(event) => set({ command: event.target.value })} />
          <small className="field-help">{tr("The leading / is optional. Skipped while the server is off.")}</small>
        </label>
      )}
      {kind === "backup" && <p className="field-help">{tr("If it's running, saving pauses while it packs. Old backups are removed after the retention period in Settings.")}</p>}
      {(kind === "restart" || kind === "stop") && (() => {
        const marks = warnMarks(draft);
        const on = marks.length > 0;
        return (
          <div className="task-warn">
            <label className="task-warn-toggle">
              <Switch
                checked={on}
                onChange={(event) => set(event.target.checked ? { warnAt: [300, 60, 10], warnMinutes: 0 } : { warnAt: [], warnMinutes: 0 })}
              />
              <span>
                <strong>{tr("Warn players in game")}</strong>
                <small>{tr("Messages go out before it happens, then it runs after the last one.")}</small>
              </span>
            </label>
            {on && (
              <>
                <div className="field-block">
                  <span className="field-label">{tr("When to warn")}</span>
                  <div className="task-days" role="group">
                    {MARKS.map((seconds) => {
                      const picked = marks.includes(seconds);
                      return (
                        <button
                          type="button"
                          key={seconds}
                          className={`task-day ${picked ? "on" : ""}`}
                          aria-pressed={picked}
                          onClick={() => {
                            const next = picked ? marks.filter((one) => one !== seconds) : [...marks, seconds];
                            set({ warnAt: next.sort((a, b) => b - a), warnMinutes: 0 });
                          }}
                        >
                          {markLabel(seconds)}
                        </button>
                      );
                    })}
                  </div>
                </div>
                <div className="field-block">
                  <span className="field-label">{tr("How to show it")}</span>
                  <SegmentedControl
                    className="page-tabs"
                    value={(draft.warnStyle || "say") as "say" | "tellraw" | "title"}
                    onChange={(warnStyle) => set({ warnStyle })}
                    options={[
                      { value: "say", label: tr("Chat ([Server])") },
                      { value: "tellraw", label: tr("Chat (no name)") },
                      { value: "title", label: tr("Big text on screen") },
                    ]}
                  />
                </div>
                <label className="field-block">
                  <span className="field-label">{tr("Warning text")}</span>
                  <input
                    value={draft.message}
                    placeholder={kind === "stop" ? tr("The server stops in {time}.") : tr("The server restarts in {time}.")}
                    onChange={(event) => set({ message: event.target.value })}
                  />
                  <small className="field-help">{tr("{time} becomes the time left, like 5 min or 10 sec.")}</small>
                </label>
                <label className="field-block">
                  <span className="field-label">{tr("Last message (optional)")}</span>
                  <input
                    value={draft.finalMessage}
                    placeholder={kind === "stop" ? tr("Stopping now.") : tr("Restarting now. Come back in a minute!")}
                    onChange={(event) => set({ finalMessage: event.target.value })}
                  />
                  <small className="field-help">{tr("Sent right before it happens. Leave empty to skip.")}</small>
                </label>
                <p className="task-warn-preview">
                  <span className="muted">{tr("Players will see")}</span>
                  {marks.slice(0, 3).map((seconds) => (
                    <code key={seconds}>{warnPreview(draft, seconds)}</code>
                  ))}
                  {draft.finalMessage.trim() && <code>{draft.finalMessage.trim()}</code>}
                </p>
              </>
            )}
          </div>
        );
      })()}
      {kind === "start" && <p className="field-help">{tr("Skipped if it's already running.")}</p>}
      {kind === "notify" && (
        <label className="field-block">
          <span className="field-label">{tr("Message")}</span>
          <input value={draft.message} placeholder={tr("e.g. Time to check the backups")} onChange={(event) => set({ message: event.target.value })} />
        </label>
      )}
      <div className="field-block">
        <span className="field-label">{tr("When")}</span>
        <SegmentedControl
          value={draft.schedule as "interval" | "daily" | "weekly"}
          onChange={(schedule) => set({ schedule, weekdays: schedule === "weekly" && !draft.weekdays.length ? [0, 1, 2, 3, 4, 5, 6] : draft.weekdays })}
          options={[
            { value: "daily", label: tr("Daily") },
            { value: "weekly", label: tr("On days") },
            { value: "interval", label: tr("Every few minutes") },
          ]}
        />
      </div>
      {draft.schedule === "weekly" && (
        <div className="task-days" role="group">
          {DAYS.map((day, index) => {
            const on = draft.weekdays.includes(index);
            return (
              <button
                type="button"
                key={day}
                className={`task-day ${on ? "on" : ""}`}
                aria-pressed={on}
                onClick={() => set({ weekdays: on ? draft.weekdays.filter((one) => one !== index) : [...draft.weekdays, index].sort() })}
              >
                {tr(day)}
              </button>
            );
          })}
        </div>
      )}
      {draft.schedule === "interval" ? (
        <label className="field-inline">
          <span>{tr("Interval")}</span>
          <input type="number" min="1" max="43200" value={draft.intervalMinutes} onChange={(event) => set({ intervalMinutes: Number(event.target.value) || 1 })} />
          {tr(" min")}
        </label>
      ) : (
        <label className="field-inline">
          <span>{tr("Time")}</span>
          <input type="time" value={draft.dailyTime} onChange={(event) => set({ dailyTime: event.target.value })} />
        </label>
      )}
      <div className="modal-actions">
        <button className="button ghost" onClick={onCancel}>
          {tr("Cancel")}
        </button>
        <button className="button primary" onClick={onSave}>
          {tr("Save")}
        </button>
      </div>
    </div>
  );
}

export function TasksPage({
  tasks,
  servers,
  draft,
  onDraft,
  onChanged,
  onNotice,
  onError,
  confirm,
}: {
  tasks: Task[];
  servers: Server[];
  draft: Task | null;
  onDraft: (task: Task | null) => void;
  onChanged: (list: Task[]) => void;
  onNotice: (message: string) => void;
  onError: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [running, setRunning] = useState<string | null>(null);
  const name = (id: string) => servers.find((one) => one.id === id)?.name ?? tr("Deleted server");

  const save = async (task: Task) => {
    try {
      const fallback = task.kind === "stop" ? tr("The server stops in {time}.") : task.kind === "restart" ? tr("The server restarts in {time}.") : "";
      onChanged(await commands.saveTask({ ...task, message: task.message.trim() || fallback, timeWords: [tr("{0} min"), tr("{0} sec")] }));
      onDraft(null);
      onNotice(tr("Saved {0}.", [task.name]));
    } catch (cause) {
      onError(String(cause));
    }
  };
  const toggle = async (task: Task, enabled: boolean) => {
    try {
      onChanged(await commands.saveTask({ ...task, enabled }));
    } catch (cause) {
      onError(String(cause));
    }
  };
  const remove = async (task: Task) => {
    if (!(await confirm(tr("Delete {0}?", [task.name]), tr("This task won't repeat. Its past actions won't be undone."), tr("Delete"), true))) return;
    try {
      onChanged(await commands.deleteTask(task.id));
    } catch (cause) {
      onError(String(cause));
    }
  };
  const runNow = async (task: Task) => {
    if (POWER.has(task.kind) && task.kind !== "start" && !(await confirm(tr("Do it now?"), whatText(task, name(task.serverId)), tr("Do it now")))) return;
    setRunning(task.id);
    try {
      onNotice(await commands.runTaskNow(task.id));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setRunning(null);
      onChanged(await commands.listTasks().catch(() => tasks));
    }
  };

  const ordered = [...tasks].sort((a, b) => (nextRun(a)?.getTime() ?? Infinity) - (nextRun(b)?.getTime() ?? Infinity));

  return (
    <div className="tasks-page">
      <div className="panel-head">
        <div>
          <h2>{tr("Tasks")}</h2>
          <p className="modal-copy">{tr("The app does these on schedule. It needs to be running.")}</p>
        </div>
        {!draft && (
          <button className="button primary" onClick={() => onDraft(blankTask())}>
            <Icon name="plus" />
            {tr("Add task")}
          </button>
        )}
      </div>
      {draft ? (
        <>
          {!tasks.some((one) => one.id === draft.id) && (
            <div className="task-templates">
              <span>{tr("Quick start")}</span>
              {TEMPLATES.map((one) => (
                <button key={one.label} type="button" className="chip-button" onClick={() => onDraft(blankTask({ ...one.make(), id: draft.id, serverId: draft.serverId || servers[0]?.id || "" }))}>
                  {tr(one.label)}
                </button>
              ))}
            </div>
          )}
          <TaskForm draft={draft} servers={servers} onChange={onDraft} onCancel={() => onDraft(null)} onSave={() => void save(draft)} />
        </>
      ) : tasks.length ? (
        <div className="task-list">
          {ordered.map((task) => {
            const next = nextRun(task);
            return (
              <div className={`task-row ${task.enabled ? "" : "off"}`} key={task.id}>
                <label className="switch" aria-label={tr("Enable")}>
                  <input type="checkbox" checked={task.enabled} onChange={(event) => void toggle(task, event.target.checked)} />
                  <span />
                </label>
                <div className="task-body">
                  <strong>{task.name}</strong>
                  <small>
                    {whatText(task, name(task.serverId))} · {whenText(task)}
                  </small>
                  {task.lastRunAt > 0 && task.lastResult && (
                    <small className={`task-last ${task.lastOk ? "ok" : "bad"}`} data-tip={task.lastResult}>
                      <Icon name={task.lastOk ? "check" : "alert"} />
                      {relativeTime(task.lastRunAt)} · {task.lastResult}
                    </small>
                  )}
                </div>
                <span className="task-next">{nextText(next)}</span>
                <button className="button ghost small" disabled={running === task.id} onClick={() => void runNow(task)}>
                  {running === task.id ? tr("Running…") : tr("Do it now")}
                </button>
                <button className="icon-button" aria-label={tr("Edit")} data-tip={tr("Edit")} onClick={() => onDraft(task)}>
                  <Icon name="pencil" />
                </button>
                <button className="icon-button danger" aria-label={tr("Delete")} data-tip={tr("Delete")} onClick={() => void remove(task)}>
                  <Icon name="trash" />
                </button>
              </div>
            );
          })}
        </div>
      ) : (
        <EmptyState
          icon="refresh"
          title={tr("No tasks yet.")}
          hint={tr("Back up, restart or run commands on a schedule. For example: back up every day at 4 AM.")}
          action={
            <button className="button secondary small" onClick={() => onDraft(blankTask())}>
              <Icon name="plus" />
              {tr("Add task")}
            </button>
          }
        />
      )}
    </div>
  );
}
