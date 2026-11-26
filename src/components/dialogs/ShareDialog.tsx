import * as RadixDialog from "@radix-ui/react-dialog";
import { useCallback, useEffect, useState } from "react";
import type { Invite, InviteLink, ServerPerson } from "../../lib/bindings";
import { date } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { roleHint, roleName, useRoles } from "../../lib/roles";
import { List, ListRow, SegmentedControl } from "../primitives";
import { RolePicker } from "../settings/RolesEditor";
import { Icon } from "../ui/Icon";

/**
 * Share a server: invite people with a one-time link to the web panel, see who has access and who is on
 * right now, and take access away. Links use this PC's home network address.
 */
export function ShareDialog({
  server,
  panelOn,
  onClose,
  onOpenPanelSettings,
  onError,
  onNotice,
  confirm,
}: {
  server: { id: string; name: string };
  panelOn: boolean;
  onClose: () => void;
  onOpenPanelSettings: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [role, setRole] = useState("Manager");
  const { roles } = useRoles();
  const roleLabel = (id: string) => roleName(id, roles);
  const hasOwn = roles.some((one) => !one.builtin);
  const [link, setLink] = useState<InviteLink | null>(null);
  const [people, setPeople] = useState<ServerPerson[] | null>(null);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const load = useCallback(async () => {
    try {
      const [who, open] = await Promise.all([commands.serverPeople(server.id), commands.listWebInvites(server.id)]);
      setPeople(who);
      setInvites(open);
    } catch (cause) {
      onError(String(cause));
    }
  }, [server.id, onError]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), 5_000);
    return () => window.clearInterval(timer);
  }, [load]);

  async function makeLink() {
    setBusy(true);
    setCopied(false);
    try {
      const made = await commands.createWebInvite(server.id, role);
      setLink(made);
      await copy(made.link);
      void load();
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  async function revoke(one: Invite) {
    try {
      await commands.revokeWebInvite(one.id);
      if (link?.id === one.id) setLink(null);
      void load();
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function removePerson(one: ServerPerson) {
    const ok = await confirm(
      tr("Remove {0} from {1}?", [one.name, server.name]),
      tr("They're signed out and can't open this server any more. If it was their only server, their account is deleted."),
      tr("Remove"),
      true,
    );
    if (!ok) return;
    try {
      await commands.removeServerAccess(server.id, one.name);
      onNotice(tr("Removed {0}.", [one.name]));
      void load();
    } catch (cause) {
      onError(String(cause));
    }
  }

  return (
    <RadixDialog.Root open onOpenChange={(open) => !open && onClose()}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="ask-backdrop modal-backdrop" />
        <RadixDialog.Content className="ask-dialog dialog share-dialog" aria-describedby={undefined}>
          <RadixDialog.Title asChild>
            <h2>{tr("Share {0}", [server.name])}</h2>
          </RadixDialog.Title>
          {!panelOn ? (
            <div className="share-off">
              <Icon name="globe" />
              <p>{tr("People can also join from a browser through the web panel. Turn it on first. Friends outside your home network need port forwarding on your router.")}</p>
              <button type="button" className="button primary" onClick={onOpenPanelSettings}>
                {tr("Open web panel settings")}
              </button>
            </div>
          ) : (
            <>
              <section className="share-section">
                <div className="share-section-head">
                  <h3>{tr("Invite someone")}</h3>
                  <button type="button" className="button ghost small" onClick={onOpenPanelSettings}>
                    {tr("Edit roles")}
                  </button>
                </div>
                {hasOwn ? (
                  <RolePicker
                    value={role}
                    onChange={(next) => {
                      setRole(next);
                      setLink(null);
                    }}
                  />
                ) : (
                  <SegmentedControl
                    value={role}
                    onChange={(next) => {
                      setRole(next);
                      setLink(null);
                    }}
                    options={[
                      { value: "Manager", label: tr("Manager") },
                      { value: "Viewer", label: tr("Viewer") },
                    ]}
                  />
                )}
                <p className="muted share-hint">{roleHint(roles.find((one) => one.id === role))}</p>
                {link ? (
                  <div className="share-link">
                    <code>{link.link}</code>
                    <button type="button" className="button secondary small" onClick={() => void copy(link.link)}>
                      <Icon name={copied ? "check" : "copy"} />
                      {copied ? tr("Copied") : tr("Copy")}
                    </button>
                  </div>
                ) : (
                  <button type="button" className="button primary" disabled={busy} onClick={() => void makeLink()}>
                    <Icon name="plus" />
                    {busy ? tr("Making a link…") : tr("Make an invite link")}
                  </button>
                )}
                {link && <p className="muted share-hint">{tr("Works once, until {0}. Whoever opens it picks their own name and password.", [date(link.expires)])}</p>}
              </section>

              <section className="share-section">
                <h3>{tr("People with access")}</h3>
                {people === null ? (
                  <p className="muted">{tr("Loading…")}</p>
                ) : !people.length ? (
                  <p className="muted">{tr("Only you so far.")}</p>
                ) : (
                  <List variant="box" density="compact" className="share-people">
                    {people.map((one) => (
                      <ListRow
                        key={one.name}
                        leading={<span className={`share-mark${one.here ? " here" : one.online ? " online" : ""}`}>{one.name.slice(0, 1).toUpperCase()}</span>}
                        title={one.name}
                        subtitle={
                          <>
                            {roleLabel(one.role)}
                            {one.here ? ` · ${tr("on this server now")}` : one.online ? ` · ${tr("online")}` : ""}
                          </>
                        }
                        actions={
                          one.role !== "Admin" && (
                            <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove access")} onClick={() => void removePerson(one)}>
                              <Icon name="close" />
                            </button>
                          )
                        }
                      />
                    ))}
                  </List>
                )}
              </section>

              {invites.length > 0 && (
                <section className="share-section">
                  <h3>{tr("Open invites")}</h3>
                  <List variant="box" density="compact" className="share-people">
                    {invites.map((one) => (
                      <ListRow
                        key={one.id}
                        leading={<span className="share-mark"><Icon name="clock" /></span>}
                        title={roleLabel(one.role)}
                        subtitle={tr("Until {0}", [date(one.expires)])}
                        actions={
                          <button type="button" className="button ghost small" onClick={() => void revoke(one)}>
                            {tr("Cancel")}
                          </button>
                        }
                      />
                    ))}
                  </List>
                </section>
              )}
            </>
          )}
          <div className="modal-actions">
            <button type="button" className="button secondary" onClick={onClose}>
              {tr("Done")}
            </button>
          </div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}
