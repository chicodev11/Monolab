/* eslint-disable @typescript-eslint/no-explicit-any */
import { useCallback, useEffect, useMemo, useState } from "react";
import { date, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { isoSecs, pcall, serverBase, type Json } from "../../lib/panelApi";
import { DAYS, describeCron, fromCron, toCron, type Cron, type Easy } from "../../lib/cron";
import { Badge, Banner, Button, EmptyState, IconButton, Select, SegmentedControl, Skeleton } from "../primitives";
import { Icon } from "../ui/Icon";
import { PanelStartupCard } from "./PanelStartupCard";
import { TimePicker } from "../ui/TimePicker";
import { Dropdown } from "../ui/Dropdown";

export type PanelView = "databases" | "network" | "subusers" | "startup" | "schedules" | "psettings" | "activity" | "software";

type Ask = {
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  ask: (title: string, label: string, value: string, confirmLabel: string, danger?: boolean) => Promise<string | null>;
};

export function PanelServerTools({
  view,
  instanceId,
  panelName,
  hostId,
  daemon,
  ident,
  name,
  onError,
  onNotice,
  onRenamed,
  confirm,
  ask,
}: {
  view: PanelView;
  instanceId: string;
  panelName: string;
  hostId: string;
  daemon: string;
  ident: string;
  name: string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onRenamed: () => void;
} & Ask) {
  const base = serverBase(daemon, ident);
  const props = { hostId, base, ident, onError, onNotice, confirm, ask };
  return (
    <div className="pt-page">
      <div className="pt-body" key={view}>
        {view === "schedules" && <Schedules {...props} />}
        {view === "databases" && <Databases {...props} />}
        {view === "network" && <Network {...props} />}
        {view === "subusers" && <Subusers {...props} />}
        {view === "startup" && <Startup {...props} instanceId={instanceId} panelName={panelName} />}
        {view === "psettings" && <Settings {...props} name={name} onRenamed={onRenamed} />}
        {view === "activity" && <Activity {...props} />}
        {view === "software" && <Software {...props} />}
      </div>
    </div>
  );
}

type Part = {
  hostId: string;
  base: string;
  ident: string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
} & Ask;

function useRows(hostId: string, path: string, pick: (got: any) => Json[], onError: (message: string) => void) {
  const [rows, setRows] = useState<Json[] | null>(null);
  const load = useCallback(async () => {
    try {
      const got = await pcall(hostId, "GET", path);
      setRows(pick(got));
    } catch (cause) {
      setRows([]);
      onError(String(cause));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId, path]);
  useEffect(() => {
    void load();
  }, [load]);
  return { rows, load };
}

const attrs = (got: any): Json[] => (got?.data ?? []).map((one: Json) => one.attributes ?? one);

async function act(part: Part, work: () => Promise<unknown>, done: string, reload?: () => Promise<void>) {
  try {
    await work();
    if (done) part.onNotice(done);
    if (reload) await reload();
  } catch (cause) {
    part.onError(String(cause));
  }
}

const ACTIONS: { value: string; label: string }[] = [
  { value: "command", label: "Command" },
  { value: "power", label: "Power" },
  { value: "backup", label: "Backups" },
];

function Schedules(part: Part) {
  const { rows, load } = useRows(part.hostId, `${part.base}/schedules?include=tasks`, attrs, part.onError);
  const [draft, setDraft] = useState<Json | null>(null);
  const [taskFor, setTaskFor] = useState<Json | null>(null);

  const save = async () => {
    if (!draft) return;
    const body = {
      name: draft.name,
      minute: draft.minute,
      hour: draft.hour,
      day_of_month: draft.day_of_month,
      month: draft.month,
      day_of_week: draft.day_of_week,
      is_active: draft.is_active,
      only_when_online: draft.only_when_online,
    };
    await act(
      part,
      () => pcall(part.hostId, "POST", draft.id ? `${part.base}/schedules/${draft.id}` : `${part.base}/schedules`, body),
      tr("Saved the schedule."),
      load,
    );
    setDraft(null);
  };

  if (!rows) return <Skeleton height={120} />;
  return (
    <>
      <div className="pt-head">
        <p className="muted">{tr("The panel runs commands, power actions and backups on schedule, even while this app is closed.")}</p>
        {!draft && (
          <Button
            variant="primary"
            icon="plus"
            onClick={() =>
              setDraft({ name: "", minute: "0", hour: "4", day_of_month: "*", month: "*", day_of_week: "*", is_active: true, only_when_online: false })
            }
          >
            {tr("Add schedule")}
          </Button>
        )}
      </div>
      {draft && (
        <div className="pt-form">
          <label className="field-block">
            <span className="field-label">{tr("Name")}</span>
            <input autoFocus value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} />
          </label>
          <WhenPicker value={draft as Cron} onChange={(cron) => setDraft({ ...draft, ...cron })} />
          <label className="pt-check">
            <input type="checkbox" checked={draft.is_active} onChange={(event) => setDraft({ ...draft, is_active: event.target.checked })} />
            {tr("Enabled")}
          </label>
          <label className="pt-check">
            <input
              type="checkbox"
              checked={draft.only_when_online}
              onChange={(event) => setDraft({ ...draft, only_when_online: event.target.checked })}
            />
            {tr("Only while the server is online")}
          </label>
          <div className="pt-actions">
            <Button variant="ghost" onClick={() => setDraft(null)}>
              {tr("Cancel")}
            </Button>
            <Button variant="primary" disabled={!draft.name.trim()} onClick={() => void save()}>
              {tr("Save")}
            </Button>
          </div>
        </div>
      )}
      {taskFor && <TaskForm part={part} schedule={taskFor} onDone={() => { setTaskFor(null); void load(); }} />}
      {!rows.length && !draft ? (
        <EmptyState icon="refresh" title={tr("No schedules")} hint={tr("Add a schedule for the panel to handle automatic restarts or backups.")} />
      ) : (
        <div className="pt-list">
          {rows.map((one) => {
            const cron = one.cron ?? {};
            const tasks: Json[] = (one.relationships?.tasks?.data ?? []).map((task: Json) => task.attributes ?? task);
            return (
              <div className="pt-row pt-block" key={one.id}>
                <div className="pt-row-main">
                  <strong>
                    {one.name} {!one.is_active && <Badge tone="subtle">{tr("Off")}</Badge>}
                    {one.is_processing && <Badge tone="warn">{tr("Running")}</Badge>}
                  </strong>
                  <span className="muted" data-tip={`cron: ${cron.minute} ${cron.hour} ${cron.day_of_month} ${cron.month} ${cron.day_of_week}`}>
                    {describeCron(cron)}
                  </span>
                  <small className="muted">
                    {tr("Next")} {one.next_run_at ? date(isoSecs(one.next_run_at)) : "—"} · {tr("Last")}{" "}
                    {one.last_run_at ? relativeTime(isoSecs(one.last_run_at)) : "—"}
                  </small>
                  {tasks.length > 0 && (
                    <ol className="pt-tasks">
                      {tasks
                        .sort((a, b) => a.sequence_id - b.sequence_id)
                        .map((task) => (
                          <li key={task.id}>
                            <Badge>{tr(ACTIONS.find((each) => each.value === task.action)?.label ?? task.action)}</Badge>
                            <span className="mono">{task.payload}</span>
                            {task.time_offset > 0 && <small className="muted">+{task.time_offset}s</small>}
                            <IconButton
                              label={tr("Delete task")}
                              icon="trash"
                              onClick={() =>
                                void act(part, () => pcall(part.hostId, "DELETE", `${part.base}/schedules/${one.id}/tasks/${task.id}`), "", load)
                              }
                            />
                          </li>
                        ))}
                    </ol>
                  )}
                </div>
                <div className="pt-row-side">
                  <IconButton label={tr("Add task")} icon="plus" onClick={() => setTaskFor(one)} />
                  <IconButton
                    label={tr("Run now")}
                    icon="play"
                    onClick={() => void act(part, () => pcall(part.hostId, "POST", `${part.base}/schedules/${one.id}/execute`), tr("Schedule executed."), load)}
                  />
                  <IconButton
                    label={tr("Edit")}
                    icon="pencil"
                    onClick={() => setDraft({ id: one.id, name: one.name, ...cron, is_active: one.is_active, only_when_online: one.only_when_online })}
                  />
                  <IconButton
                    label={tr("Delete")}
                    icon="trash"
                    onClick={async () => {
                      if (!(await part.confirm(tr("Delete this schedule?"), one.name, tr("Delete"), true))) return;
                      await act(part, () => pcall(part.hostId, "DELETE", `${part.base}/schedules/${one.id}`), tr("Deleted the schedule."), load);
                    }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </>
  );
}

function WhenPicker({ value, onChange }: { value: Cron; onChange: (cron: Cron) => void }) {
  const [easy, setEasy] = useState<Easy>(() => fromCron(value));
  const set = (next: Easy) => {
    setEasy(next);
    if (next.kind !== "custom") onChange(toCron(next, value));
  };
  const time = (hour: number, minute: number, patch: (hour: number, minute: number) => Easy) => (
    <TimePicker
      value={`${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`}
      onChange={(next) => {
        const [h, m] = next.split(":").map(Number);
        if (!Number.isNaN(h) && !Number.isNaN(m)) set(patch(h, m));
      }}
    />
  );
  return (
    <div className="pt-when">
      <SegmentedControl
        value={easy.kind}
        onChange={(kind) => {
          if (kind === "daily") set({ kind, hour: 4, minute: 0 });
          else if (kind === "hours") set({ kind, every: 6 });
          else if (kind === "minutes") set({ kind, every: 30 });
          else if (kind === "weekly") set({ kind, days: [0], hour: 4, minute: 0 });
          else setEasy({ kind: "custom" });
        }}
        options={[
          { value: "daily", label: tr("Daily") },
          { value: "weekly", label: tr("Weekly") },
          { value: "hours", label: tr("Every few hours") },
          { value: "minutes", label: tr("Every few minutes") },
          { value: "custom", label: tr("Custom (cron)") },
        ]}
      />
      <div className="pt-when-body">
        {easy.kind === "daily" && (
          <>
            {tr("Daily")} {time(easy.hour, easy.minute, (hour, minute) => ({ kind: "daily", hour, minute }))} {tr("suffix.at")}
          </>
        )}
        {easy.kind === "weekly" && (
          <>
            <span className="pt-days">
              {DAYS.map((label, day) => (
                <button
                  key={label}
                  type="button"
                  className={easy.days.includes(day) ? "on" : ""}
                  onClick={() =>
                    set({ ...easy, days: easy.days.includes(day) ? easy.days.filter((one) => one !== day) : [...easy.days, day] })
                  }
                >
                  {tr(label)}
                </button>
              ))}
            </span>
            {time(easy.hour, easy.minute, (hour, minute) => ({ ...easy, hour, minute }))} {tr("suffix.at")}
          </>
        )}
        {easy.kind === "hours" && (
          <>
            <Dropdown
              className="pt-num"
              value={String(easy.every)}
              options={[1, 2, 3, 4, 6, 8, 12].map((one) => ({ value: String(one), label: String(one) }))}
              onChange={(value) => set({ kind: "hours", every: Number(value) })}
            />
            {tr("hours (on the hour)")}
          </>
        )}
        {easy.kind === "minutes" && (
          <>
            <Dropdown
              className="pt-num"
              value={String(easy.every)}
              options={[5, 10, 15, 20, 30].map((one) => ({ value: String(one), label: String(one) }))}
              onChange={(value) => set({ kind: "minutes", every: Number(value) })}
            />
            {tr("minutes")}
          </>
        )}
        {easy.kind === "custom" && (
          <div className="pt-cron">
            {(["minute", "hour", "day_of_month", "month", "day_of_week"] as const).map((key) => (
              <label className="field-block" key={key}>
                <span className="field-label">
                  {{ minute: tr(" min"), hour: tr("Hour"), day_of_month: tr(" days"), month: tr("Month"), day_of_week: tr("Weekday") }[key]}
                </span>
                <input className="mono" value={value[key]} onChange={(event) => onChange({ ...value, [key]: event.target.value })} />
              </label>
            ))}
          </div>
        )}
      </div>
      <small className="muted">
        {describeCron(value)} · <span className="mono">{value.minute} {value.hour} {value.day_of_month} {value.month} {value.day_of_week}</span>
      </small>
    </div>
  );
}

function TaskForm({ part, schedule, onDone }: { part: Part; schedule: Json; onDone: () => void }) {
  const [action, setAction] = useState("command");
  const [payload, setPayload] = useState("");
  const [offset, setOffset] = useState("0");
  return (
    <div className="pt-form">
      <strong>
        {schedule.name} · {tr("Add task")}
      </strong>
      <SegmentedControl
        value={action}
        onChange={(value) => {
          setAction(value);
          setPayload(value === "power" ? "restart" : "");
        }}
        options={ACTIONS.map((one) => ({ value: one.value, label: tr(one.label) }))}
      />
      {action === "power" ? (
        <SegmentedControl
          value={payload || "restart"}
          onChange={setPayload}
          options={["start", "stop", "restart", "kill"].map((one) => ({ value: one, label: one }))}
        />
      ) : (
        <label className="field-block">
          <span className="field-label">{action === "command" ? tr("Command") : tr("Files to exclude from backups (one per line, optional)")}</span>
          <input className="mono" value={payload} onChange={(event) => setPayload(event.target.value)} placeholder={action === "command" ? "say hello" : ""} />
        </label>
      )}
      <label className="field-inline">
        <span className="field-label">{tr("Seconds after the previous task")}</span>
        <input type="number" min={0} max={900} value={offset} onChange={(event) => setOffset(event.target.value)} />
      </label>
      <div className="pt-actions">
        <Button variant="ghost" onClick={onDone}>
          {tr("Cancel")}
        </Button>
        <Button
          variant="primary"
          disabled={action === "command" && !payload.trim()}
          onClick={() =>
            void act(
              part,
              () =>
                pcall(part.hostId, "POST", `${part.base}/schedules/${schedule.id}/tasks`, {
                  action,
                  payload: action === "power" ? payload || "restart" : payload,
                  time_offset: Number(offset) || 0,
                  continue_on_failure: false,
                }),
              tr("Added the task."),
            ).then(onDone)
          }
        >
          {tr("Add")}
        </Button>
      </div>
    </div>
  );
}

function Databases(part: Part) {
  const { rows, load } = useRows(part.hostId, `${part.base}/databases?include=password`, attrs, part.onError);
  const [shown, setShown] = useState<string>("");
  if (!rows) return <Skeleton height={120} />;
  const copy = (text: string) => {
    void navigator.clipboard.writeText(text);
    part.onNotice(tr("Copied."));
  };
  return (
    <>
      <div className="pt-head">
        <p className="muted">{tr("MySQL databases for plugins. They are created on the panel's database host.")}</p>
        <Button
          variant="primary"
          icon="plus"
          onClick={async () => {
            const name = await part.ask(tr("New database"), tr("Name"), "", tr("Create"));
            if (!name?.trim()) return;
            await act(part, () => pcall(part.hostId, "POST", `${part.base}/databases`, { database: name.trim(), remote: "%" }), tr("Created the database."), load);
          }}
        >
          {tr("Add database")}
        </Button>
      </div>
      {!rows.length ? (
        <EmptyState icon="archive" title={tr("No databases")} hint={tr("For plugins that use MySQL, such as LuckPerms or CoreProtect, create a database and add the connection details to their settings.")} />
      ) : (
        <div className="pt-list">
          {rows.map((one) => {
            const password: string = one.relationships?.password?.attributes?.password ?? "";
            const address = `${one.host?.address ?? ""}:${one.host?.port ?? ""}`;
            return (
              <div className="pt-row" key={one.id}>
                <div className="pt-row-main">
                  <strong className="mono">{one.name}</strong>
                  <span className="pt-kv">
                    <span>{tr("Address")}</span>
                    <button className="mono pt-copy" onClick={() => copy(address)}>{address}</button>
                    <span>{tr("User")}</span>
                    <button className="mono pt-copy" onClick={() => copy(one.username)}>{one.username}</button>
                    <span>{tr("Password")}</span>
                    <button className="mono pt-copy" onClick={() => (shown === one.id ? copy(password) : setShown(one.id))}>
                      {shown === one.id ? password || "—" : "••••••••"}
                    </button>
                    <span>{tr("Allowed connections")}</span>
                    <span className="mono">{one.connections_from}</span>
                  </span>
                </div>
                <div className="pt-row-side">
                  <IconButton
                    label={tr("Change password")}
                    icon="refresh"
                    onClick={async () => {
                      if (!(await part.confirm(tr("Generate a new password?"), tr("The old password in your plugin settings will no longer work."), tr("Change")))) return;
                      await act(part, () => pcall(part.hostId, "POST", `${part.base}/databases/${one.id}/rotate-password`), tr("Changed the password."), load);
                    }}
                  />
                  <IconButton
                    label={tr("Delete")}
                    icon="trash"
                    onClick={async () => {
                      if (!(await part.confirm(tr("Delete this database?"), tr("All data inside will be deleted. This can't be undone."), tr("Delete"), true))) return;
                      await act(part, () => pcall(part.hostId, "DELETE", `${part.base}/databases/${one.id}`), tr("Deleted the database."), load);
                    }}
                  />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </>
  );
}

function Network(part: Part) {
  const { rows, load } = useRows(part.hostId, `${part.base}/network/allocations`, attrs, part.onError);
  if (!rows) return <Skeleton height={120} />;
  return (
    <>
      <div className="pt-head">
        <p className="muted">{tr("Ports assigned to this server. The primary port is used for Minecraft connections.")}</p>
        <Button
          variant="primary"
          icon="plus"
          onClick={() => void act(part, () => pcall(part.hostId, "POST", `${part.base}/network/allocations`), tr("One more port assigned."), load)}
        >
          {tr("Add port")}
        </Button>
      </div>
      <div className="pt-list">
        {rows.map((one) => (
          <div className="pt-row" key={one.id}>
            <div className="pt-row-main">
              <strong className="mono">
                {one.ip_alias || one.ip}:{one.port} {one.is_default && <Badge tone="ok">{tr("Default")}</Badge>}
              </strong>
              <small className="muted">{one.notes || tr("No note")}</small>
            </div>
            <div className="pt-row-side">
              <IconButton
                label={tr("Note")}
                icon="pencil"
                onClick={async () => {
                  const notes = await part.ask(tr("Port note"), tr("Note"), one.notes ?? "", tr("Save"));
                  if (notes === null) return;
                  await act(part, () => pcall(part.hostId, "POST", `${part.base}/network/allocations/${one.id}`, { notes }), "", load);
                }}
              />
              {!one.is_default && (
                <>
                  <IconButton
                    label={tr("Make primary")}
                    icon="check"
                    onClick={() =>
                      void act(part, () => pcall(part.hostId, "POST", `${part.base}/network/allocations/${one.id}/primary`), tr("Primary port changed. Applies on the next start."), load)
                    }
                  />
                  <IconButton
                    label={tr("Delete")}
                    icon="trash"
                    onClick={async () => {
                      if (!(await part.confirm(tr("Remove this port?"), `${one.ip}:${one.port}`, tr("Unassign"), true))) return;
                      await act(part, () => pcall(part.hostId, "DELETE", `${part.base}/network/allocations/${one.id}`), tr("Removed the port."), load);
                    }}
                  />
                </>
              )}
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function Subusers(part: Part) {
  const { rows, load } = useRows(part.hostId, `${part.base}/users`, attrs, part.onError);
  const [catalog, setCatalog] = useState<Record<string, { description: string; keys: Record<string, string> }> | null>(null);
  const [draft, setDraft] = useState<{ uuid?: string; email: string; permissions: string[] } | null>(null);
  useEffect(() => {
    pcall(part.hostId, "GET", "/api/client/permissions")
      .then((got: any) => setCatalog(got?.attributes?.permissions ?? {}))
      .catch(() => setCatalog({}));
  }, [part.hostId]);
  if (!rows) return <Skeleton height={120} />;
  const toggle = (key: string) =>
    draft &&
    setDraft({ ...draft, permissions: draft.permissions.includes(key) ? draft.permissions.filter((one) => one !== key) : [...draft.permissions, key] });
  const save = async () => {
    if (!draft) return;
    const path = draft.uuid ? `${part.base}/users/${draft.uuid}` : `${part.base}/users`;
    await act(part, () => pcall(part.hostId, "POST", path, { email: draft.email, permissions: draft.permissions }), tr("Saved the subuser."), load);
    setDraft(null);
  };
  return (
    <>
      <div className="pt-head">
        <p className="muted">{tr("Invite a friend with a panel account to this server only. Choose their permissions.")}</p>
        {!draft && (
          <Button variant="primary" icon="plus" onClick={() => setDraft({ email: "", permissions: ["control.console", "control.start", "control.stop", "control.restart"] })}>
            {tr("Add user")}
          </Button>
        )}
      </div>
      {draft && (
        <div className="pt-form">
          <label className="field-block">
            <span className="field-label">{tr("Email")}</span>
            <input autoFocus disabled={!!draft.uuid} value={draft.email} onChange={(event) => setDraft({ ...draft, email: event.target.value })} />
          </label>
          <div className="pt-perms">
            {Object.entries(catalog ?? {})
              .filter(([group]) => group !== "websocket")
              .map(([group, info]) => (
                <fieldset key={group}>
                  <legend>{group}</legend>
                  {Object.entries(info.keys ?? {}).map(([key, about]) => {
                    const full = `${group}.${key}`;
                    return (
                      <label className="pt-check" key={full} data-tip={about}>
                        <input type="checkbox" checked={draft.permissions.includes(full)} onChange={() => toggle(full)} />
                        {key}
                      </label>
                    );
                  })}
                </fieldset>
              ))}
          </div>
          <div className="pt-actions">
            <Button variant="ghost" onClick={() => setDraft(null)}>
              {tr("Cancel")}
            </Button>
            <Button variant="primary" disabled={!draft.email.includes("@")} onClick={() => void save()}>
              {tr("Save")}
            </Button>
          </div>
        </div>
      )}
      {!rows.length && !draft ? (
        <EmptyState icon="list" title={tr("No subusers")} hint={tr("Add a friend to give them access to this server's console or files.")} />
      ) : (
        <div className="pt-list">
          {rows.map((one) => (
            <div className="pt-row" key={one.uuid}>
              <div className="pt-row-main">
                <strong>
                  {one.username} <small className="muted">{one.email}</small>
                </strong>
                <small className="muted">{tr("{0, plural, one {# permission} other {# permissions}}", [(one.permissions ?? []).length])}</small>
              </div>
              <div className="pt-row-side">
                <IconButton label={tr("Edit permissions")} icon="pencil" onClick={() => setDraft({ uuid: one.uuid, email: one.email, permissions: one.permissions ?? [] })} />
                <IconButton
                  label={tr("Delete")}
                  icon="trash"
                  onClick={async () => {
                    if (!(await part.confirm(tr("Remove this subuser?"), one.email, tr("Remove"), true))) return;
                    await act(part, () => pcall(part.hostId, "DELETE", `${part.base}/users/${one.uuid}`), tr("Removed the subuser."), load);
                  }}
                />
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

function useServerRecord(part: Part) {
  const [record, setRecord] = useState<Json | null>(null);
  const load = useCallback(async () => {
    try {
      const got = await pcall<any>(part.hostId, "GET", `/api/client/servers/${part.ident}`);
      setRecord(got?.attributes ?? null);
    } catch (cause) {
      part.onError(String(cause));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [part.hostId, part.ident]);
  useEffect(() => {
    void load();
  }, [load]);
  return { record, reload: load };
}

function Startup(part: Part & { instanceId: string; panelName: string }) {
  const { record, reload } = useServerRecord(part);
  const [images, setImages] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    pcall<any>(part.hostId, "GET", `${part.base}/startup`)
      .then((got) => setImages(got?.meta?.docker_images ?? {}))
      .catch((cause) => part.onError(String(cause)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [part.hostId, part.base]);
  const current: string = record?.docker_image ?? "";
  const options = Object.entries(images).map(([label, image]) => ({ value: image, label, hint: image }));
  if (current && !options.some((one) => one.value === current)) options.unshift({ value: current, label: current, hint: tr("Image not listed by the egg") });
  return (
    <>
      <div className="pt-row">
        <div className="pt-row-main">
          <strong>
            <Icon name="package" /> {tr("Docker image")}
          </strong>
          <small className="muted">{tr("The Java version used to run the server. Changes apply on the next start.")}</small>
        </div>
        <div className="pt-select">
          <Select
            value={current}
            options={options}
            disabled={busy || !options.length}
            onChange={async (image: string) => {
              if (image === current) return;
              const label = Object.entries(images).find(([, one]) => one === image)?.[0] ?? image;
              if (!(await part.confirm(tr("Change the Docker image?"), `${label} — ${tr("Uses this Java version on the next start.")}`, tr("Change")))) return;
              setBusy(true);
              await act(part, () => pcall(part.hostId, "PUT", `${part.base}/settings/docker-image`, { docker_image: image }), tr("Changed the Docker image."), reload);
              setBusy(false);
            }}
          />
        </div>
      </div>
      <PanelStartupCard id={part.instanceId} panelName={part.panelName} onError={part.onError} onNotice={part.onNotice} />
    </>
  );
}

function Settings(part: Part & { name: string; onRenamed: () => void }) {
  const { record, reload } = useServerRecord(part);
  const [user, setUser] = useState("");
  useEffect(() => {
    pcall<any>(part.hostId, "GET", "/api/client/account")
      .then((got) => setUser(got?.attributes?.username ?? ""))
      .catch(() => setUser(""));
  }, [part.hostId]);
  const sftp = record?.sftp_details ?? {};
  const address = sftp.ip ? `sftp://${sftp.ip}:${sftp.port}` : "";
  const login = user ? `${user}.${part.ident}` : "";
  const copy = (text: string) => {
    void navigator.clipboard.writeText(text);
    part.onNotice(tr("Copied."));
  };
  return (
    <div className="pt-settings">
      <div className="pt-row">
        <div className="pt-row-main">
          <strong>{tr("Server name")}</strong>
          <small className="muted">
            {record?.name ?? part.name}
            {record?.description ? ` · ${record.description}` : ""}
          </small>
        </div>
        <Button
          variant="secondary"
          onClick={async () => {
            const name = await part.ask(tr("Rename server"), tr("New name"), record?.name ?? part.name, tr("Change"));
            if (!name?.trim() || name === (record?.name ?? part.name)) return;
            await act(part, () => pcall(part.hostId, "POST", `${part.base}/settings/rename`, { name: name.trim() }), tr("Name updated on the panel."), reload);
            part.onRenamed();
          }}
        >
          {tr("Rename")}
        </Button>
        <Button
          variant="ghost"
          onClick={async () => {
            const description = await part.ask(tr("Server description"), tr("Description"), record?.description ?? "", tr("Save"));
            if (description === null) return;
            await act(part, () => pcall(part.hostId, "POST", `${part.base}/settings/rename`, { name: record?.name ?? part.name, description }), tr("Description updated."), reload);
          }}
        >
          {tr("Description")}
        </Button>
      </div>
      <div className="pt-row pt-block">
        <div className="pt-row-main">
          <strong>{tr("SFTP details")}</strong>
          <small className="muted">{tr("Use these details to connect with an app such as FileZilla. Use your panel account password. You don't need to enter these details in the app's SFTP tab.")}</small>
          <span className="pt-kv">
            <span>{tr("Address")}</span>
            {address ? (
              <button className="mono pt-copy" onClick={() => copy(address)}>{address}</button>
            ) : (
              <span>—</span>
            )}
            <span>{tr("User")}</span>
            {login ? (
              <button className="mono pt-copy" onClick={() => copy(login)}>{login}</button>
            ) : (
              <span>—</span>
            )}
          </span>
        </div>
      </div>
      <div className="pt-row">
        <div className="pt-row-main">
          <strong>{tr("Reinstall")}</strong>
          <small className="muted">{tr("Runs the egg's install script again. Server files may change.")}</small>
        </div>
        <Button
          variant="danger"
          onClick={async () => {
            const typed = await part.ask(tr("Reinstall?"), tr("Type the server name exactly to confirm"), "", tr("Reinstall"), true);
            if (typed === null) return;
            if (typed.trim() !== (record?.name ?? part.name)) {
              part.onError(tr("The server name didn't match. Nothing was reinstalled."));
              return;
            }
            await act(part, () => pcall(part.hostId, "POST", `${part.base}/settings/reinstall`), tr("Reinstallation started."));
          }}
        >
          {tr("Reinstall")}
        </Button>
      </div>
    </div>
  );
}

type Step = 0 | 1 | 2 | 3;

function Software(part: Part) {
  const { record, reload } = useServerRecord(part);
  const [nests, setNests] = useState<Json[] | null>(null);
  const [step, setStep] = useState<Step>(0);
  const [nest, setNest] = useState<Json | null>(null);
  const [egg, setEgg] = useState<Json | null>(null);
  const [preview, setPreview] = useState<Json | null>(null);
  const [image, setImage] = useState("");
  const [startup, setStartup] = useState("");
  const [env, setEnv] = useState<Record<string, string>>({});
  const [currentVars, setCurrentVars] = useState<Record<string, string>>({});
  const [backup, setBackup] = useState(true);
  const [wipe, setWipe] = useState(false);
  const [busy, setBusy] = useState<"" | "preview" | "apply" | "waiting">("");

  useEffect(() => {
    pcall<any>(part.hostId, "GET", "/api/client/nests?include=eggs")
      .then((got) => setNests((got?.data ?? []).map((one: Json) => one.attributes ?? one)))
      .catch((cause) => {
        setNests([]);
        part.onError(String(cause));
      });
    pcall<any>(part.hostId, "GET", `${part.base}/startup`)
      .then((got) =>
        setCurrentVars(Object.fromEntries((got?.data ?? []).map((one: Json) => [one.attributes?.env_variable, one.attributes?.server_value ?? ""]))),
      )
      .catch(() => setCurrentVars({}));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [part.hostId, part.base]);

  const eggsOf = (one: Json): Json[] => (one.relationships?.eggs?.data ?? []).map((each: Json) => each.attributes ?? each);
  const currentEgg = useMemo(() => {
    for (const one of nests ?? []) {
      const found = eggsOf(one).find((each) => each.uuid === record?.egg || String(each.id) === String(record?.egg));
      if (found) return { nest: one, egg: found };
    }
    return null;
  }, [nests, record]);

  const pick = async (chosen: Json) => {
    if (!nest) return;
    setEgg(chosen);
    setBusy("preview");
    try {
      const got = await pcall<any>(part.hostId, "POST", `${part.base}/settings/egg/preview`, { egg_id: chosen.id, nest_id: nest.id });
      setPreview(got);
      const images: Record<string, string> = got?.docker_images ?? {};
      const values = Object.values(images);
      const javaOf = (label: string) => Number(label.replace(/\D+/g, " ").trim().split(" ").pop()) || 0;
      const newest = Object.entries(images).sort((a, b) => javaOf(b[0]) - javaOf(a[0]))[0]?.[1] ?? "";
      setImage(values.includes(record?.docker_image) ? record?.docker_image : newest);
      setStartup(got?.egg?.startup ?? "");
      setEnv(
        Object.fromEntries(
          (got?.variables ?? []).map((one: Json) => [one.env_variable, currentVars[one.env_variable] ?? one.default_value ?? ""]),
        ),
      );
      setStep(2);
    } catch (cause) {
      part.onError(String(cause));
    } finally {
      setBusy("");
    }
  };

  const apply = async () => {
    if (!nest || !egg) return;
    const body = {
      egg_id: egg.id,
      nest_id: nest.id,
      docker_image: image,
      startup_command: startup,
      environment: env,
      should_backup: backup,
      should_wipe: wipe,
    };
    setBusy("apply");
    try {
      await pcall(part.hostId, "POST", `${part.base}/settings/egg/apply`, body);
    } catch (cause) {
      part.onError(String(cause));
      setBusy("");
      return;
    }
    setBusy("waiting");
    part.onNotice(tr("The panel is changing the software. You'll be notified when it's done."));
    for (let tries = 0; tries < 100; tries++) {
      await new Promise((done) => window.setTimeout(done, 3000));
      try {
        const got = await pcall<any>(part.hostId, "GET", `/api/client/servers/${part.ident}`);
        if (got?.attributes?.egg === egg.uuid) {
          part.onNotice(tr("Switched to {0}.", [egg.name]));
          await reload();
          setStep(0);
          setNest(null);
          setEgg(null);
          setBusy("");
          return;
        }
      } catch {}
    }
    setBusy("");
    part.onError(tr("The panel is still changing the software. Check the Activity tab shortly."));
  };

  if (!nests || !record) return <Skeleton height={160} />;
  const steps = [tr("Category"), tr("Software"), tr("Settings"), tr("Review changes")];
  const variables: Json[] = preview?.variables ?? [];
  const images: Record<string, string> = preview?.docker_images ?? {};
  const imageLabel = (value: string) => Object.entries(images).find(([, one]) => one === value)?.[0] ?? value;
  const changedVars = variables.filter((one) => (currentVars[one.env_variable] ?? "") !== (env[one.env_variable] ?? ""));

  return (
    <div className="sw-page">
      <div className="sw-now">
        <span className="muted">{tr("Now")}</span>
        <strong>{currentEgg ? `${currentEgg.nest.name} · ${currentEgg.egg.name}` : tr("Unknown")}</strong>
        <span className="mono muted">{record.docker_image}</span>
      </div>
      <ol className="sw-steps">
        {steps.map((label, index) => (
          <li key={label} className={index === step ? "on" : index < step ? "done" : ""}>
            <button
              type="button"
              disabled={index > step || !!busy}
              onClick={() => setStep(index as Step)}
            >
              <span className="sw-num">{index < step ? <Icon name="check" /> : index + 1}</span>
              {label}
            </button>
          </li>
        ))}
      </ol>

      {step === 0 && (
        <div className="sw-grid">
          {nests.map((one) => (
            <button
              key={one.id}
              type="button"
              className={`sw-card ${currentEgg?.nest.id === one.id ? "current" : ""}`}
              onClick={() => {
                setNest(one);
                setStep(1);
              }}
            >
              <strong>
                {one.name} {currentEgg?.nest.id === one.id && <Badge tone="ok">{tr("Now")}</Badge>}
              </strong>
              {one.description && <small className="muted sw-desc">{one.description}</small>}
              <small className="muted">{tr("{0, plural, one {# software} other {# software}}", [eggsOf(one).length])}</small>
            </button>
          ))}
        </div>
      )}

      {step === 1 && nest && (
        <div className="sw-grid">
          {eggsOf(nest).map((one) => {
            const now = currentEgg?.egg.id === one.id;
            return (
              <button key={one.id} type="button" className={`sw-card ${now ? "current" : ""}`} disabled={!!busy} onClick={() => void pick(one)}>
                <strong>
                  {one.name} {now && <Badge tone="ok">{tr("Now")}</Badge>}
                </strong>
                <small className="muted sw-desc">{one.description || "—"}</small>
                {busy === "preview" && egg?.id === one.id && <small className="muted">{tr("Loading…")}</small>}
              </button>
            );
          })}
        </div>
      )}

      {step === 2 && egg && preview && (
        <div className="pt-form">
          {(preview.warnings ?? []).map((one: string) => (
            <Banner tone="warn" key={one}>
              {one}
            </Banner>
          ))}
          <label className="field-block">
            <span className="field-label">{tr("Docker image")}</span>
            <Select value={image} options={Object.entries(images).map(([label, value]) => ({ value, label, hint: value }))} onChange={setImage} />
          </label>
          <label className="field-block">
            <span className="field-label">{tr("Startup command")}</span>
            <textarea className="mono sw-startup" value={startup} onChange={(event) => setStartup(event.target.value)} />
          </label>
          {variables
            .filter((one) => one.user_viewable !== false)
            .map((one) => (
              <label className="field-block" key={one.env_variable}>
                <span className="field-label">
                  {one.name} <span className="mono muted">{one.env_variable}</span>
                </span>
                <input
                  className="mono"
                  value={env[one.env_variable] ?? ""}
                  disabled={one.user_editable === false}
                  onChange={(event) => setEnv({ ...env, [one.env_variable]: event.target.value })}
                />
                {one.description && <small className="muted">{one.description}</small>}
              </label>
            ))}
          <label className="pt-check">
            <input type="checkbox" checked={backup} onChange={(event) => setBackup(event.target.checked)} />
            {tr("Back up before switching")}
          </label>
          <label className="pt-check">
            <input type="checkbox" checked={wipe} onChange={(event) => setWipe(event.target.checked)} />
            {tr("Wipe all server files and reinstall")}
          </label>
          <div className="pt-actions">
            <Button variant="ghost" onClick={() => setStep(1)}>
              {tr("Back")}
            </Button>
            <Button variant="primary" disabled={!image} onClick={() => setStep(3)}>
              {tr("Next")}
            </Button>
          </div>
        </div>
      )}

      {step === 3 && egg && nest && (
        <div className="pt-form">
          <div className="sw-review">
            <span>{tr("Software")}</span>
            <span>
              <s className="muted">{currentEgg ? currentEgg.egg.name : "—"}</s> → <strong>{egg.name}</strong>
            </span>
            <span>{tr("Docker image")}</span>
            <span>
              {record.docker_image === image ? (
                <span className="muted">{tr("Keep")}</span>
              ) : (
                <>
                  <s className="muted mono">{record.docker_image}</s> → <strong>{imageLabel(image)}</strong>
                </>
              )}
            </span>
            <span>{tr("Startup command")}</span>
            <span className="mono">{startup}</span>
            <span>{tr("Changed variables")}</span>
            <span>
              {changedVars.length
                ? changedVars.map((one) => (
                    <span className="mono sw-var" key={one.env_variable}>
                      {one.env_variable}={env[one.env_variable] || '""'}
                    </span>
                  ))
                : tr("None")}
            </span>
            <span>{tr("Backups")}</span>
            <span>{backup ? tr("Created before switching") : tr("No backup")}</span>
          </div>
          {wipe && (
            <Banner tone="danger">{tr("Deletes all server files and reinstalls. Worlds, settings and plugins will be lost.")}</Banner>
          )}
          <div className="pt-actions">
            <Button variant="ghost" disabled={!!busy} onClick={() => setStep(2)}>
              {tr("Back")}
            </Button>
            <Button
              variant="danger"
              disabled={!!busy}
              onClick={async () => {
                if (!(await part.confirm(tr("Change the software?"), tr("Switching to {0}. The server will stop and the install script will run.", [egg.name]), tr("Change"), true))) return;
                void apply();
              }}
            >
              {busy === "apply" || busy === "waiting" ? tr("Changing…") : tr("Change")}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

export function ActivityList({ rows }: { rows: Json[] }) {
  if (!rows.length) return <EmptyState icon="list" title={tr("No activity")} />;
  return (
    <div className="pt-list pt-activity">
      {rows.map((one) => {
        const actor = one.relationships?.actor?.attributes?.username ?? "";
        const props = one.properties && typeof one.properties === "object" ? one.properties : {};
        const detail = Object.entries(props)
          .filter(([, value]) => typeof value === "string" || typeof value === "number")
          .map(([key, value]) => `${key}=${value}`)
          .join(" · ");
        return (
          <div className="pt-activity-row" key={one.id}>
            <span className="mono">{one.event}</span>
            <span className="muted">{[actor, one.ip, detail].filter(Boolean).join(" · ")}</span>
            <small className="muted" data-tip={date(isoSecs(one.timestamp))}>
              {relativeTime(isoSecs(one.timestamp))}
            </small>
          </div>
        );
      })}
    </div>
  );
}

function Activity(part: Part) {
  const { rows } = useRows(part.hostId, `${part.base}/activity?include=actor&per_page=50`, attrs, part.onError);
  if (!rows) return <Skeleton height={120} />;
  return (
    <>
      <Banner>{tr("The latest 50 activities recorded by the panel for this server. Actions from this app appear here too.")}</Banner>
      <ActivityList rows={rows} />
    </>
  );
}
