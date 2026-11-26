import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { commands } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import "./ShellTab.css";

type Output = { session: string; data: string };
type Exit = { session: string; message: string };

function bytesOf(base64: string): Uint8Array {
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let index = 0; index < raw.length; index += 1) out[index] = raw.charCodeAt(index);
  return out;
}

/** Terminal colors from the app's own tokens, so the shell follows light and dark. */
function themeOf(element: HTMLElement) {
  const style = getComputedStyle(element);
  const pick = (name: string, fallback: string) => style.getPropertyValue(name).trim() || fallback;
  return {
    background: pick("--raise", "#141414"),
    foreground: pick("--text", "#e8e8e8"),
    cursor: pick("--text", "#e8e8e8"),
    selectionBackground: pick("--line", "#444444"),
  };
}

/**
 * Shell tab: this PC's shell in the server's folder (PowerShell on Windows), or a login shell on the server's SSH
 * machine. Each open is a new shell; "New shell" starts again after it ends.
 */
export function ShellTab({ serverId, remote, onError }: { serverId: string; remote: boolean; onError: (message: string) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const [round, setRound] = useState(0);
  const [ended, setEnded] = useState<string | null>(null);

  useEffect(() => {
    const host = box.current;
    if (!host) return;
    setEnded(null);
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const term = new Terminal({
      cursorBlink: !reduce,
      // A real fixed-width font: the app's --mono is the UI font, and xterm needs equal cell widths.
      fontFamily: '"Cascadia Mono", Consolas, "DejaVu Sans Mono", Menlo, monospace',
      fontSize: 13,
      scrollback: 5000,
      theme: themeOf(host),
      allowProposedApi: false,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();
    let session: string | null = null;
    let gone = false;
    const offs: (() => void)[] = [];
    // Keys go out one call at a time, in order (calls can otherwise overtake each other); what's typed meanwhile
    // goes in the next call.
    let waiting = "";
    let sending = false;
    const flush = async () => {
      if (sending) return;
      sending = true;
      while (waiting && session) {
        const data = waiting;
        waiting = "";
        await commands.shellInput(session, data).catch(() => {});
      }
      sending = false;
    };
    const typed = term.onData((data) => {
      if (!session) return;
      waiting += data;
      void flush();
    });
    const sized = term.onResize(({ cols, rows }) => {
      if (session) void commands.shellResize(session, cols, rows).catch(() => {});
    });
    const watch = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        // Hidden or zero size: fit again on the next change.
      }
    });
    watch.observe(host);
    const colors = window.matchMedia("(prefers-color-scheme: dark)");
    const recolor = () => {
      term.options.theme = themeOf(host);
    };
    colors.addEventListener("change", recolor);

    void (async () => {
      const keep = (off: () => void) => (gone ? off() : offs.push(off));
      keep(await listen<Output>("shell-output", (event) => {
        if (event.payload.session === session) term.write(bytesOf(event.payload.data));
      }));
      keep(await listen<Exit>("shell-exit", (event) => {
        if (event.payload.session !== session) return;
        session = null;
        setEnded(event.payload.message || tr("The shell ended."));
      }));
      if (gone) return;
      try {
        const opened = await commands.shellOpen(serverId, term.cols, term.rows);
        if (gone) {
          void commands.shellClose(opened);
          return;
        }
        session = opened;
        term.focus();
      } catch (cause) {
        setEnded(String(cause));
        onError(String(cause));
      }
    })();

    return () => {
      gone = true;
      if (session) void commands.shellClose(session);
      offs.forEach((off) => off());
      typed.dispose();
      sized.dispose();
      watch.disconnect();
      colors.removeEventListener("change", recolor);
      // xterm still has a frame queued right after opening (React opens twice in development); disposing in the
      // same frame makes that callback read a renderer that's gone.
      window.requestAnimationFrame(() => window.setTimeout(() => term.dispose(), 0));
    };
  }, [serverId, round, onError]);

  return (
    <section className="shell-tab">
      <div className="shell-tab-head">
        <Icon name="terminal" />
        <strong>{remote ? tr("Shell on the SSH server") : tr("Shell on this PC")}</strong>
        <span className="muted">{tr("Starts in the server's folder. Commands run with your account, like any terminal.")}</span>
        <button type="button" className="button ghost small" onClick={() => setRound((one) => one + 1)}>
          <Icon name="refresh" />
          {tr("New shell")}
        </button>
      </div>
      {ended && (
        <p className="shell-tab-ended" role="status">
          {ended}
        </p>
      )}
      <div className="shell-tab-term" ref={box} />
    </section>
  );
}
