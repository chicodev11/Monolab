import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { hasStyle, parseAnsi, type AnsiPiece } from "../../lib/ansi";
import { groupConsole, isErrorTail, readChat, readLevel, readPlayerCommand, tidyLogLine } from "../../lib/console";
import { useHead } from "../../lib/heads";
import { tr } from "../../lib/i18n";
import { useServerLogs, type LogLine } from "../../lib/logStore";
import { Icon } from "../ui/Icon";

const LINE_ESTIMATE = 22;

export type ConsoleLevel = "all" | "error" | "warn" | "info";

export function ConsoleOutput({
  serverId,
  filter,
  level = "all",
}: {
  serverId: string;
  filter: string;
  level?: ConsoleLevel;
}) {
  const logs = useServerLogs(serverId);
  return <LogLines logs={logs} filter={filter} level={level} />;
}

export function ConsoleLevels({
  serverId,
  value,
  onChange,
}: {
  serverId: string;
  value: ConsoleLevel;
  onChange: (level: ConsoleLevel) => void;
}) {
  const logs = useServerLogs(serverId);
  const counts = useMemo(() => {
    let error = 0;
    let warn = 0;
    for (const log of logs) {
      if (log.noise) continue;
      const found = readLevel(log.line);
      if (found === "error" || (log.stream === "stderr" && !isErrorTail(log.line))) error += 1;
      else if (found === "warn") warn += 1;
    }
    return { error, warn };
  }, [logs]);
  const options: { value: ConsoleLevel; label: string; count?: number }[] = [
    { value: "all", label: tr("All") },
    { value: "error", label: tr("Errors"), count: counts.error },
    { value: "warn", label: tr("Warnings"), count: counts.warn },
    { value: "info", label: tr("Info") },
  ];
  return (
    <div className="wb-chips" role="tablist" aria-label={tr("Log level")}>
      {options.map((one) => (
        <button
          key={one.value}
          type="button"
          role="tab"
          aria-selected={value === one.value}
          className={`wb-chip ${value === one.value ? "on" : ""} ${one.value === "error" && one.count ? "danger" : ""}`}
          onClick={() => onChange(one.value)}
        >
          {one.label}
          {one.count ? <b>{one.count}</b> : null}
        </button>
      ))}
    </div>
  );
}

export function LogLines({
  logs,
  filter,
  level = "all",
  emptyText,
}: {
  logs: readonly LogLine[];
  filter: string;
  level?: ConsoleLevel;
  emptyText?: string;
}) {
  const chunks = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    let mine = needle ? logs.filter((log) => log.line.toLowerCase().includes(needle)) : logs;
    if (level !== "all") {
      mine = mine.filter((log) => {
        const found = readLevel(log.line);
        if (level === "error") return found === "error" || log.stream === "stderr" || isErrorTail(log.line);
        if (level === "warn") return found === "warn";
        return found === null && log.stream !== "stderr" && !isErrorTail(log.line);
      });
    }
    return groupConsole(mine, true);
  }, [logs, filter, level]);

  const [followTail, setFollowTail] = useState(true);
  const userMoved = useRef(false);
  const box = useRef<HTMLDivElement | null>(null);
  const glide = useRef(0);
  const born = useRef(performance.now());
  const virtualizer = useVirtualizer({
    count: chunks.length,
    getScrollElement: () => box.current,
    estimateSize: () => LINE_ESTIMATE,
    getItemKey: (index) => index,
    overscan: 12,
  });
  const total = virtualizer.getTotalSize();
  const attach = useCallback((node: HTMLDivElement | null) => {
    box.current = node;
    if (node) node.scrollTop = node.scrollHeight;
  }, []);
  useEffect(() => {
    if (!followTail) return;
    if (glide.current) cancelAnimationFrame(glide.current);
    const step = () => {
      glide.current = 0;
      const node = box.current;
      if (!node) return;
      const target = node.scrollHeight - node.clientHeight;
      const gap = target - node.scrollTop;
      if (gap <= 1 || (userMoved.current && gap > 60 && gap < node.clientHeight * 2)) return;
      const settling = performance.now() - born.current < 1500;
      node.scrollTop = settling || gap > node.clientHeight * 2 ? target : node.scrollTop + Math.max(1, gap * 0.35);
      glide.current = requestAnimationFrame(step);
    };
    glide.current = requestAnimationFrame(step);
    return () => {
      if (glide.current) cancelAnimationFrame(glide.current);
      glide.current = 0;
    };
  }, [chunks, followTail, total]);
  const followRef = useRef(followTail);
  followRef.current = followTail;
  // While reading older lines, old lines drop off the front of the capped log; keep the same line in view.
  const anchor = useRef<{ row: LogLine; index: number; offset: number } | null>(null);
  useLayoutEffect(() => {
    const node = box.current;
    const held = anchor.current;
    if (!node || followTail || !held) return;
    const index = chunks.findIndex((chunk) => (chunk.kind === "error" ? chunk.rows.includes(held.row) : chunk.row === held.row));
    if (index < 0 || index === held.index) return;
    const start = virtualizer.getOffsetForIndex(index, "start")?.[0];
    if (start === undefined) return;
    held.index = index;
    node.scrollTop = start + held.offset;
  }, [chunks, followTail, virtualizer]);
  useEffect(() => {
    const node = box.current;
    if (!node) return;
    let lastHeight = node.clientHeight;
    const watch = new ResizeObserver(() => {
      const shrunk = node.clientHeight !== lastHeight;
      lastHeight = node.clientHeight;
      if (shrunk && followRef.current && !userMoved.current) node.scrollTop = node.scrollHeight - node.clientHeight;
    });
    watch.observe(node);
    return () => watch.disconnect();
  }, []);
  const onScroll = (node: HTMLDivElement) => {
    const atBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 60;
    const top = virtualizer.getVirtualItems().find((item) => item.end > node.scrollTop);
    const chunk = top ? chunks[top.index] : undefined;
    anchor.current = !atBottom && top && chunk ? { row: chunk.kind === "error" ? chunk.rows[0] : chunk.row, index: top.index, offset: node.scrollTop - top.start } : null;
    if (atBottom) {
      userMoved.current = false;
      setFollowTail(true);
      return;
    }
    if (userMoved.current) setFollowTail(false);
  };
  const moved = () => {
    userMoved.current = true;
  };

  return (
    <>
      <div
        className="console-output live raw"
        ref={attach}
        onWheel={moved}
        onTouchMove={moved}
        onPointerDown={moved}
        onScroll={(event) => onScroll(event.currentTarget)}
      >
        {chunks.length ? (
          <div className="console-virtual" style={{ height: total }}>
            {virtualizer.getVirtualItems().map((item) => {
              const chunk = chunks[item.index];
              return (
                <div
                  key={item.key}
                  className={`console-item ${chunk.kind === "error" ? "is-error" : ""}`}
                  data-index={item.index}
                  ref={virtualizer.measureElement}
                  style={{ transform: `translateY(${item.start}px)` }}
                >
                  {chunk.kind === "error" ? (
                    <div className="console-error">
                      <b>
                        <Icon name="alert" />
                        {tr("An error occurred!")}
                      </b>
                      {chunk.rows.map((row, at) => (
                        <RawLine key={at} row={row} />
                      ))}
                    </div>
                  ) : (
                    <RawLine row={chunk.row} />
                  )}
                </div>
              );
            })}
          </div>
        ) : emptyText ? (
          <p className="muted log-lines-empty">{emptyText}</p>
        ) : null}
      </div>
      {!followTail && (
        <button
          className="console-tail"
          type="button"
          aria-label={tr("Jump to latest")}
          data-tip={tr("Jump to latest")}
          onClick={() => {
            userMoved.current = false;
            setFollowTail(true);
          }}
        >
          <Icon name="arrow-down" />
        </button>
      )}
    </>
  );
}

