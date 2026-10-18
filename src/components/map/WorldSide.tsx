import { useEffect, useRef, useState, type ReactNode } from "react";
import { tr } from "../../lib/i18n";
import { date, relativeTime } from "../../lib/format";
import { Icon } from "../ui/Icon";

export type SideWorld = {
  key: string;
  realm: string;
  folder: string;
  sizeMib: number;
  modifiedAt: number;
  hasLevelData: boolean;
};

export type SideSpot = { key: string; label: string; x: number; z: number; kind: "spawn" | "player" | "mark"; online?: boolean; icon?: string | null; id?: string };

export function realmLabel(realm: string) {
  if (realm === "nether") return tr("Nether");
  if (realm === "end") return tr("End");
  if (realm === "overworld") return tr("Overworld");
  return realm.replace(".", ":");
}

type Menu = { id: string; icon: string; label: string; badge?: string; body: ReactNode };

export function WorldSide({
  worlds,
  realm,
  showSize,
  onRealm,
  onSettings,
  onRefresh,
  spots,
  onFocus,
  onRemoveMark,
  onManageMarks,
  onPregen,
  onTrim,
  onShare,
}: {
  worlds: SideWorld[];
  realm: string | null;
  showSize: boolean;
  onRealm: (realm: string) => void;
  onSettings: (key: string) => void;
  onRefresh: () => void;
  spots: SideSpot[];
  onFocus: (x: number, z: number) => void;
  onRemoveMark: (id: string) => void;
  onManageMarks: () => void;
  onPregen: (() => void) | null;
  onTrim: (() => void) | null;
  onShare: (() => void) | null;
}) {
  const [hovered, setHovered] = useState<string | null>(null);
  const [pinned, setPinned] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const open = pinned ?? hovered;

  useEffect(() => {
    if (!pinned) return;
    const away = (event: PointerEvent) => {
      if (root.current && !root.current.contains(event.target as Node) && !(event.target as Element).closest?.(".dropdown-menu, .modal-backdrop")) setPinned(null);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPinned(null);
    };
    document.addEventListener("pointerdown", away);
    window.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      window.removeEventListener("keydown", key);
    };
  }, [pinned]);

  const players = spots.filter((one) => one.kind === "player");
  const marks = spots.filter((one) => one.kind !== "player");

  const menus: Menu[] = [
    {
      id: "worlds",
      icon: "map",
      label: tr("Dimensions"),
      body: (
        <>
          <header>
            <strong>{tr("Dimensions")}</strong>
            <button className="world-side-icon" data-tip={tr("Refresh")} aria-label={tr("Refresh")} onClick={onRefresh}>
              <Icon name="refresh" />
            </button>
          </header>
          {worlds.length === 0 && <p className="world-side-empty">{tr("No world folders found.")}</p>}
          <div className="world-side-list">
            {worlds.map((one) => (
              <div key={one.key} className={`world-dim ${realm === one.realm ? "chosen" : ""}`}>
                <button className="world-dim-main" onClick={() => onRealm(one.realm)}>
                  <span className={`world-dim-dot realm-${["overworld", "nether", "end"].includes(one.realm) ? one.realm : "mod"}`} />
                  <span className="world-dim-text">
                    <strong>{realmLabel(one.realm)}</strong>
                    <small>
                      <code>{one.folder}</code>
                      {showSize && ` · ${one.sizeMib >= 1024 ? `${(one.sizeMib / 1024).toFixed(1)} GB` : `${one.sizeMib.toFixed(1)} MB`}`}
                      {" · "}
                      <span data-tip={date(one.modifiedAt)}>{relativeTime(one.modifiedAt)}</span>
                    </small>
                  </span>
                </button>
                {one.hasLevelData && (
                  <button className="world-side-icon" data-tip={tr("World settings")} aria-label={tr("World settings")} onClick={() => onSettings(one.key)}>
                    <Icon name="settings" />
                  </button>
                )}
              </div>
            ))}
          </div>
        </>
      ),
    },
    {
      id: "marks",
      icon: "users",
      label: tr("Players and bookmarks"),
      badge: players.some((one) => one.online) ? String(players.filter((one) => one.online).length) : undefined,
      body: (
        <>
          {players.length > 0 && (
            <>
              <header>
                <strong>{tr("Players")}</strong>
                <span className="world-side-count">{players.length}</span>
              </header>
              <div className="world-side-list">
                {players.map((one) => (
                  <button key={one.key} className="world-spot" onClick={() => { setPinned(null); setHovered(null); onFocus(one.x, one.z); }}>
                    <span className={`world-spot-face ${one.online ? "on" : ""}`}>{one.icon ? <img src={one.icon} alt="" /> : <i />}</span>
                    <span className="world-spot-text">
                      <strong>{one.label}</strong>
                      <small>
                        {one.online ? tr("Online now") : tr("Offline")} · {one.x}, {one.z}
                      </small>
                    </span>
                  </button>
                ))}
              </div>
            </>
          )}
          <header>
            <strong>{tr("Bookmarks")}</strong>
            <button className="world-side-icon" data-tip={tr("Manage bookmarks")} aria-label={tr("Manage bookmarks")} onClick={onManageMarks}>
              <Icon name="plus" />
            </button>
          </header>
          <div className="world-side-list">
            {marks.map((one) => (
              <div key={one.key} className="world-spot-row">
                <button className="world-spot" onClick={() => { setPinned(null); setHovered(null); onFocus(one.x, one.z); }}>
                  <span className={`world-spot-mark ${one.kind}`} />
                  <span className="world-spot-text">
                    <strong>{one.label}</strong>
                    <small>
                      {one.x}, {one.z}
                    </small>
                  </span>
                </button>
                {one.kind === "mark" && one.id && (
                  <button className="world-side-icon danger" data-tip={tr("Delete")} aria-label={tr("Delete")} onClick={() => onRemoveMark(one.id!)}>
                    <Icon name="trash" />
                  </button>
                )}
              </div>
            ))}
            {marks.every((one) => one.kind !== "mark") && <p className="world-side-empty">{tr("Click the map to add a bookmark")}</p>}
          </div>
        </>
      ),
    },
  ];
  if (onPregen || onTrim || onShare) {
    menus.push({
      id: "tools",
      icon: "wrench",
      label: tr("Tools"),
      body: (
        <>
          <header>
            <strong>{tr("Tools")}</strong>
          </header>
          <div className="world-side-tools">
            {onPregen && (
              <button className="world-tool" onClick={() => { setPinned(null); setHovered(null); onPregen(); }}>
                <Icon name="layers" />
                <span>
                  <strong>{tr("Pre-generate")}</strong>
                  <small>{tr("Make chunks ahead of time")}</small>
                </span>
              </button>
            )}
            {onTrim && (
              <button className="world-tool" onClick={() => { setPinned(null); setHovered(null); onTrim(); }}>
                <Icon name="trash" />
                <span>
                  <strong>{tr("Clean up chunks")}</strong>
                  <small>{tr("Remove chunks nobody stayed in")}</small>
                </span>
              </button>
            )}
            {onShare && (
              <button className="world-tool" onClick={() => { setPinned(null); setHovered(null); onShare(); }}>
                <Icon name="cloud" />
                <span>
                  <strong>{tr("Share the map")}</strong>
                  <small>{tr("A web link friends can open")}</small>
                </span>
              </button>
            )}
          </div>
        </>
      ),
    });
  }

  return (
    <div className="world-menus" ref={root}>
      {menus.map((menu) => (
        <div
          key={menu.id}
          className="world-menu"
          onPointerEnter={() => setHovered(menu.id)}
          onPointerLeave={() => setHovered((was) => (was === menu.id ? null : was))}
        >
          <button
            className={`world-menu-button ${open === menu.id ? "open" : ""} ${pinned === menu.id ? "pinned" : ""}`}
            aria-label={menu.label}
            aria-expanded={open === menu.id}
            onClick={() => setPinned((was) => (was === menu.id ? null : menu.id))}
          >
            <Icon name={menu.icon} />
            {menu.badge && <span className="world-menu-badge">{menu.badge}</span>}
          </button>
          {open === menu.id && <div className="world-menu-panel">{menu.body}</div>}
        </div>
      ))}
    </div>
  );
}
