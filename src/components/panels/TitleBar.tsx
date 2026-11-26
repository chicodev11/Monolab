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
            <svg viewBox="0 0 12 12" aria-hidden>
              <path d="M2 6h8" />
            </svg>
          </button>
          <button
            type="button"
            aria-label={maximized ? tr("Restore size") : tr("Maximize")}
            onClick={() => void appWindow.toggleMaximize()}
          >
            {maximized ? (
              <svg viewBox="0 0 12 12" aria-hidden>
                <path d="M3.5 3.5h5v5h-5z" />
                <path d="M2 8.5v-5h5" />
              </svg>
            ) : (
              <svg viewBox="0 0 12 12" aria-hidden>
                <path d="M2.5 2.5h7v7h-7z" />
              </svg>
            )}
          </button>
          <button
            type="button"
            className="close-window"
            aria-label={tr("Close")}
            onClick={() => void appWindow.close()}
          >
            <svg viewBox="0 0 12 12" aria-hidden>
              <path d="M3 3l6 6M9 3l-6 6" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