function pieceStyle(piece: AnsiPiece): CSSProperties | undefined {
  const { color, background, bold, dim, italic, underline, strike } = piece.style;
  if (!color && !background && !bold && !dim && !italic && !underline && !strike) return undefined;
  return {
    color,
    background,
    fontWeight: bold ? 600 : undefined,
    opacity: dim ? 0.65 : undefined,
    fontStyle: italic ? "italic" : undefined,
    textDecoration: [underline && "underline", strike && "line-through"].filter(Boolean).join(" ") || undefined,
  };
}

function RawLine({ row }: { row: LogLine }) {
  if (row.stream === "input") return <SentLine text={row.line} />;
  const chat = readChat(row.line);
  if (chat) return <ChatLine player={chat.player} text={chat.text} />;
  const typed = readPlayerCommand(row.line);
  if (typed) return <PlayerCommandLine player={typed.player} command={typed.command} />;
  return <PlainLine row={row} />;
}

function Head({ name }: { name: string }) {
  const head = useHead(name);
  return head ? (
    <img className="console-face" src={head} alt="" />
  ) : (
    <span className="console-face letter" aria-hidden>
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
}

function ChatLine({ player, text }: { player: string; text: string }) {
  return (
    <div className="console-chat">
      <Head name={player} />
      <div className="console-chat-body">
        <b>{player}</b>
        <p className="console-bubble">{text}</p>
      </div>
    </div>
  );
}

function PlayerCommandLine({ player, command }: { player: string; command: string }) {
  return (
    <div className="console-cmd">
      <Head name={player} />
      <b>{player}</b>
      <code>{command}</code>
    </div>
  );
}

function SentLine({ text }: { text: string }) {
  return (
    <div className="console-cmd mine">
      <span className="console-cmd-mark" aria-hidden>
        <Icon name="terminal" />
      </span>
      <b>{tr("Me")}</b>
      <code>{text.startsWith("/") ? text : "/" + text}</code>
    </div>
  );
}

function PlainLine({ row }: { row: LogLine }) {
  const pieces = useMemo(() => parseAnsi(tidyLogLine(row.ansi || row.line)), [row.ansi, row.line]);
  const tone = hasStyle(pieces)
    ? ""
    : (readLevel(row.line) ?? (row.stream === "stderr" || isErrorTail(row.line) ? "error" : "info"));
  return (
    <p className={`console-line raw ${tone}`}>
      {pieces.map((piece, at) => (
        <span key={at} style={pieceStyle(piece)}>
          {piece.text}
        </span>
      ))}
    </p>
  );
}
