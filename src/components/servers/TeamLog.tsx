import { useCallback, useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { commands, type TeamLogEntry } from "../../lib/bindings";
import { dateTimeSeconds } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { DropdownMenu, EmptyState, SearchField } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import "./TeamLog.css";

const PAGE = 200;

/** What each filter set last showed this session, so coming back to the tab shows it at once while it refreshes. */
const seen = new Map<string, TeamLogEntry[]>();
const seenKey = (serverId: string, who: string, text: string, refused: boolean) => JSON.stringify([serverId, who, text, refused]);

/** To the second: the log is for finding out exactly when. */
const when = dateTimeSeconds;

/** Typing in a filter waits this long before asking again. */
const TYPING_MS = 250;

/**
 * The host's log of what other people did on this server: Monolink teammates and web
 * panel accounts. Kept on this PC only; only this PC can clear or export it (the app refuses it
 * anywhere else). The same read again within a minute is one line with a count.
 */
export function TeamLog({
  servers,
  confirm,
  onError,
  onDone,
}: {
  /** This PC's servers, for the picker; the log can show all of them together or one. */
  servers: { id: string; name: string }[];
  confirm: (title: string, body: string, confirmLabel: string, danger?: boolean) => Promise<boolean>;
  onError: (message: string) => void;
  onDone: (message: string) => void;
}) {
  const [entries, setEntries] = useState<TeamLogEntry[] | null>(() => seen.get(seenKey("", "", "", false)) ?? null);
  // "" = every server together.
  const [serverId, setServerId] = useState("");
  const serverName = (id: string) => servers.find((one) => one.id === id)?.name ?? (id ? tr("Deleted server") : tr("No server"));
  const [who, setWho] = useState("");
  const [text, setText] = useState("");
  const [refusedOnly, setRefusedOnly] = useState(false);
  const [asked, setAsked] = useState({ who: "", text: "" });
  const [more, setMore] = useState(false);
  const [open, setOpen] = useState<number | null>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => setAsked({ who, text }), TYPING_MS);
    return () => window.clearTimeout(timer);
  }, [who, text]);

  const load = useCallback(() => {
    void commands.teamLog(serverId, asked.who, asked.text, refusedOnly, null, PAGE).then(
      (list) => {
        seen.set(seenKey(serverId, asked.who, asked.text, refusedOnly), list);
        setEntries(list);
        setMore(list.length === PAGE);
        setOpen(null);
      },
      (cause) => onError(String(cause)),
    );
  }, [serverId, asked, refusedOnly, onError]);
  useEffect(load, [load]);

  async function older() {
    const last = entries?.[entries.length - 1];
    if (!last) return;
    try {
      const next = await commands.teamLog(serverId, asked.who, asked.text, refusedOnly, last.at, PAGE);
      setEntries([...(entries ?? []), ...next]);
      setMore(next.length === PAGE);
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function exportCsv() {
    const day = new Date().toISOString().slice(0, 10);
    const target = await save({
      title: tr("Export teammate log"),
      defaultPath: `teammate-log-${day}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (typeof target !== "string") return;
    try {
      const count = await commands.exportTeamLog(target, serverId, asked.who, asked.text, refusedOnly);
      onDone(tr("Saved {0, plural, one {# line} other {# lines}} to a CSV file.", [count]));
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function clear() {
    const ok = await confirm(
      tr("Clear the teammate log?"),
      tr("Every server's log on this PC is deleted, for good. That it was cleared stays in the log."),
      tr("Clear log"),
      true,
    );
    if (!ok) return;
    try {
      await commands.clearTeamLog();
      load();
    } catch (cause) {
      onError(String(cause));
    }
  }

  const filtered = asked.who !== "" || asked.text !== "" || refusedOnly || serverId !== "";

  return (
    <section className={`team-log ${serverId ? "" : "all"}`}>
      <div className="team-log-head">
        <div>
          <strong>{tr("Monolink log")}</strong>
          <p>{tr("Everything Monolink teammates and web panel accounts did on this PC's servers, with what they sent. Only this PC keeps it and can clear it.")}</p>
        </div>
        <div className="team-log-tools">
          <DropdownMenu
            trigger={
              <button type="button" className="wb-icon ghost team-log-more" aria-label={tr("More")} data-tip={tr("More")}>
                <Icon name="dots" />
              </button>
            }
            items={[
              { label: tr("Refresh"), icon: "refresh", onSelect: load },
              { label: tr("Export CSV"), icon: "import", disabled: !entries?.length, onSelect: () => void exportCsv() },
              { label: tr("Clear log"), icon: "trash", danger: true, onSelect: () => void clear() },
            ]}
          />
        </div>
      </div>
      <div className="team-log-filters">
        <Dropdown
          className="team-log-server"
          value={serverId}
          onChange={setServerId}
          options={[{ value: "", label: tr("All servers") }, ...servers.map((one) => ({ value: one.id, label: one.name }))]}
        />
        <SearchField value={who} onChange={setWho} placeholder={tr("Person")} label={tr("Filter by person")} />
        <SearchField className="wide" value={text} onChange={setText} placeholder={tr("Command or what was sent")} label={tr("Filter by command or what was sent")} />
        <button type="button" className="team-log-toggle" aria-pressed={refusedOnly} onClick={() => setRefusedOnly(!refusedOnly)}>
          {refusedOnly && <Icon name="check" />}
          {tr("Refused only")}
        </button>
      </div>
      <div className="team-log-box">
      {entries === null ? (
        <ol className="team-log-list team-log-wait" aria-busy="true" aria-label={tr("Loading…")}>
          {Array.from({ length: 8 }, (_, index) => (
            <li key={index} aria-hidden>
              <span />
            </li>
          ))}
        </ol>
      ) : entries.length === 0 ? (
        filtered ? (
          <EmptyState icon="search" title={tr("Nothing matches")} hint={tr("Try another person or command, or clear the filters.")} />
        ) : (
          <EmptyState icon="users" title={tr("Nothing yet")} hint={tr("When someone uses this server through Monolink or the web panel, what they do shows up here.")} />
        )
      ) : (
        <ol className="team-log-list">
          {entries.map((one, index) => {
            const count = one.count ?? 1;
            const at = one.at ?? 0;
            const lastAt = one.lastAt ?? at;
            return (
              <li key={`${one.at}-${index}`} className={one.ok ? "" : "refused"}>
                <button type="button" className="team-log-row" aria-expanded={open === index} onClick={() => setOpen(open === index ? null : index)}>
                  <time>{when(at)}</time>
                  <span className="team-log-who">{one.who}</span>
                  <span className="team-log-via">{one.via}</span>
                  {!serverId && <span className="team-log-server-name" title={serverName(one.server)}>{serverName(one.server)}</span>}
                  <span className="team-log-what">
                    <code className="team-log-command">{one.command}</code>
                    {count > 1 && (
                      <span className="team-log-count" data-tip={tr("Last at {0}", [when(lastAt)])}>
                        ×{count}
                      </span>
                    )}
                  </span>
                  {!one.ok && <span className="team-log-flag">{tr("Refused")}</span>}
                </button>
                {open === index && (
                  <div className="team-log-more">
                    {count > 1 && (
                      <p className="team-log-repeat">
                        {tr("Asked {0} times, from {1} to {2}.", [count, when(at), when(lastAt)])}
                      </p>
                    )}
                    {one.error && <p className="team-log-error">{one.error}</p>}
                    <pre>{one.detail}</pre>
                  </div>
                )}
              </li>
            );
          })}
        </ol>
      )}
      {more && (
        <button type="button" className="button secondary small team-log-older" onClick={() => void older()}>
          {tr("Older")}
        </button>
      )}
      </div>
    </section>
  );
}
