import { tr } from "../../lib/i18n";
import { useEffect, useMemo, useState } from "react";
import { Icon } from "../ui/Icon";

export type AddonVersion = {
  id: string;
  name: string;
  channel: string;
  gameVersions: string[];
  loaders: string[];
  fits: boolean;
  changelog?: string | null;
  published: string;
  downloads: number;
};

const channelLabels: Record<string, string> = {
  release: tr("Stable"),
  beta: tr("Beta"),
  alpha: tr("Alpha"),
};

function Changelog({ text }: { text: string }) {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  return (
    <div className="changelog">
      {lines.map((line, at) => {
        const body = line.trimEnd();
        const heading = /^#{1,6}\s+/.exec(body);
        if (heading) {
          return <h4 key={at}>{body.slice(heading[0].length)}</h4>;
        }
        const bullet = /^\s*[-*+]\s+/.exec(body);
        if (bullet) {
          return <p className="bullet" key={at}>{body.slice(bullet[0].length)}</p>;
        }
        if (!body.trim()) return <span className="gap" key={at} />;
        return <p key={at}>{body}</p>;
      })}
    </div>
  );
}

function day(iso: string): string {
  if (!iso) return "";
  const when = new Date(iso);
  if (Number.isNaN(when.getTime())) return "";
  return tr("{1}/{2}/{0}", [when.getFullYear(), when.getMonth() + 1, when.getDate()]);
}

export function VersionDialog({
  title,
  iconUrl,
  currentFile,
  versions,
  gameVersion,
  busy,
  onCancel,
  onSwap,
}: {
  title: string;
  iconUrl?: string | null;
  currentFile: string;
  versions: AddonVersion[] | null;
  gameVersion: string;
  busy: boolean;
  onCancel: () => void;
  onSwap: (version: AddonVersion) => void;
}) {
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<string | null>(null);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onCancel();
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [busy, onCancel]);

  const fitting = useMemo(
    () => (versions ?? []).filter((one) => one.fits),
    [versions],
  );
  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return needle
      ? fitting.filter((one) => one.name.toLowerCase().includes(needle))
      : fitting;
  }, [fitting, query]);

  useEffect(() => {
    if (!shown.length) return;
    if (picked && shown.some((item) => item.id === picked)) return;
    setPicked(shown[0].id);
  }, [shown, picked]);

  const one = shown.find((item) => item.id === picked) ?? null;
  const hidden = (versions?.length ?? 0) - fitting.length;

  return (
    <div
      className="modal-backdrop ver-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div className="ver-dialog" role="dialog" aria-modal="true">
        <div className="ver-head">
          {iconUrl ? (
            <img src={iconUrl} alt="" className="ver-icon" />
          ) : (
            <span className="ver-icon empty" aria-hidden>
              <Icon name="copy" />
            </span>
          )}
          <div>
            <strong>{title}</strong>
            <small>{tr("Choose the version to install")}</small>
          </div>
          <button className="icon-button" disabled={busy} onClick={onCancel}>
            <Icon name="arrow" />
          </button>
        </div>

        <div className="ver-body">
          <div className="ver-list">
            <label className="ver-search">
              <Icon name="search" />
              <input
                value={query}
                placeholder={tr("Search versions")}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <div className="ver-scroll">
              {!versions ? (
                <p className="muted tool-empty">{tr("Loading versions…")}</p>
              ) : shown.length ? (
                shown.map((item) => (
                  <button
                    className={`ver-row ${item.id === picked ? "chosen" : ""}`}
                    key={item.id}
                    onClick={() => setPicked(item.id)}
                  >
                    <span className={`ver-dot ${item.channel}`} aria-hidden />
                    <span className="ver-num">{item.name}</span>
                    {currentFile && item.name && currentFile.includes(item.name) && (
                      <span className="ver-now">{tr("Current")}</span>
                    )}
                  </button>
                ))
              ) : (
                <p className="muted tool-empty">
                  {fitting.length
                    ? tr("No results.")
                    : tr("No compatible version for Minecraft {0}.", [gameVersion])}
                </p>
              )}
            </div>
          </div>

          <div className="ver-detail">
            {!one ? (
              <p className="muted tool-empty">{tr("Choose a version on the left.")}</p>
            ) : (
              <>
                <div className="ver-title">
                  <strong>{one.name}</strong>
                  <span className={`ver-chip ${one.channel}`}>
                    {channelLabels[one.channel] ?? one.channel}
                  </span>
                  <time>{day(one.published)}</time>
                </div>
                <p className="ver-meta">
                  {tr("Supported versions")}{" "}
                  {one.gameVersions.length > 1
                    ? `${one.gameVersions[0]} ~ ${one.gameVersions[one.gameVersions.length - 1]}`
                    : one.gameVersions[0] ?? "—"}
                  {one.gameVersions.length > 2
                    ? tr(" ({0})", [one.gameVersions.length])
                    : ""}
                  {" · "}
                  {one.loaders.join(", ")}
                  {" · "}
                  {tr("Download")}{" "}{one.downloads.toLocaleString()}{tr("suffix.runs")}</p>
                {one.changelog ? (
                  <Changelog text={one.changelog} />
                ) : (
                  <p className="muted ver-nochange">
                    {tr("No changelog was provided.")}</p>
                )}
              </>
            )}
          </div>
        </div>

        <div className="ver-foot">
          {hidden > 0 && (
            <span className="ver-toggle">
              {tr("Incompatible")}{" "}{hidden}{tr(" excluded")}</span>
          )}
          <span className="ver-note">
            {tr("Existing files are moved to the trash inside the server folder.")}</span>
          <button className="button ghost" disabled={busy} onClick={onCancel}>
            {tr("Cancel")}</button>
          <button
            className="button primary"
            disabled={busy || !one}
            onClick={() => one && onSwap(one)}
          >
            {busy ? tr("Changing…") : tr("Switch to this version")}
          </button>
        </div>
      </div>
    </div>
  );
}
