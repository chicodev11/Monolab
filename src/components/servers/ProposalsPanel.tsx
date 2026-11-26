import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { Proposal, ProposalTexts } from "../../lib/bindings";
import { call, onWeb } from "../../lib/bridge";
import { relativeTime } from "../../lib/format";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { foldDiff, lineDiff } from "../../lib/lineDiff";
import { Icon } from "../ui/Icon";

function what(one: Proposal) {
  const paths = one.before.map((file) => file.path).join(", ");
  switch (one.command) {
    case "trash_server_path":
    case "studio_asset_remove":
      return tr("Delete {0}", [paths]);
    case "create_server_file":
      return tr("Make {0}", [paths]);
    case "studio_asset_add_upload":
      return tr("Add {0}", [paths]);
    default:
      return tr("Change {0}", [paths]);
  }
}

function Compare({ texts }: { texts: ProposalTexts }) {
  const rows = useMemo(() => {
    const diff = lineDiff(texts.now ?? "", texts.proposed ?? "");
    return diff ? foldDiff(diff, 3) : null;
  }, [texts]);
  if (texts.proposed === null) return <p className="muted">{tr("Not a text change; there is nothing to compare.")}</p>;
  if (!rows) return <p className="muted">{tr("Too many changed lines to compare.")}</p>;
  return (
    <div className="bk-diff">
      <div className="bk-diff-legend">
        <span className="bk-line remove">− {texts.now === null ? tr("No file now") : tr("Now")}</span>
        <span className="bk-line add">+ {tr("Proposed")}</span>
      </div>
      <pre>
        {rows.map((row, at) =>
          row.kind === "gap" ? (
            <span key={at} className="bk-line gap">
              {tr("{0, plural, one {# unchanged line} other {# unchanged lines}}", [row.count])}
            </span>
          ) : (
            <span key={at} className={`bk-line ${row.kind}`}>
              {row.kind === "add" ? "+ " : row.kind === "remove" ? "− " : "  "}
              {row.text}
            </span>
          ),
        )}
      </pre>
    </div>
  );
}

/**
 * F8: changes waiting for review on this server. The host's window and team Admins/Owners merge or reject them; a
 * Developer sees their own and how they were answered. Shown above the History list while any exist.
 */
export function ProposalsPanel({ serverId, canAnswer, onNotice, onError }: { serverId: string; canAnswer: boolean; onNotice: (message: string) => void; onError: (message: string) => void }) {
  const [list, setList] = useState<Proposal[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [texts, setTexts] = useState<ProposalTexts | null>(null);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setList(onWeb ? await call<Proposal[]>("__proposals", { id: serverId }) : await commands.proposalsList(serverId));
    } catch {
      setList([]);
    }
  }, [serverId]);
  useEffect(() => {
    void load();
    let stop: (() => void) | undefined;
    let gone = false;
    void listen<{ serverId: string; kind: string }>("team-notice", (event) => {
      if (event.payload.serverId === serverId && event.payload.kind.startsWith("proposal")) void load();
    }).then((off) => {
      if (gone) off();
      else stop = off;
    }, () => {});
    return () => {
      gone = true;
      stop?.();
    };
  }, [serverId, load]);

  const show = async (id: string) => {
    if (open === id) {
      setOpen(null);
      return;
    }
    setOpen(id);
    setTexts(null);
    setReason("");
    try {
      setTexts(onWeb ? await call<ProposalTexts>("__proposal_texts", { id: serverId, proposal: id }) : await commands.proposalTexts(serverId, id));
    } catch (cause) {
      onError(trBack(String(cause)));
    }
  };

  const answer = async (id: string, merge: boolean) => {
    setBusy(true);
    try {
      if (merge) await (onWeb ? call("__proposal_merge", { id: serverId, proposal: id }) : commands.proposalMerge(serverId, id));
      else await (onWeb ? call("__proposal_reject", { id: serverId, proposal: id, reason }) : commands.proposalReject(serverId, id, reason));
      onNotice(merge ? tr("Merged.") : tr("Rejected."));
      setOpen(null);
      await load();
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const waiting = list.filter((one) => one.state === "open");
  const answered = list.filter((one) => one.state !== "open").slice(-5).reverse();
  if (!waiting.length && !answered.length) return null;
  return (
    <section className="proposals" aria-label={tr("Change proposals")}>
      <header>
        <Icon name="git" />
        <strong>{tr("Waiting for review")}</strong>
        <span className="memo-count">{waiting.length}</span>
      </header>
      <ul>
        {[...waiting, ...answered].map((one) => (
          <li key={one.id} className={one.state}>
            <button type="button" className="proposal-row" aria-expanded={open === one.id} onClick={() => void show(one.id)}>
              <span>{what(one)}</span>
              <small className="muted">
                {one.by === "host" ? tr("the host") : one.by} · {relativeTime(Number(one.at ?? 0))}
              </small>
              {one.state !== "open" && <span className="chip subtle">{one.state === "merged" ? tr("Merged") : tr("Rejected")}</span>}
            </button>
            {open === one.id && (
              <div className="proposal-body">
                {one.reason && <p className="muted">{tr("Reason: {0}", [one.reason])}</p>}
                {texts ? <Compare texts={texts} /> : <p className="muted">{tr("Loading…")}</p>}
                {one.state === "open" && canAnswer && (
                  <div className="proposal-actions">
                    <input value={reason} placeholder={tr("Why not (optional)")} aria-label={tr("Why not")} onChange={(event) => setReason(event.target.value)} />
                    <button type="button" className="button ghost small" disabled={busy} onClick={() => void answer(one.id, false)}>
                      {tr("Reject")}
                    </button>
                    <button type="button" className="button primary small" disabled={busy} onClick={() => void answer(one.id, true)}>
                      {tr("Merge")}
                    </button>
                  </div>
                )}
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
