import { open as openFile, save } from "@tauri-apps/plugin-dialog";
import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { onMonolink, onWeb, webSession } from "../../lib/bridge";
import { downloadFromHost } from "../../lib/download";
import { pickRawFiles, uploadRaw } from "../../lib/pickFile";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { SegmentedControl, Switch } from "../primitives";
import { Icon } from "../ui/Icon";
import "./StudioItems.css";
import "./StudioBundles.css";

/**
 * Studio maker M6: share studio files as one `.monolab` file (a zip of the files, their assets and pack numbers)
 * and bring one in. Resources others point at (a mob's model, loot, gear) come along by themselves.
 */

type Ref = { kind: string; id: string };
type Peek = { name: string; resources: Ref[]; assets: string[]; clashes: string[] };

const recordOf = (value: unknown): Record<string, unknown> => (value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {});
const arrayOf = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const stringOf = (value: unknown): string => (typeof value === "string" ? value : "");

function refsOf(value: unknown): Ref[] {
  return arrayOf(value)
    .map((one) => ({ kind: stringOf(recordOf(one).kind), id: stringOf(recordOf(one).id) }))
    .filter((one) => one.kind && one.id);
}

/** Assets as "folder/name", from objects or plain strings. */
function assetsOf(value: unknown): string[] {
  return arrayOf(value)
    .map((one) => (typeof one === "string" ? one : [stringOf(recordOf(one).folder), stringOf(recordOf(one).name)].filter(Boolean).join("/")))
    .filter(Boolean);
}

const keyOf = (one: Ref) => `${one.kind}/${one.id}`;

/** "Ruby pack" → "ruby_pack.monolab". */
function fileNameOf(name: string): string {
  const stem = name
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 48);
  return `${stem || "studio"}.monolab`;
}

function Check({ on, label, hint, onToggle }: { on: boolean; label: string; hint?: string; onToggle: () => void }) {
  // A large checkbox in front, like picking what an invite link allows (the user's call, 2026-11-21).
  return (
    <button type="button" className={`studio-item-check ${on ? "on" : ""}`} aria-pressed={on} onClick={onToggle}>
      <span className="studio-item-check-box" aria-hidden>
        {on && <Icon name="check" />}
      </span>
      <span className="studio-bundle-check-text">
        {label}
        {hint && <small>{hint}</small>}
      </span>
    </button>
  );
}

