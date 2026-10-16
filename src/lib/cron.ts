import { tr } from "./i18n";

export type Cron = { minute: string; hour: string; day_of_month: string; month: string; day_of_week: string };

export type Easy =
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "hours"; every: number }
  | { kind: "minutes"; every: number }
  | { kind: "weekly"; days: number[]; hour: number; minute: number }
  | { kind: "custom" };

export const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

const num = (text: string) => (/^\d+$/.test(text) ? Number(text) : NaN);

export function toCron(easy: Easy, fallback: Cron): Cron {
  const every = { day_of_month: "*", month: "*" };
  switch (easy.kind) {
    case "daily":
      return { minute: String(easy.minute), hour: String(easy.hour), ...every, day_of_week: "*" };
    case "hours":
      return { minute: "0", hour: easy.every <= 1 ? "*" : `*/${easy.every}`, ...every, day_of_week: "*" };
    case "minutes":
      return { minute: easy.every <= 1 ? "*" : `*/${easy.every}`, hour: "*", ...every, day_of_week: "*" };
    case "weekly":
      return {
        minute: String(easy.minute),
        hour: String(easy.hour),
        ...every,
        day_of_week: easy.days.length ? [...easy.days].sort().join(",") : "*",
      };
    default:
      return fallback;
  }
}

export function fromCron(cron: Cron): Easy {
  const { minute, hour, day_of_month: dom, month, day_of_week: dow } = cron;
  if (dom !== "*" || month !== "*") return { kind: "custom" };
  const step = (text: string) => (text === "*" ? 1 : text.startsWith("*/") ? num(text.slice(2)) : NaN);
  if (dow === "*" && hour === "*" && !Number.isNaN(step(minute))) return { kind: "minutes", every: step(minute) };
  if (dow === "*" && minute === "0" && !Number.isNaN(step(hour))) return { kind: "hours", every: step(hour) };
  if (!Number.isNaN(num(minute)) && !Number.isNaN(num(hour))) {
    if (dow === "*") return { kind: "daily", hour: num(hour), minute: num(minute) };
    const days = dow.split(",").map(num);
    if (days.every((one) => one >= 0 && one <= 7)) {
      return { kind: "weekly", days: [...new Set(days.map((one) => one % 7))], hour: num(hour), minute: num(minute) };
    }
  }
  return { kind: "custom" };
}

const clock = (hour: number, minute: number) => `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;

export function describeCron(cron: Cron): string {
  const easy = fromCron(cron);
  switch (easy.kind) {
    case "daily":
      return tr("Daily at {0}", [clock(easy.hour, easy.minute)]);
    case "hours":
      return easy.every <= 1 ? tr("Every hour on the hour") : tr("Every {0} hours (on the hour)", [easy.every]);
    case "minutes":
      return easy.every <= 1 ? tr("Every minute") : tr("Every {0} min", [easy.every]);
    case "weekly":
      return tr("Every {0} at {1}", [easy.days.map((day) => tr(DAYS[day])).join("·"), clock(easy.hour, easy.minute)]);
    default:
      return `${cron.minute} ${cron.hour} ${cron.day_of_month} ${cron.month} ${cron.day_of_week}`;
  }
}
