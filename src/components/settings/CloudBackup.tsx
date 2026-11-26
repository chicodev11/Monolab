import { useEffect, useState } from "react";
import type { CloudInfo } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Switch } from "../primitives";

/** Copies of each backup to S3-compatible storage (Backblaze B2, AWS S3, Cloudflare R2...). */
export function CloudBackup({ onNotice, onError }: { onNotice: (text: string) => void; onError: (text: string) => void }) {
  const [info, setInfo] = useState<CloudInfo | null>(null);
  const [draft, setDraft] = useState<CloudInfo | null>(null);
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void commands
      .cloudInfo()
      .then((one) => {
        setInfo(one);
        setDraft(one);
      })
      .catch(() => undefined);
  }, []);
  if (!info || !draft) return null;
  const save = async (next: CloudInfo) => {
    setBusy(true);
    try {
      const saved = await commands.setCloud(next, secret.trim() ? secret : null);
      setInfo(saved);
      setDraft(saved);
      setSecret("");
      if (next.on && !saved.on) onError(tr("Fill in the endpoint, bucket and both keys to turn copies on."));
      else onNotice(tr("Cloud copy settings saved."));
    } catch (error) {
      onError(trBack(String(error)));
    } finally {
      setBusy(false);
    }
  };
  const field = (key: "endpoint" | "region" | "bucket" | "prefix" | "keyId", label: string, placeholder: string) => (
    <label className="field">
      <span>{label}</span>
      <input value={draft[key]} placeholder={placeholder} spellCheck={false} autoComplete="off" onChange={(event) => setDraft({ ...draft, [key]: event.target.value })} />
    </label>
  );
  const changed = JSON.stringify(draft) !== JSON.stringify(info) || secret.trim() !== "";
  return (
    <div className="setting-row cloud-backup">
      <div>
        <strong>{tr("Copy backups to the cloud")}</strong>
        <small>
          {tr("After each backup on this PC, a copy goes to an S3-compatible bucket (Backblaze B2, AWS S3, Cloudflare R2). Up to 5 GB a backup. The keys stay sealed on this PC.")}
          {info.last && (
            <>
              <br />
              {tr("Last copy:")} {trBack(info.last)}
            </>
          )}
        </small>
      </div>
      <Switch checked={info.on} disabled={busy} aria-label={tr("Copy backups to the cloud")} onChange={(event) => void save({ ...draft, on: event.target.checked })} />
      <div className="cloud-fields">
        {field("endpoint", tr("Endpoint"), "https://s3.us-west-004.backblazeb2.com")}
        {field("region", tr("Region"), "us-west-004")}
        {field("bucket", tr("Bucket"), "my-backups")}
        {field("prefix", tr("Folder in the bucket"), "monolab")}
        {field("keyId", tr("Key ID"), "004a1b2c…")}
        <label className="field">
          <span>{tr("Secret key")}</span>
          <input type="password" value={secret} placeholder={info.hasSecret ? tr("Saved. Type to replace it.") : ""} autoComplete="off" onChange={(event) => setSecret(event.target.value)} />
        </label>
      </div>
      <div className="cloud-actions">
        <button type="button" className="button small primary" disabled={busy || !changed} onClick={() => void save(draft)}>
          {tr("Save")}
        </button>
        <button
          type="button"
          className="button small secondary"
          disabled={busy || changed || !info.hasSecret}
          onClick={() => {
            setBusy(true);
            void commands
              .testCloud()
              .then((key) => onNotice(tr("Wrote a test file: {0}", [key])))
              .catch((error: unknown) => onError(trBack(String(error))))
              .finally(() => setBusy(false));
          }}
        >
          {tr("Test")}
        </button>
      </div>
    </div>
  );
}