function ShareTab({ serverId, resources, kindLabel, busy, setBusy, onError, onNotice, onClose }: { serverId: string; resources: Ref[]; kindLabel: (kind: string) => string; busy: boolean; setBusy: (on: boolean) => void; onError: (message: string) => void; onNotice: (message: string) => void; onClose: () => void }) {
  const [picked, setPicked] = useState<Set<string>>(() => new Set());
  const [name, setName] = useState("");
  const [withAssets, setWithAssets] = useState(true);
  const [done, setDone] = useState<{ path: string; added: Ref[]; assets: number } | null>(null);
  const groups = useMemo(() => {
    const kinds = [...new Set(resources.map((one) => one.kind))];
    return kinds.map((kind) => ({ kind, items: resources.filter((one) => one.kind === kind).sort((a, b) => a.id.localeCompare(b.id)) }));
  }, [resources]);
  const toggle = (keys: string[], on: boolean) =>
    setPicked((now) => {
      const next = new Set(now);
      for (const key of keys) {
        if (on) next.add(key);
        else next.delete(key);
      }
      return next;
    });
  const chosen = resources.filter((one) => picked.has(keyOf(one)));

  const share = async () => {
    const title = name.trim() || chosen[0]?.id || "studio";
    if (onMonolink) {
      // A teammate's window: the host makes the bundle and this window downloads it (F6).
      setBusy(true);
      try {
        const saved = await downloadFromHost({ id: serverId, kind: "bundle", name: title, resources: chosen, includeAssets: withAssets });
        setDone({ path: saved, added: [], assets: 0 });
        onNotice(tr("Saved {0}.", [saved]));
      } catch (cause) {
        onError(trBack(String(cause)));
      } finally {
        setBusy(false);
      }
      return;
    }
    const path = await save({ title: tr("Share as .monolab"), defaultPath: fileNameOf(title), filters: [{ name: "Monolab", extensions: ["monolab"] }] });
    if (typeof path !== "string") return;
    setBusy(true);
    try {
      const info = recordOf((await commands.studioBundleExport(serverId, title, chosen, withAssets, path)) as unknown);
      const all = refsOf(info.resources);
      const added = all.filter((one) => !picked.has(keyOf(one)));
      setDone({ path, added, assets: assetsOf(info.assets).length });
      onNotice(tr("Saved {0}.", [fileNameOf(title)]));
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  if (done) {
    return (
      <div className="studio-bundle-body">
        <p>{tr("Saved to {0}.", [done.path])}</p>
        {done.added.length > 0 && (
          <div className="studio-bundle-list">
            <strong>{tr("Also included, because the picked files use them")}</strong>
            <div className="studio-bundle-chips">
              {done.added.map((one) => (
                <code key={keyOf(one)}>{keyOf(one)}</code>
              ))}
            </div>
          </div>
        )}
        <p className="muted">{tr("{0, plural, one {# asset} other {# assets}} inside.", [done.assets])}</p>
        <div className="modal-actions">
          <button type="button" className="button secondary" onClick={() => setDone(null)}>
            {tr("Share more")}
          </button>
          <button type="button" className="button primary" onClick={onClose}>
            {tr("Done")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="studio-bundle-body">
      <label className="field">
        <span>{tr("Name")}</span>
        <input value={name} maxLength={64} placeholder={tr("Ruby set")} onChange={(event) => setName(event.target.value)} />
      </label>
      {resources.length === 0 ? (
        <p className="studio-steps-empty">{tr("This server has no studio files to share yet.")}</p>
      ) : (
        <div className="studio-bundle-groups">
          {groups.map(({ kind, items }) => {
            const keys = items.map(keyOf);
            const all = keys.every((key) => picked.has(key));
            return (
              <div key={kind} className="studio-bundle-group">
                <div className="studio-bundle-group-head">
                  <strong>{kindLabel(kind)}</strong>
                  <button type="button" className="button ghost small" onClick={() => toggle(keys, !all)}>
                    {all ? tr("Pick none") : tr("Pick all")}
                  </button>
                </div>
                <div className="studio-item-checks" role="group" aria-label={kindLabel(kind)}>
                  {items.map((one) => (
                    <Check key={one.id} on={picked.has(keyOf(one))} label={one.id} onToggle={() => toggle([keyOf(one)], !picked.has(keyOf(one)))} />
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
      <label className="studio-toggle">
        <span>{tr("Include pictures, models and sounds")}</span>
        <Switch checked={withAssets} onChange={(event) => setWithAssets(event.target.checked)} />
      </label>
      <p className="muted studio-bundle-note">{tr("Files the picked ones use, like a mob's model or loot table, come along by themselves.")}</p>
      <div className="modal-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          {tr("Cancel")}
        </button>
        <button type="button" className="button primary" disabled={busy || chosen.length === 0} onClick={() => void share()}>
          <Icon name="package" />
          {chosen.length ? tr("Save {0, plural, one {# file} other {# files}}", [chosen.length]) : tr("Save")}
        </button>
      </div>
    </div>
  );
}

function ImportTab({ serverId, kindLabel, busy, setBusy, onError, onNotice, onImported, onClose }: { serverId: string; kindLabel: (kind: string) => string; busy: boolean; setBusy: (on: boolean) => void; onError: (message: string) => void; onNotice: (message: string) => void; onImported?: () => void; onClose: () => void }) {
  const [path, setPath] = useState<string | null>(null);
  const [peek, setPeek] = useState<Peek | null>(null);
  const [replace, setReplace] = useState(false);

  // In a teammate's or web window the file is sent to the host first and named by its upload.
  const choose = async (): Promise<string | null> => {
    if (!onWeb) {
      const chosen = await openFile({ title: tr("Import .monolab"), multiple: false, directory: false, filters: [{ name: "Monolab", extensions: ["monolab", "zip"] }] });
      return typeof chosen === "string" ? chosen : null;
    }
    const [file] = await pickRawFiles(".monolab,.zip");
    return file ? uploadRaw(file, webSession()) : null;
  };

  const pick = async () => {
    setBusy(true);
    setPeek(null);
    try {
      const chosen = await choose();
      if (!chosen) return;
      const read = recordOf((await (onWeb ? commands.studioBundlePeekUpload(serverId, chosen) : commands.studioBundlePeek(serverId, chosen))) as unknown);
      setPath(chosen);
      setPeek({ name: stringOf(read.name), resources: refsOf(read.resources), assets: assetsOf(read.assets), clashes: arrayOf(read.clashes).map(stringOf).filter(Boolean) });
      setReplace(false);
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const bring = async () => {
    if (!path || !peek) return;
    setBusy(true);
    try {
      const info = recordOf((await (onWeb ? commands.studioBundleImportUpload(serverId, path, replace) : commands.studioBundleImport(serverId, path, replace))) as unknown);
      const count = refsOf(info.resources).length;
      onNotice(tr("Imported {0, plural, one {# file} other {# files}}. The pack was built again.", [count]));
      onImported?.();
      onClose();
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const clashes = new Set(peek?.clashes ?? []);
  const kept = peek ? peek.resources.filter((one) => replace || !clashes.has(keyOf(one))).length : 0;
  return (
    <div className="studio-bundle-body">
      <div className="studio-bundle-pick">
        <button type="button" className="button secondary" disabled={busy} onClick={() => void pick()}>
          <Icon name="import" />
          {path ? tr("Pick another file") : tr("Pick a .monolab file")}
        </button>
        {path && <code title={path}>{path.split(/[\\/]/).pop()}</code>}
      </div>
      {peek && (
        <>
          <p>
            <strong>{peek.name || tr("Unnamed")}</strong>
            {" · "}
            {tr("{0, plural, one {# file} other {# files}}, {1, plural, one {# asset} other {# assets}}", [peek.resources.length, peek.assets.length])}
          </p>
          <div className="studio-bundle-rows">
            {peek.resources.map((one) => (
              <div key={keyOf(one)} className={`studio-bundle-row ${clashes.has(keyOf(one)) ? "clash" : ""}`}>
                <span className="muted">{kindLabel(one.kind)}</span>
                <code>{one.id}</code>
                {clashes.has(keyOf(one)) && <span className="studio-bundle-flag">{replace ? tr("Replaces yours") : tr("Already here, skipped")}</span>}
              </div>
            ))}
            {peek.assets.map((one) => (
              <div key={one} className={`studio-bundle-row ${clashes.has(`assets/${one}`) ? "clash" : ""}`}>
                <span className="muted">{tr("Asset")}</span>
                <code>{one}</code>
                {clashes.has(`assets/${one}`) && <span className="studio-bundle-flag">{replace ? tr("Replaces yours") : tr("Already here, skipped")}</span>}
              </div>
            ))}
          </div>
          {clashes.size > 0 && (
            <label className="studio-toggle">
              <span>{tr("Replace what's already here ({0})", [clashes.size])}</span>
              <Switch checked={replace} onChange={(event) => setReplace(event.target.checked)} />
            </label>
          )}
          <p className="muted studio-bundle-note">{tr("Pack numbers are kept when they're free here; otherwise new ones are handed out. The pack is built again after.")}</p>
        </>
      )}
      <div className="modal-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          {tr("Cancel")}
        </button>
        <button type="button" className="button primary" disabled={busy || !peek || (kept === 0 && peek.assets.length === 0)} onClick={() => void bring()}>
          {tr("Import")}
        </button>
      </div>
    </div>
  );
}

/** The share / import dialog. `resources` are the server's studio files; `kindLabel` names a kind for headings. */
export function StudioBundles({
  serverId,
  resources,
  kindLabel = (kind) => kind,
  startOn = "share",
  onClose,
  onError,
  onNotice,
  onImported,
}: {
  serverId: string;
  resources: Ref[];
  kindLabel?: (kind: string) => string;
  startOn?: "share" | "import";
  onClose: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onImported?: () => void;
}) {
  const [tab, setTab] = useState<"share" | "import">(startOn);
  const [busy, setBusy] = useState(false);
  const close = () => !busy && onClose();
  return createPortal(
    <div className="modal-backdrop roomy" onClick={close}>
      <div className="add-modal studio-bundle-dialog" role="dialog" aria-modal="true" aria-label={tr("Share and import")} onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Share and import")}</strong> · .monolab
          </p>
          <div className="modal-top-actions">
            <button type="button" className="close in-flow" aria-label={tr("Close")} disabled={busy} onClick={close}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        {onWeb && !onMonolink ? (
          <ImportTab serverId={serverId} kindLabel={kindLabel} busy={busy} setBusy={setBusy} onError={onError} onNotice={onNotice} onImported={onImported} onClose={onClose} />
        ) : (
          <>
            <SegmentedControl
              value={tab}
              onChange={(value) => setTab(value)}
              options={[
                { value: "share", label: tr("Share as .monolab") },
                { value: "import", label: tr("Import .monolab") },
              ]}
            />
            {tab === "share" ? (
              <ShareTab serverId={serverId} resources={resources} kindLabel={kindLabel} busy={busy} setBusy={setBusy} onError={onError} onNotice={onNotice} onClose={onClose} />
            ) : (
              <ImportTab serverId={serverId} kindLabel={kindLabel} busy={busy} setBusy={setBusy} onError={onError} onNotice={onNotice} onImported={onImported} onClose={onClose} />
            )}
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
