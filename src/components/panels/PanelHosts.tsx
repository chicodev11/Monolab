import { useCallback, useEffect, useState } from "react";
import type { PanelHost, PanelHostView, PanelProbe, PanelRole } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { SegmentedControl } from "../primitives";
import { Icon } from "../ui/Icon";
import { PanelCreate } from "./PanelCreate";

const NEEDS: { of: string; label: string }[] = [
  { of: "control.console", label: "Console" },
  { of: "control.start", label: "Enable" },
  { of: "control.stop", label: "Suspend" },
  { of: "file.read", label: "View files" },
  { of: "file.update", label: "Save files" },
  { of: "backup.read", label: "Backups" },
];

type Draft = Omit<PanelHost, "role"> & { role: PanelRole; key: string; appKey: string };

function blank(): Draft {
  return { id: "", name: "", url: "", role: "user", key: "", appKey: "" };
}

export function PanelHosts({
  startAdding,
  confirm,
  onError,
  onNotice,
  onChanged,
}: {
  startAdding: boolean;
  confirm: (title: string, body: string, ok: string, danger?: boolean) => Promise<boolean>;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onChanged: () => void;
}) {
  const [hosts, setHosts] = useState<PanelHostView[]>([]);
  const [editing, setEditing] = useState<string | null>(startAdding ? "new" : null);
  const [draft, setDraft] = useState(blank());
  const [probe, setProbe] = useState<PanelProbe | null>(null);
  const [busy, setBusy] = useState("");
  const [creating, setCreating] = useState<string | null>(null);

  const reload = useCallback(() => {
    commands
      .listPanelHosts()
      .then(setHosts)
      .catch((cause) => onError(String(cause)));
  }, [onError]);
  useEffect(() => reload(), [reload]);

  function startNew() {
    setDraft(blank());
    setProbe(null);
    setEditing("new");
  }

  function startEdit(one: PanelHostView) {
    setDraft({ id: one.id, name: one.name, url: one.url, role: one.role ?? "user", key: "", appKey: "" });
    setProbe(null);
    setEditing(one.id);
  }

  async function test() {
    setBusy("test");
    setProbe(null);
    try {
      setProbe(
        await commands.testPanelHost(
          draft.url,
          draft.key.trim() || null,
          draft.role === "admin" ? draft.appKey.trim() || null : null,
          draft.role,
          draft.id || null,
        ),
      );
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy("");
    }
  }

  async function save() {
    setBusy("save");
    try {
      const id = draft.id || `panel-${Date.now().toString(36)}`;
      await commands.savePanelHost(
        { id, name: draft.name, url: draft.url, role: draft.role },
        draft.key.trim() ? draft.key : null,
        draft.role === "admin" && draft.appKey.trim() ? draft.appKey : null,
      );
      onNotice(tr("Panel saved."));
      setEditing(null);
      setProbe(null);
      reload();
      onChanged();
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy("");
    }
  }

  async function remove(one: PanelHostView) {
    if (
      !(await confirm(
        tr("Remove {0} from the list?", [one.name]),
        tr("Only removes it from the app. Imported servers are also removed from the list, but the panel and its servers stay unchanged. API keys are deleted too."),
        tr("Remove"),
        true,
      ))
    )
      return;
    try {
      const gone = await commands.deletePanelHost(one.id);
      onNotice(
        gone > 0
          ? tr("Removed {0} and {1, plural, one {# server} other {# servers}} from the list.", [one.name, gone])
          : tr("Deleted {0}.", [one.name]),
      );
      reload();
      onChanged();
    } catch (cause) {
      onError(String(cause));
    }
  }

  const hasNeededKey =
    !!draft.id || (draft.role === "admin" ? !!draft.appKey.trim() : !!draft.key.trim());

  const form = (
    <div className="panel-form">
      <label className="field-block">
        <span>{tr("Name")}</span>
        <input
          value={draft.name}
          placeholder={tr("My panel")}
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
        />
      </label>
      <label className="field-block">
        <span>{tr("Panel address")}</span>
        <input
          value={draft.url}
          placeholder="http://192.168.0.10 · https://panel.example.com"
          spellCheck={false}
          onChange={(event) => setDraft({ ...draft, url: event.target.value })}
        />
      </label>
      <div className="field-block">
        <span>{tr("Role")}</span>
        <SegmentedControl
          className="even"
          value={draft.role}
          options={[
            { value: "user", label: tr("User") },
            { value: "admin", label: tr("Admin") },
          ]}
          onChange={(role) => {
            setDraft({ ...draft, role });
            setProbe(null);
          }}
        />
        <small className="panel-role-note">
          {draft.role === "admin"
            ? tr("View all servers on the panel and create new ones.")
            : tr("View and manage only this account's servers, including shared servers.")}
        </small>
      </div>
      {draft.role === "admin" && (
        <label className="field-block">
          <span>{tr("Application API key")}</span>
          <input
            type="password"
            value={draft.appKey}
            placeholder={draft.id ? tr("Leave blank to keep it") : "ptlc_…"}
            spellCheck={false}
            onChange={(event) => setDraft({ ...draft, appKey: event.target.value })}
          />
        </label>
      )}
      <label className="field-block">
        <span>{draft.role === "admin" ? tr("Account API key (optional)") : tr("Account API key")}</span>
        <input
          type="password"
          value={draft.key}
          placeholder={draft.id ? tr("Leave blank to keep it") : "ptlc_…"}
          spellCheck={false}
          onChange={(event) => setDraft({ ...draft, key: event.target.value })}
        />
      </label>
      <p className="panel-hint">
        <Icon name="info" />
        {draft.role === "admin"
          ? tr("Create an Application key under Application API in the admin area (read/write access to servers; read access to nodes, allocations, users and eggs). An account key is required to start and stop servers or use the console.")
          : tr("Sign in to the panel and create a key under Account → API Credentials. It's only shown once.")}
      </p>
      <div className="panel-form-actions">
        <button
          className="button secondary small"
          disabled={!!busy || !draft.url.trim() || !hasNeededKey}
          onClick={() => void test()}
        >
          {busy === "test" ? tr("Checking connection…") : tr("Test connection")}
        </button>
        <button
          className="button primary small"
          disabled={!!busy || !draft.url.trim() || !hasNeededKey}
          onClick={() => void save()}
        >
          {busy === "save" ? tr("Saving…") : tr("Save")}
        </button>
        <button className="button ghost small" disabled={!!busy} onClick={() => setEditing(null)}>
          {tr("Cancel")}
        </button>
      </div>
      {probe && (
        <div className="panel-probe">
          <strong>
            <Icon name="check" />
            {tr("Valid key · {0, plural, one {# server} other {# servers}}", [probe.servers.length])}
            {probe.username && ` · ${probe.username}${probe.admin ? tr(" (admin)") : ""}`}
            {probe.daemon && ` · ${probe.daemon}`}
          </strong>
          {!probe.accountKey && (
            <p className="panel-locked">
              {tr("Without an account key, you can only create and view servers. Add one to start and stop servers or use the console.")}
            </p>
          )}
          {probe.servers.length ? (
            <ul>
              {probe.servers.slice(0, 8).map((one) => (
                <li key={one.identifier}>
                  <b>{one.name}</b>
                  <span>{one.identifier}</span>
                  {one.node && <span>{one.node}</span>}
                  {one.port > 0 && <span>:{one.port}</span>}
                  {one.suspended && <em>{tr("Suspended")}</em>}
                </li>
              ))}
            </ul>
          ) : (
            <p>{tr("This key can't access any servers. Check its server permissions in the panel.")}</p>
          )}
          {probe.permissions.length > 0 && (
            <p className="panel-locked">
              {(() => {
                const missing = NEEDS.filter((one) => !probe.permissions.includes(one.of));
                return missing.length
                  ? tr("Restricted features: {0}", [missing.map((one) => tr(one.label)).join(", ")])
                  : tr("All required permissions are enabled.");
              })()}
            </p>
          )}
        </div>
      )}
    </div>
  );

  return (
    <div className="panel-hosts">
      {hosts.map((one) => (
        <div key={one.id} className="panel-card glow-card">
          <div className="panel-card-head">
            <div>
              <strong>
                {one.name}
                <em className="panel-role">{one.role === "admin" ? tr("Admin") : tr("User")}</em>
              </strong>
              <span>{one.url}</span>
            </div>
            <div className="panel-card-actions">
              {!(one.role === "admin" ? one.hasAppKey : one.hasKey) && (
                <em className="panel-nokey">{tr("No key")}</em>
              )}
              {one.role === "admin" && one.hasAppKey && creating !== one.id && (
                <button className="button secondary small" onClick={() => setCreating(one.id)}>
                  <Icon name="plus" />
                  {tr("Create server")}
                </button>
              )}
              <button
                className="icon-button"
                data-tip={tr("Edit")}
                aria-label={tr("Edit")}
                onClick={() => startEdit(one)}
              >
                <Icon name="pencil" />
              </button>
              <button
                className="icon-button"
                data-tip={tr("Remove")}
                aria-label={tr("Remove")}
                onClick={() => void remove(one)}
              >
                <Icon name="trash" />
              </button>
            </div>
          </div>
          {editing === one.id && form}
          {creating === one.id && (
            <PanelCreate
              hostId={one.id}
              onDone={() => setCreating(null)}
              onCancel={() => setCreating(null)}
              onError={onError}
              onNotice={onNotice}
            />
          )}
        </div>
      ))}
      {editing === "new" ? (
        <div className="panel-card glow-card">{form}</div>
      ) : (
        <button className="button primary small panel-add" onClick={startNew}>
          <Icon name="plus" />
          {tr("Add panel")}
        </button>
      )}
      {!hosts.length && editing !== "new" && (
        <p className="panel-hint">
          {tr("Add a Pterodactyl-compatible panel (Pterodactyl, Pelican or Hydrodactyl) to start and stop its servers and view their consoles here.")}
        </p>
      )}
    </div>
  );
}
