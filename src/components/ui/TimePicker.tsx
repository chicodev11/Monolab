import { Dropdown } from "./Dropdown";

const pad = (value: number) => String(value).padStart(2, "0");

/** "HH:MM" picked with the app's own dropdowns instead of the browser's time input. */
export function TimePicker({ value, onChange, disabled }: { value: string; onChange: (value: string) => void; disabled?: boolean }) {
  const [hourText, minuteText] = value.split(":");
  const hour = Math.min(23, Math.max(0, Number(hourText) || 0));
  const minute = Math.min(59, Math.max(0, Number(minuteText) || 0));
  const minutes = Array.from({ length: 12 }, (_, at) => at * 5);
  if (!minutes.includes(minute)) minutes.push(minute);
  minutes.sort((a, b) => a - b);
  return (
    <span className="time-pick">
      <Dropdown
        className="time-pick-part"
        value={pad(hour)}
        disabled={disabled}
        options={Array.from({ length: 24 }, (_, at) => ({ value: pad(at), label: pad(at) }))}
        onChange={(next) => onChange(`${next}:${pad(minute)}`)}
      />
      <b>:</b>
      <Dropdown
        className="time-pick-part"
        value={pad(minute)}
        disabled={disabled}
        options={minutes.map((at) => ({ value: pad(at), label: pad(at) }))}
        onChange={(next) => onChange(`${pad(hour)}:${next}`)}
      />
    </span>
  );
}
