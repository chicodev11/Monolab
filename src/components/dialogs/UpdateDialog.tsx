import { useEffect, useMemo, useState } from "react";
import type { AddonUpdate, AddonVersion } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

export type UpdateRow = {
  update: AddonUpdate;
  title: string;
  iconUrl: string | null;
  projectId: string | null;
  kind: "mod" | "plugin";
};

export type UpdateChoice = { fileName: string; versionId: string | null; versionName: string };

const CHANNEL: Record<string, string> = { release: "R", beta: "B", alpha: "A" };

export function UpdateDialog({
  rows,
  busy,
  listVersions,
  onCancel,
  onApply,
}: {
  rows: UpdateRow[];
  busy: boolean;
  listVersions: (projectId: string, kind: "mod" | "plugin") => Promise<AddonVersion[]>;
  onCancel: () => void;
  onApply: (choices: UpdateChoice[]) => void;
}) {
  const [checked, setChecked] = useState<Set<string>>(() => new Set(rows.map((row) => row.update.fileName)));
  const [versions, setVersions] = useState<Record<string, AddonVersion[] | null>>({});
  const [chosen, setChosen] = useState<Record<string, string>>({});
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    for (const row of rows) {
      if (!row.projectId) continue;
      const key = row.update.fileName;
      setVersions((was) => ({ ...was, [key]: null }));
      void listVersions(row.projectId, row.kind).then(
        (list) => setVersions((was) => ({ ...was, [key]: list })),
        () => setVersions((was) => ({ ...was, [key]: [] })),
      );
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => event.key === "Escape" && !busy && onCancel();
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [busy, onCancel]);

  const choicesOf = useMemo(() => {
    const out: Record<string, AddonVersion[]> = {};
    for (const row of rows) {
      const list = (versions[row.update.fileName] ?? []).filter((one) => one.fits);
      const now = list.findIndex((one) => one.name === row.update.currentVersion || row.update.fileName.includes(one.name));
      out[row.update.fileName] = now > 0 ? list.slice(0, now) : list.filter((one) => one.name === row.update.newVersion);
    }
    return out;
  }, [rows, versions]);

  const pickOf = (row: UpdateRow) => {
    const list = choicesOf[row.update.fileName] ?? [];
    const id = chosen[row.update.fileName];
    return list.find((one) => one.id === id) ?? list.find((one) => one.name === row.update.newVersion) ?? list[0] ?? null;
  };

  const allOn = rows.length > 0 && rows.every((row) => checked.has(row.update.fileName));
  const toggle = (key: string) =>
    setChecked((was) => {
      const next = new Set(was);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const apply = () =>
    onApply(
      rows
        .filter((row) => checked.has(row.update.fileName))
        .map((row) => {
          const pick = pickOf(row);
          const latest = !pick || pick.name === row.update.newVersion;
          return { fileName: row.update.fileName, versionId: latest ? null : pick.id, versionName: pick?.name ?? row.update.newVersion };
        }),
    );

  return (
    <div className="modal-backdrop upd-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onCancel()}>
      <div className="upd-dialog" role="dialog" aria-modal="true">
        <header className="upd-head">
          <h3>{tr("Update")}</h3>
          <button className="icon-button" aria-label={tr("Close")} disabled={busy} onClick={onCancel}>
            <Icon name="close" />
          </button>
        </header>
        <div className="upd-row upd-colhead">
          <input type="checkbox" checked={allOn} aria-label={tr("Select all")} onChange={() => setChecked(allOn ? new Set() : new Set(rows.map((row) => row.update.fileName)))} />
          <span>{tr("Name")}</span>
          <span>{tr("Current version / New version")}</span>
          <span className="upd-right">{tr("Changelog")}</span>
        </div>
        <div className="upd-list">
          {rows.map((row) => {
            const key = row.update.fileName;
            const list = choicesOf[key] ?? [];
            const pick = pickOf(row);
            const loading = row.projectId !== null && versions[key] === null;
            const channel = pick?.channel ?? "release";
            return (
              <div key={key} className={`upd-item ${checked.has(key) ? "" : "off"}`}>
                <div className="upd-row">
                  <input type="checkbox" checked={checked.has(key)} aria-label={row.title} onChange={() => toggle(key)} />
                  <span className="upd-name">
                    {row.iconUrl ? <img src={row.iconUrl} alt="" /> : <span className="upd-icon-empty"><Icon name="package" /></span>}
                    <strong>{row.title}</strong>
                  </span>
                  <span className="upd-versions">
                    <span className="upd-now">{row.update.currentVersion || "—"}</span>
                    <Icon name="arrow" />
                    <span className={`upd-chip ${channel}`} data-tip={channel === "release" ? tr("Stable") : channel === "beta" ? tr("Beta") : tr("Alpha")}>
                      {CHANNEL[channel] ?? "R"}
                    </span>
                    {list.length > 1 ? (
                      <Dropdown
                        value={pick?.id ?? ""}
                        options={list.map((one) => ({ value: one.id, label: one.name }))}
                        onChange={(value: string) => setChosen((was) => ({ ...was, [key]: value }))}
                      />
                    ) : (
                      <strong className="upd-new">{pick?.name ?? row.update.newVersion}</strong>
                    )}
                  </span>
                  <span className="upd-right">
                    <button className="button ghost small" disabled={loading || !pick} onClick={() => setOpen(open === key ? null : key)}>
                      <Icon name={open === key ? "chevron" : "list"} />
                      {tr("View")}
                    </button>
                  </span>
                </div>
                {open === key && pick && (
                  <div className="upd-log">
                    {pick.changelogHtml ? (
                      <div className="md-body" dangerouslySetInnerHTML={{ __html: pick.changelogHtml }} />
                    ) : (
                      <p className="muted">{tr("No changelog was provided.")}</p>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
        <footer className="upd-foot">
          <span className="upd-warn">
            <Icon name="alert" />
            {tr("Updates aren't guaranteed to be safe for your worlds. Review the changelog and back up first.")}
          </span>
          <button className="button ghost" disabled={busy} onClick={onCancel}>
            {tr("Cancel")}
          </button>
          <button className="button primary" disabled={busy || checked.size === 0} onClick={apply}>
            <Icon name="import" />
            {busy ? tr("Updating…") : tr("Update {0, plural, one {# project} other {# projects}}", [checked.size])}
          </button>
        </footer>
      </div>
    </div>
  );
}
