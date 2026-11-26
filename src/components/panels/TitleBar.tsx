import { tr } from "../../lib/i18n";
import { useEffect, useState } from "react";
import type { Window } from "@tauri-apps/api/window";

export function TitleBar({
  title,
  nav,
}: {
  title: string;
  nav?: {
    back: boolean;
    forward: boolean;
    onBack: () => void;
    onForward: () => void;
  };
}) {
  const [appWindow, setAppWindow] = useState<Window | null>(null);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let alive = true;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const current = getCurrentWindow();
        const isMaximized = await current.isMaximized();
        if (!alive) return;
        setAppWindow(current);
        setMaximized(isMaximized);
        stop = await current.onResized(() => {
          void current.isMaximized().then(setMaximized);
        });
      } catch {}
    })();
    return () => {
      alive = false;
      stop?.();
    };
  }, []);

  return (
    <div className="titlebar" data-tauri-drag-region>
      <span className="titlebar-name" data-tauri-drag-region>
        <svg className="titlebar-mark" viewBox="0 0 32 32" aria-hidden>
          <rect width="32" height="32" rx="9" />
          <path d="M9 11.5h14M9 16h14M9 20.5h14" />
        </svg>
        {title}
      </span>
      {nav && (
        <div className="titlebar-nav">
          <button
            type="button"
            aria-label={tr("Go back")}
            data-tip={tr("Go back (mouse thumb button)")}
            disabled={!nav.back}
            onClick={nav.onBack}
          >
            <svg viewBox="0 0 16 16" aria-hidden>
              <path d="M10 3 5 8l5 5" />
            </svg>
          </button>
          <button
            type="button"
            aria-label={tr("Go forward")}
            data-tip={tr("Go forward (mouse thumb button)")}
            disabled={!nav.forward}
            onClick={nav.onForward}
          >
            <svg viewBox="0 0 16 16" aria-hidden>
              <path d="m6 3 5 5-5 5" />
            </svg>
          </button>
        </div>
      )}
      {appWindow && (
        <div className="window-controls">
          <button
            type="button"
            aria-label={tr("Minimize")}
            onClick={() => void appWindow.minimize()}
          >
            <svg viewBox="0 0 16 16" aria-hidden>
              <path d="M3.5 8h9" />
            </svg>
          </button>
          <button
            type="button"
            aria-label={maximized ? tr("Restore size") : tr("Maximize")}
            onClick={() => void appWindow.toggleMaximize()}
          >
            {maximized ? (
              <svg viewBox="0 0 16 16" aria-hidden>
                <rect x="3" y="5" width="8" height="8" rx="2" />
                <path d="M5.5 3h5A2.5 2.5 0 0 1 13 5.5v5" />
              </svg>
            ) : (
              <svg viewBox="0 0 16 16" aria-hidden>
                <rect x="3.5" y="3.5" width="9" height="9" rx="2.25" />
              </svg>
            )}
          </button>
          <button
            type="button"
            className="close-window"
            aria-label={tr("Close")}
            onClick={() => void appWindow.close()}
          >
            <svg viewBox="0 0 16 16" aria-hidden>
              <path d="m4.5 4.5 7 7m0-7-7 7" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
