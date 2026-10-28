import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import { language, tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

type Props = {
  server: { id: string; name: string; panelHost?: string };
  onClose: () => void;
  onNotice: (message: string) => void;
  onOpen: (url: string) => void;
};

type View = Awaited<ReturnType<typeof commands.mapWebState>>;
type Probe = "idle" | "testing" | "open" | "closed";

function labels(): Record<string, string> {
  return {
    lang: language(),
    title: tr("{0} — world map"),
    loading: tr("Loading…"),
    missing: tr("This dimension hasn't been generated yet."),
    zoomIn: tr("Zoom in"),
    zoomOut: tr("Zoom out"),
    night: tr("Night"),
    players: tr("Players"),
    none: tr("No one is online."),
    copyLink: tr("Copy a link to this spot"),
    copied: tr("Copied the link."),
    height: tr("Map height"),
    auto: tr("Auto"),
    drawing: tr("Drawing {0}/{1}"),
    "realm.overworld": tr("Overworld"),
    "realm.overworld-cave": tr("Caves"),
    "realm.nether": tr("Nether"),
    "realm.end": tr("End"),
  };
}

export function MapShareDialog({ server, onClose, onNotice, onOpen }: Props) {
  const [view, setView] = useState<View | null>(null);
  const [port, setPort] = useState("8100");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [address, setAddress] = useState<string | null>(null);
  const [looking, setLooking] = useState(false);
  const [probe, setProbe] = useState<Probe>("idle");
  const touched = useRef(false);

  const load = useCallback(() => {
    commands.mapWebState(server.id).then(
      (next) => {
        setView(next);
        if (!touched.current) setPort(String(next.port));
      },
      (cause) => setError(trBack(String(cause))),
    );
  }, [server.id]);

  useEffect(() => {
    load();
    const timer = window.setInterval(load, 3000);
    return () => window.clearInterval(timer);
  }, [load]);

  const save = async (change: Partial<{ enabled: boolean; port: number; private: boolean; players: boolean; renew: boolean }>) => {
    if (!view) return;
    const nextPort = change.port ?? (Number(port) || view.port);
    setError(null);
    setBusy(true);
    try {
      const next = await commands.mapWebSet(
        server.id,
        change.enabled ?? view.enabled,
        nextPort,
        change.private ?? view.private,
        change.players ?? view.players,
        change.renew ?? false,
        labels(),
      );
      touched.current = false;
      setView(next);
      setPort(String(next.port));
      setProbe("idle");
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const lookUp = async () => {
    setLooking(true);
    setError(null);
    try {
      setAddress(await commands.publicAddress());
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setLooking(false);
    }
  };

  const test = async () => {
    if (!address || !view) return;
    setProbe("testing");
    try {
      setProbe((await commands.mapWebProbe(address, view.port)) ? "open" : "closed");
    } catch (cause) {
      setError(trBack(String(cause)));
      setProbe("idle");
    }
  };

  const copy = async (url: string) => {
    await navigator.clipboard.writeText(url).catch(() => {});
    onNotice(tr("Copied the link."));
  };

  const portChanged = view !== null && Number(port) !== view.port;
  const outside = view && address ? `http://${address.includes(":") ? `[${address}]` : address}:${view.port}${view.path}` : "";

  const state = !view?.enabled
    ? null
    : view.error
      ? { tone: "bad", text: tr("Couldn't start: {0}", [view.error]) }
      : view.running
        ? { tone: "ok", text: tr("Running on port {0}", [view.port]) }
        : { tone: "wait", text: tr("Starts together with the server.") };

  return createPortal(
    <div className="modal-backdrop roomy" onClick={() => !busy && onClose()}>
      <div className="add-modal share-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Share the map")}</strong> · {server.name}
          </p>
          <div className="modal-top-actions">
            <button className="close in-flow" aria-label={tr("Close")} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        <p className="muted trim-lead">
          {tr("The map is served by the Monolab add-on inside the server, like BlueMap. Friends open a web page where they can move around, zoom and switch dimensions.")}
        </p>
        {error && <p className="modal-error">{error}</p>}
        {!view ? (
          <p className="muted tool-empty">{tr("Loading…")}</p>
        ) : !view.addonReady ? (
          <div className="share-step">
            <Icon name="map" />
            <span>
              <strong>{tr("The add-on isn't ready")}</strong>
              <small>{trBack(view.reason)}</small>
            </span>
          </div>
        ) : (
          <>
            <div className="share-rows">
              <div className="share-row">
                <span>
                  <strong>{tr("Publish the map")}</strong>
                  <small>{tr("Starts a small web server inside the game server. It runs only while the server runs.")}</small>
                </span>
                <label className="switch">
                  <input type="checkbox" checked={view.enabled} disabled={busy} onChange={(event) => void save({ enabled: event.target.checked })} />
                  <span />
                </label>
              </div>
              {state && (
                <div className={`share-state ${state.tone}`}>
                  <i />
                  {state.text}
                </div>
              )}
              <div className="share-row">
                <span>
                  <strong>{tr("Port")}</strong>
                  <small>{tr("Forward this port on your router. 1024–65535.")}</small>
                </span>
                <span className="share-port">
                  <input
                    className="input"
                    inputMode="numeric"
                    value={port}
                    onChange={(event) => {
                      touched.current = true;
                      setPort(event.target.value.replace(/[^0-9]/g, "").slice(0, 5));
                    }}
                    onKeyDown={(event) => event.key === "Enter" && portChanged && void save({ port: Number(port) })}
                  />
                  {portChanged && (
                    <button className="button primary small" disabled={busy} onClick={() => void save({ port: Number(port) })}>
                      {tr("Apply")}
                    </button>
                  )}
                </span>
              </div>
              <div className="share-row">
                <span>
                  <strong>{tr("Secret address")}</strong>
                  <small>{tr("Adds a random part to the address, so only people with the link can open the map.")}</small>
                </span>
                <label className="switch">
                  <input type="checkbox" checked={view.private} disabled={busy} onChange={(event) => void save({ private: event.target.checked })} />
                  <span />
                </label>
              </div>
              <div className="share-row">
                <span>
                  <strong>{tr("Show players")}</strong>
                  <small>{tr("Online players appear on the map with their names.")}</small>
                </span>
                <label className="switch">
                  <input type="checkbox" checked={view.players} disabled={busy} onChange={(event) => void save({ players: event.target.checked })} />
                  <span />
                </label>
              </div>
            </div>

            {view.enabled && (
              <div className="share-howto">
                <div className="share-num">
                  <b>1</b>
                  <span>
                    <strong>{tr("Forward the port on your router")}</strong>
                    <small>
                      {server.panelHost
                        ? tr("On a hosting panel, add port {0} to the server's network allocations first, then forward it on the machine's router if needed.", [view.port])
                        : tr("In your router's settings, forward TCP port {0} to this computer.", [view.port])}
                    </small>
                  </span>
                </div>
                {view.lanUrl && (
                  <div className="share-link">
                    <small>{tr("Home network")}</small>
                    <code>{view.lanUrl}</code>
                    <button className="button secondary small" onClick={() => void copy(view.lanUrl)}>
                      <Icon name="copy" />
                      {tr("Copy")}
                    </button>
                    <button className="button ghost small" onClick={() => onOpen(view.lanUrl)}>
                      {tr("Open")}
                    </button>
                  </div>
                )}
                <div className="share-num">
                  <b>2</b>
                  <span>
                    <strong>{tr("Send friends the outside address")}</strong>
                    <small>{tr("The address comes from your internet connection, so it can change. Look it up when you need it.")}</small>
                  </span>
                </div>
                {outside ? (
                  <>
                    <div className="share-link">
                      <small>{tr("Outside")}</small>
                      <code>{outside}</code>
                      <button className="button secondary small" onClick={() => void copy(outside)}>
                        <Icon name="copy" />
                        {tr("Copy")}
                      </button>
                      <button className="button ghost small" disabled={probe === "testing"} onClick={() => void test()}>
                        {probe === "testing" ? tr("Testing…") : tr("Test")}
                      </button>
                    </div>
                    {probe === "open" && <p className="share-result ok">{tr("The port answers from here.")}</p>}
                    {probe === "closed" && (
                      <p className="share-result bad">
                        {tr("Nothing answered. The server may be off, the port isn't forwarded yet, or your router doesn't allow testing from inside the house — ask a friend to try.")}
                      </p>
                    )}
                  </>
                ) : (
                  <button className="button secondary" disabled={looking} onClick={() => void lookUp()}>
                    <Icon name="cloud" />
                    {looking ? tr("Looking up…") : tr("Look up my public address")}
                  </button>
                )}
                {view.private && (
                  <div className="trim-actions">
                    <button className="button ghost" disabled={busy} onClick={() => void save({ renew: true })}>
                      {tr("Make a new secret")}
                    </button>
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
