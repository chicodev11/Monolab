import { useEffect, useRef, useState } from "react";
import { tr } from "../../lib/i18n";
import { EMPTY_FILTER, LOG_LEVELS, isEmptyFilter, toggleLevel, type LogFilter } from "../../lib/logFilter";
import { Icon } from "../ui/Icon";
import { TimePicker } from "../ui/TimePicker";

/** Narrowing a log file by level and time. */
export function LogFilters({
  filter,
  onChange,
}: {
  filter: LogFilter;
  onChange: (next: LogFilter) => void;
}) {
  const timed = filter.from !== "" || filter.to !== "";
  const [timeOpen, setTimeOpen] = useState(false);
  // The range shown in the time box. It starts as the whole day and only narrows the log once it is changed.
  const [range, setRange] = useState({ from: "00:00", to: "23:59" });
  function openTime() {
    if (!timeOpen) setRange({ from: filter.from || "00:00", to: filter.to || "23:59" });
    setTimeOpen((open) => !open);
  }
  function pickTime(next: { from: string; to: string }) {
    setRange(next);
    onChange({ ...filter, from: next.from, to: next.to });
  }
  const timeBox = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!timeOpen) return;
    const away = (event: MouseEvent) => {
      const target = event.target as Element | null;
      // The hour and minute lists open in the page body, outside this box.
      if (timeBox.current?.contains(target) || target?.closest?.('[role="listbox"]')) return;
      setTimeOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setTimeOpen(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", escape);
    };
  }, [timeOpen]);
  return (
    <div className="log-filters" role="group" aria-label={tr("Log filters")}>
      <div className="log-levels" role="group" aria-label={tr("Log level")}>
        <Icon name="filter" />
        <button type="button" className={`log-level${filter.levels.length === 0 ? " on" : ""}`} aria-pressed={filter.levels.length === 0} onClick={() => onChange({ ...filter, levels: [] })}>
          {tr("All")}
        </button>
        {LOG_LEVELS.map((level) => (
          <button
            key={level}
            type="button"
            className={`log-level${filter.levels.includes(level) ? " on" : ""}`}
            aria-pressed={filter.levels.includes(level)}
            onClick={() => onChange({ ...filter, levels: toggleLevel(filter.levels, level) })}
          >
            {level === "error" ? tr("Error") : level === "warn" ? tr("Warn") : tr("Info")}
          </button>
        ))}
      </div>
      <div className="log-filter-time" ref={timeBox}>
        <button
          type="button"
          className={`button ghost small${timed ? " chosen" : ""}`}
          aria-expanded={timeOpen}
          onClick={openTime}
        >
          <Icon name="clock" />
          {timed ? `${filter.from} – ${filter.to}` : tr("Time")}
        </button>
        {timeOpen && (
          <div className="log-filter-pop" role="dialog" aria-label={tr("Filter")}>
            <div className="log-filters-time">
              <span>{tr("Only between")}</span>
              {timed && (
                <button type="button" className="button ghost small" onClick={() => {
                  setRange({ from: "00:00", to: "23:59" });
                  onChange({ ...filter, from: "", to: "" });
                }}>
                  {tr("Whole day")}
                </button>
              )}
            </div>
            <div className="log-filter-range">
              <TimePicker value={range.from} onChange={(from) => pickTime({ ...range, from })} />
              <span className="muted">~</span>
              <TimePicker value={range.to} onChange={(to) => pickTime({ ...range, to })} />
            </div>
          </div>
        )}
      </div>
      <span className="live-spacer" />
      {!isEmptyFilter(filter) && (
        <button type="button" className="button ghost small" onClick={() => onChange(EMPTY_FILTER)}>
          <Icon name="close" />
          {tr("Clear filters")}
        </button>
      )}
    </div>
  );
}
