import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import { language, tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

type Props = {
  server: { id: string; name: string };
  onClose: () => void;
  onNotice: (message: string) => void;
  onOpen: (url: string) => void;
};

type View = Awaited<ReturnType<typeof commands.mapShareState>>;

export function MapShareDialog({ server, onClose, onNotice, onOpen }: Props) {
  const [view, setView] = useState<View | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    commands.mapShareState(server.id).then(setView, (cause) => setError(trBack(String(cause))));
  }, [server.id]);

  useEffect(() => {
    load();
    const timer = window.setInterval(load, 3000);
    return () => window.clearInterval(timer);
  }, [load]);

  const act = async (label: string, work: () => Promise<unknown>) => {
    setError(null);
    setBusy(label);
    try {
      await work();
      load();
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(null);
    }
  };

  const turnOn = () =>
    act("panel", async () => {
      const now = await commands.webPanelState();
      if (!now.on) await commands.setWebPanel(true, now.port);
      if (!now.tunnel) await commands.setWebTunnel(true);
    });

  const create = () =>
    act("create", () =>
      commands.mapShareCreate(server.id, {
        lang: language(),
        title: tr("{0} — world map"),
        loading: tr("Loading…"),
        missing: tr("This dimension hasn't been generated yet."),
        zoomIn: tr("Zoom in"),
        zoomOut: tr("Zoom out"),
        night: tr("Night"),
        "realm.overworld": tr("Overworld"),
        "realm.nether": tr("Nether"),
        "realm.end": tr("End"),
      }),
    );

  const copy = async () => {
    if (!view?.url) return;
    await navigator.clipboard.writeText(view.url).catch(() => {});
    onNotice(tr("Copied the link."));
  };

  return createPortal(
    <div className="modal-backdrop roomy" onClick={() => !busy && onClose()}>
      <div className="add-modal share-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Share the map")}</strong> · {server.name}
          </p>
          <div className="modal-top-actions">
            <button className="close in-flow" aria-label={tr("Close")} disabled={!!busy} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        <p className="muted trim-lead">
          {tr("Friends open a web page with this world's map. They can move around, zoom and switch dimensions. Players, files and server controls are not shown.")}
        </p>
        {error && <p className="modal-error">{error}</p>}
        {!view ? (
          <p className="muted tool-empty">{tr("Loading…")}</p>
        ) : !view.panelOn || !view.tunnelOn ? (
          <div className="share-step">
            <Icon name="cloud" />
            <span>
              <strong>{tr("Needs the web panel's outside address")}</strong>
              <small>{tr("The link goes through the web panel and its Cloudflare address, so friends outside your home can open it.")}</small>
            </span>
            <button className="button primary" disabled={!!busy} onClick={() => void turnOn()}>
              {busy === "panel" ? tr("Turning on…") : view.panelOn ? tr("Turn on outside address") : tr("Turn on web panel")}
            </button>
          </div>
        ) : !view.shared ? (
          <div className="share-step">
            <Icon name="map" />
            <span>
              <strong>{tr("Not shared yet")}</strong>
              <small>{tr("A new random link is made. Only people who have it can see the map.")}</small>
            </span>
            <button className="button primary" disabled={!!busy} onClick={() => void create()}>
              {tr("Create link")}
            </button>
          </div>
        ) : (
          <>
            <div className="share-link">
              <code>{view.url}</code>
              <button className="button secondary small" onClick={() => void copy()}>
                <Icon name="copy" />
                {tr("Copy")}
              </button>
              <button className="button ghost small" onClick={() => onOpen(view.url)}>
                {tr("Open")}
              </button>
            </div>
            <p className="muted share-note">
              {tr("The Cloudflare address changes when the web panel restarts, so send the new link again then. The link stops working when you stop sharing or turn the web panel off.")}
            </p>
            <div className="trim-actions">
              <button className="button ghost" disabled={!!busy} onClick={() => void create()}>
                {tr("Make a new link")}
              </button>
              <button className="button danger" disabled={!!busy} onClick={() => void act("stop", () => commands.mapShareRemove(server.id))}>
                {tr("Stop sharing")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
