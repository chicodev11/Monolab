import { useState } from "react";
import type { RoleView } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { PERMS, permLabel, roleHint, roleName, rolesChanged, serverRoles, useRoles } from "../../lib/roles";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { List, ListRow } from "../primitives";

type Draft = { id: string; name: string; perms: string[] };

/**
 * The host's own roles next to the built-in ones: make one, rename it, tick what it may do, delete it.
 * A role people still have is deleted only after picking where they move. Saved in the app settings and
 * checked by the host for web panel accounts and Monolink teammates alike.
 */
export function RolesEditor({
  onError,
  onNotice,
  confirm,
}: {
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const { roles } = useRoles();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [leaving, setLeaving] = useState<{ role: RoleView; to: string } | null>(null);
  const [busy, setBusy] = useState(false);

  async function save() {
    if (!draft) return;
    setBusy(true);
    try {
      await commands.saveCustomRole(draft.id, draft.name, draft.perms);
      onNotice(draft.id ? tr("Saved role {0}.", [draft.name.trim()]) : tr("Made role {0}.", [draft.name.trim()]));
      setDraft(null);
      rolesChanged();
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  async function remove(role: RoleView, to: string | null) {
    if (!to) {
      const ok = await confirm(tr("Delete role {0}?", [role.name]), tr("Nobody has this role, so nobody loses anything."), tr("Delete"), true);
      if (!ok) return;
    }
    setBusy(true);
    try {
      await commands.deleteCustomRole(role.id, to);
      onNotice(tr("Deleted role {0}.", [role.name]));
      setLeaving(null);
      if (draft?.id === role.id) setDraft(null);
      rolesChanged();
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  function toggle(perm: string, on: boolean) {
    if (!draft) return;
    const next = on ? [...draft.perms, perm] : draft.perms.filter((one) => one !== perm);
    setDraft({ ...draft, perms: PERMS.filter((one) => next.includes(one)) });
  }

  const editor = draft && (
    <div className="role-editor">
      <label className="field-block">
        <span className="field-label">{tr("Role name")}</span>
        <input autoFocus value={draft.name} maxLength={32} placeholder={tr("e.g. Builder")} onChange={(event) => setDraft({ ...draft, name: event.target.value })} />
      </label>
      <div className="role-perms" role="group" aria-label={tr("Permissions")}>
        {PERMS.map((perm) => (
          <label key={perm} className={`role-perm${draft.perms.includes(perm) ? " on" : ""}`}>
            {/* A large checkbox in front, like picking what an invite link allows (the user's call, 2026-11-21). */}
            <input type="checkbox" className="role-check" checked={draft.perms.includes(perm)} onChange={(event) => toggle(perm, event.target.checked)} />
            <span>
              <strong>{permLabel(perm)}</strong>
              <code>{perm}</code>
            </span>
          </label>
        ))}
      </div>
      <p className="muted role-note">{tr("Anyone with a role can open the servers they were given and see their overview. App settings stay with Admins.")}</p>
      <div className="modal-actions">
        <button type="button" className="button secondary" onClick={() => setDraft(null)}>
          {tr("Cancel")}
        </button>
        <button type="button" className="button primary" disabled={busy || !draft.name.trim()} onClick={() => void save()}>
          {tr("Save")}
        </button>
      </div>
    </div>
  );

  return (
    <section className="web-card roles-card">
      <header className="web-card-head">
        <div>
          <strong>{tr("Roles")}</strong>
          <small>{tr("What people may do on the servers you give them. Make your own next to the built-in ones.")}</small>
        </div>
        <button type="button" className="button secondary small" disabled={draft?.id === ""} onClick={() => setDraft({ id: "", name: "", perms: ["console.view"] })}>
          <Icon name="plus" />
          {tr("New role")}
        </button>
      </header>
      {draft?.id === "" && editor}
      <List variant="box" className="role-list">
        {roles.map((role) => (
          <ListRow
            key={role.id}
            leading={
              <span className="role-mark">
                <Icon name={role.builtin ? "shield" : "users"} />
              </span>
            }
            title={
              <>
                {roleName(role.id, roles)}
                {role.builtin && <span className="chip">{tr("Built-in")}</span>}
              </>
            }
            subtitle={roleHint(role)}
            meta={<span className="role-count">{tr("{0, plural, one {# person} other {# people}}", [role.people])}</span>}
            actions={
              !role.builtin && (
                <>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={tr("Edit {0}", [role.name])}
                    data-tip={tr("Edit")}
                    onClick={() => setDraft(draft?.id === role.id ? null : { id: role.id, name: role.name, perms: role.perms })}
                  >
                    <Icon name="pencil" />
                  </button>
                  <button
                    type="button"
                    className="icon-button danger"
                    aria-label={tr("Delete {0}", [role.name])}
                    data-tip={tr("Delete")}
                    onClick={() => (role.people > 0 ? setLeaving({ role, to: "Viewer" }) : void remove(role, null))}
                  >
                    <Icon name="trash" />
                  </button>
                </>
              )
            }
            detail={
            (draft?.id === role.id || leaving?.role.id === role.id) && (
            <>
            {draft?.id === role.id && editor}
            {leaving?.role.id === role.id && (
              <div className="role-move">
                <p>{tr("{0, plural, one {# person has} other {# people have}} this role. Move them to another role before deleting it.", [role.people])}</p>
                <div className="role-move-row">
                  <Dropdown
                    value={leaving.to}
                    onChange={(to) => setLeaving({ role, to })}
                    options={serverRoles(roles)
                      .filter((one) => one.id !== role.id)
                      .map((one) => ({ value: one.id, label: roleName(one.id, roles) }))}
                  />
                  <button type="button" className="button secondary small" onClick={() => setLeaving(null)}>
                    {tr("Cancel")}
                  </button>
                  <button type="button" className="button danger small" disabled={busy} onClick={() => void remove(role, leaving.to)}>
                    {tr("Move and delete")}
                  </button>
                </div>
              </div>
            )}
            </>
            )
            }
          />
        ))}
      </List>
    </section>
  );
}

/** A role picker for one person or invite: Manager, Viewer and the host's own roles (Admin too when `admin`). */
export function RolePicker({
  value,
  onChange,
  admin,
  inherit,
  disabled,
}: {
  value: string;
  onChange: (role: string) => void;
  /** Offer Admin too (web panel accounts only). */
  admin?: boolean;
  /** Offer "" first with this label (a teammate's team role). */
  inherit?: string;
  disabled?: boolean;
}) {
  const { roles } = useRoles();
  const choices = (admin ? roles : serverRoles(roles)).map((one) => ({ value: one.id, label: roleName(one.id, roles), hint: roleHint(one) }));
  const options = inherit ? [{ value: "", label: inherit }, ...choices] : choices;
  return <Dropdown value={value} options={options} onChange={onChange} disabled={disabled} align="end" />;
}
