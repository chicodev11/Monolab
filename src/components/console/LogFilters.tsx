import { useEffect, useRef, useState } from "react";
import { tr } from "../../lib/i18n";
import { EMPTY_FILTER, isEmptyFilter, readSavedFilters, sameFilter, withSaved, writeSavedFilters, type LogFilter } from "../../lib/logFilter";
import { DropdownMenu, SegmentedControl } from "../primitives";
import { Icon } from "../ui/Icon";
import { TimePicker } from "../ui/TimePicker";

/** Narrowing a log file by level and time, and saving the whole search under a name. */
export function LogFilters({
  filter,
  onChange,
  askName,
}: {
  filter: LogFilter;
  onChange: (next: LogFilter) => void;
  askName: (current: string) => Promise<string | null>;
}) {
  const [saved, setSaved] = useState(readSavedFilters);
  const change = (next: LogFilter[]) => {
    setSaved(next);
    writeSavedFilters(next);
  };
  const timed = filter.from !== "" || filter.to !== "";
  const picked = saved.find((one) => sameFilter(one, filter));
  const [timeOpen, setTimeOpen] = useState(false);
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
  async function save() {
    const name = await askName(picked?.name ?? filter.name ?? "");
    if (!name) return;
    change(withSaved(saved, filter, name));
    onChange({ ...filter, name: name.trim() });
  }
  const items = [
    {
      label: tr("Save these filters"),
      icon: "plus",
      disabled: isEmptyFilter(filter),
      onSelect: () => void save(),
    },
    ...saved.map((one, at) => ({
      heading: at === 0 ? tr("Saved filters") : undefined,
      label: one.name,
      checked: picked?.name === one.name,
      onSelect: () => onChange(one),
    })),
    ...(picked ? [{ label: tr("Forget {0}", [picked.name]), icon: "trash", danger: true, onSelect: () => change(saved.filter((one) => one.name !== picked.name)) }] : []),
    ...(!isEmptyFilter(filter) ? [{ label: tr("Clear filters"), icon: "close", onSelect: () => onChange(EMPTY_FILTER) }] : []),
  ];
  return (
    <div className="log-filters" role="group" aria-label={tr("Log filters")}>
      <SegmentedControl
        className="console-levels"
        value={filter.level}
        onChange={(level) => onChange({ ...filter, level })}
        options={[
          { value: "all", label: tr("All") },
          { value: "error", label: tr("Errors") },
          { value: "warn", label: tr("Warnings") },
          { value: "info", label: tr("Info") },
        ]}
      />
      <div className="log-filter-time" ref={timeBox}>
        <button
          type="button"
          className={`button ghost small${timed ? " chosen" : ""}`}
          aria-expanded={timeOpen}
          onClick={() => setTimeOpen((open) => !open)}
        >
          <Icon name="filter" />
          {timed ? `${filter.from} – ${filter.to}` : tr("Filter")}
        </button>
        {timeOpen && (
          <div className="log-filter-pop" role="dialog" aria-label={tr("Filter")}>
            <label className="log-filters-time">
              <span>{tr("Only between")}</span>
              <span className="switch">
                <input
                  type="checkbox"
                  aria-label={tr("Only between")}
                  checked={timed}
                  onChange={(event) => onChange({ ...filter, from: event.target.checked ? "00:00" : "", to: event.target.checked ? "23:59" : "" })}
                />
                <span />
              </span>
            </label>
            {timed && (
              <div className="log-filter-range">
                <TimePicker value={filter.from || "00:00"} onChange={(from) => onChange({ ...filter, from })} />
                <span className="muted">{tr("and")}</span>
                <TimePicker value={filter.to || "23:59"} onChange={(to) => onChange({ ...filter, to })} />
              </div>
            )}
          </div>
        )}
      </div>
      <span className="live-spacer" />
      <DropdownMenu
        trigger={
          <button type="button" className={`button ghost small${picked ? " chosen" : ""}`} aria-label={tr("Saved filters")}>
            <Icon name="tag" />
            {picked ? picked.name : tr("Saved filters")}
          </button>
        }
        items={items}
      />
    </div>
  );
}
