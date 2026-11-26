import { useState } from "react";
import { tr } from "../../lib/i18n";
import { readSwitches, saveSwitches, type NoticeSwitches } from "../../lib/teamNotice";
import { Switch } from "../primitives";

/** Settings → Notifications: which team notices (F4) this PC shows. */
export function TeamNoticeSettings() {
  const [switches, setSwitches] = useState<NoticeSwitches>(readSwitches);
  const rows: { key: keyof NoticeSwitches; title: string; hint: string }[] = [
    { key: "assigned", title: tr("A to-do given to me"), hint: tr("On a shared server's Memo board.") },
    { key: "savedOver", title: tr("A file I have open was saved"), hint: tr("Someone else saved it; reload to see their change.") },
    { key: "assetAdded", title: tr("New studio pictures and models"), hint: tr("Someone added one to a shared server's studio.") },
    { key: "proposals", title: tr("Change proposals"), hint: tr("New proposals to review, and answers to mine.") },
  ];
  return (
    <>
      {rows.map((row) => (
        <div className="setting-row" key={row.key}>
          <div>
            <strong>{row.title}</strong>
            <small>{row.hint}</small>
          </div>
          <Switch
            checked={switches[row.key]}
            onChange={(event) => {
              const next = { ...switches, [row.key]: event.target.checked };
              setSwitches(next);
              saveSwitches(next);
            }}
          />
        </div>
      ))}
    </>
  );
}
