import { tr } from "../../lib/i18n";
import { Checkbox } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { linesOf, textOf, type StudioData } from "./StudioPreview";

/** The same lists as `crates/studio-format/src/announcement.rs`. */
export const ANNOUNCEMENT_SCHEDULES = ["every", "daily", "after-start", "after-join"] as const;
export const ANNOUNCEMENT_DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] as const;

export function scheduleOf(data: StudioData): string {
  const given = textOf(data, "schedule").trim().toLowerCase();
  return (ANNOUNCEMENT_SCHEDULES as readonly string[]).includes(given) ? given : "every";
}

function scheduleHint(schedule: string): string {
  switch (schedule) {
    case "daily":
      return tr("At clock times, on the days you pick (the server's clock).");
    case "after-start":
      return tr("Once, a while after the server starts or the studio reloads; it can repeat after that.");
    case "after-join":
      return tr("A while after each player joins, to that player only.");
    default:
      return tr("The next message every few seconds, to everyone it shows to.");
  }
}

function Seconds({ data, name, label, fallback, least, hint, onChange }: { data: StudioData; name: string; label: string; fallback: number; least: number; hint?: string; onChange: (next: StudioData) => void }) {
  const value = data[name];
  return (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        value={typeof value === "number" && Number.isFinite(value) ? value : fallback}
        onChange={(event) => {
          const read = Number(event.target.value);
          if (Number.isFinite(read)) onChange({ ...data, [name]: Math.max(least, Math.round(read)) });
        }}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** "8:05" reads as "08:05", for the time inputs; anything else is left as typed. */
function clockValue(text: string): string {
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  return match ? `${match[1].padStart(2, "0")}:${match[2]}` : text;
}

/**
 * Schedule fields for an announcement: `every` (everySeconds), `daily` (times + days), `after-start`
 * (delaySeconds + repeatSeconds) and `after-join` (delaySeconds). Fields of other schedules are kept in the file.
 */
export function AnnouncementSchedule({ data, onChange }: { data: StudioData; onChange: (next: StudioData) => void }) {
  const schedule = scheduleOf(data);
  const times = linesOf(data, "times");
  const days = linesOf(data, "days").map((one) => one.trim().toLowerCase());
  const setTimes = (next: string[]) => onChange({ ...data, times: next });
  const toggleDay = (day: string) => {
    const picked = days.some((one) => one.startsWith(day));
    const next = picked ? days.filter((one) => !one.startsWith(day)) : [...days, day];
    onChange({ ...data, days: ANNOUNCEMENT_DAYS.filter((one) => next.some((given) => given.startsWith(one))) });
  };
  return (
    <div className="studio-schedule">
      <label className="field studio-field">
        <span>{tr("Schedule")}</span>
        <Dropdown
          value={schedule}
          onChange={(value) => onChange({ ...data, schedule: value })}
          options={ANNOUNCEMENT_SCHEDULES.map((one) => ({ value: one, label: one }))}
        />
        <small>{scheduleHint(schedule)}</small>
      </label>
      {schedule === "every" && <Seconds data={data} name="everySeconds" label={tr("Every (seconds)")} fallback={300} least={10} onChange={onChange} />}
      {schedule === "daily" && (
        <>
          <div className="field studio-field">
            <span>{tr("Times")}</span>
            <div className="studio-times">
              {times.map((one, at) => (
                <div key={at} className="studio-time">
                  <input
                    type="time"
                    value={clockValue(one)}
                    aria-label={tr("Time")}
                    onChange={(event) => setTimes(times.map((was, index) => (index === at ? event.target.value : was)))}
                  />
                  <button type="button" className="icon-button small" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setTimes(times.filter((_, index) => index !== at))}>
                    <Icon name="close" />
                  </button>
                </div>
              ))}
              <button type="button" className="button ghost small" onClick={() => setTimes([...times, "12:00"])}>
                <Icon name="plus" />
                {tr("Add time")}
              </button>
            </div>
            <small>{tr("24-hour clock, the server's time zone.")}</small>
          </div>
          <div className="field studio-field">
            <span>{tr("Days")}</span>
            <div className="studio-days">
              {ANNOUNCEMENT_DAYS.map((day) => (
                <Checkbox key={day} className="studio-day" label={day} checked={days.some((one) => one.startsWith(day))} onChange={() => toggleDay(day)} />
              ))}
            </div>
            <small>{tr("Pick none to send it every day.")}</small>
          </div>
        </>
      )}
      {schedule === "after-start" && (
        <div className="studio-fields-row">
          <Seconds data={data} name="delaySeconds" label={tr("After (seconds)")} fallback={60} least={0} onChange={onChange} />
          <Seconds data={data} name="repeatSeconds" label={tr("Then every (seconds)")} fallback={0} least={0} hint={tr("0 sends it only once.")} onChange={onChange} />
        </div>
      )}
      {schedule === "after-join" && <Seconds data={data} name="delaySeconds" label={tr("After joining (seconds)")} fallback={60} least={0} onChange={onChange} />}
    </div>
  );
}
