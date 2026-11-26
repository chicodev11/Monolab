import { useState } from "react";
import { tr } from "../../lib/i18n";
import { EMPTY_FILTER, isEmptyFilter, readSavedFilters, sameFilter, withSaved, writeSavedFilters, type LogFilter } from "../../lib/logFilter";
import { DropdownMenu } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { TimePicker } from "../ui/TimePicker";

/** Narrowing a log file by plugin and time, and saving the whole search under a name. */
export function LogFilters({
  filter,
  sources,
  onChange,
  askName,
}: {
  filter: LogFilter;
  /** Plugins and loggers seen in the open file. */
  sources: string[];
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
  const options = [{ value: "", label: tr("Every plugin") }, ...sources.map((one) => ({ value: one, label: one }))];
  if (filter.source && !sources.includes(filter.source)) options.push({ value: filter.source, label: filter.source });
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
      <Dropdown
        value={filter.source}
        options={options}
        search={sources.length > 8 ? tr("Find a plugin") : undefined}
        onChange={(source) => onChange({ ...filter, source })}
      />
      <label className="log-filters-time">
        <span className="switch">
          <input
            type="checkbox"
            aria-label={tr("Only between")}
            checked={timed}
            onChange={(event) => onChange({ ...filter, from: event.target.checked ? "00:00" : "", to: event.target.checked ? "23:59" : "" })}
          />
          <span />
        </span>
        <span>{tr("Only between")}</span>
      </label>
      {timed && (
        <>
          <TimePicker value={filter.from || "00:00"} onChange={(from) => onChange({ ...filter, from })} />
          <span className="muted">{tr("and")}</span>
          <TimePicker value={filter.to || "23:59"} onChange={(to) => onChange({ ...filter, to })} />
        </>
      )}
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
