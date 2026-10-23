export type Status = "running" | "stopped" | "stopping" | "crashed";
export type ShownStatus = Status | "starting" | "checking" | "unreachable" | "missing";

export function shownStatus(instance: { status: string; ready?: boolean; checking?: boolean; unreachable?: boolean; missing?: boolean }): ShownStatus {
  if (instance.checking) return "checking";
  if (instance.missing && instance.status !== "running") return "missing";
  if (instance.unreachable && instance.status !== "running") return "unreachable";
  if (instance.status === "running" && instance.ready === false) return "starting";
  return instance.status as ShownStatus;
}

import { language, t, tr } from "./i18n";

export const labels = new Proxy({} as Record<Status, string>, {
  get: (_target, key: string) => t(`status.${key}`),
});

export type StatusInfo = {
  label: string;
  tone: "ok" | "warn" | "danger" | "muted";
  shape: "circle" | "square" | "diamond" | "ring";
};
export function statusInfo(status: string): StatusInfo {
  switch (status) {
    case "running":
      return { label: t("status.running"), tone: "ok", shape: "circle" };
    case "starting":
      return { label: t("status.starting"), tone: "warn", shape: "circle" };
    case "checking":
      return { label: t("status.checking"), tone: "muted", shape: "square" };
    case "stopping":
      return { label: t("status.stopping"), tone: "warn", shape: "square" };
    case "unreachable":
      return { label: t("status.unreachable"), tone: "warn", shape: "diamond" };
    case "missing":
      return { label: t("status.missing"), tone: "danger", shape: "diamond" };
    case "crashed":
      return { label: t("status.crashed"), tone: "danger", shape: "diamond" };
    default:
      return { label: t("status.stopped"), tone: "muted", shape: "ring" };
  }
}

const locale = () => (language() === "en" ? "en-US" : "ko-KR");

export function fileSize(bytes?: number | null): string {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** A rounded size for estimates: 110 MB, 2.1 GB, 14 GB. */
export function roughSize(bytes: number): string {
  const mb = bytes / 1024 / 1024;
  if (mb < 1024) return `${Math.max(10, Math.round(mb / 10) * 10)} MB`;
  const gb = mb / 1024;
  return gb < 10 ? `${gb.toFixed(1)} GB` : `${Math.round(gb)} GB`;
}

export function relativeTime(atSeconds: number, nowMs: number = Date.now()): string {
  const gap = Math.round(nowMs / 1000 - atSeconds);
  const ahead = gap < 0;
  const distance = Math.abs(gap);
  if (distance < 45) return tr("just now");
  const [value, unit]: [number, "minute" | "hour" | "day" | "month" | "year"] =
    distance < 3600
      ? [Math.max(1, Math.round(distance / 60)), "minute"]
      : distance < 86_400
        ? [Math.round(distance / 3600), "hour"]
        : distance < 86_400 * 30
          ? [Math.round(distance / 86_400), "day"]
          : distance < 86_400 * 365
            ? [Math.round(distance / (86_400 * 30)), "month"]
            : [Math.round(distance / (86_400 * 365)), "year"];
  const ago = {
    minute: tr("{0} min ago", [value]),
    hour: tr("{0} h ago", [value]),
    day: tr("{0} d ago", [value]),
    month: tr("{0} mo ago", [value]),
    year: tr("{0} y ago", [value]),
  };
  const later = {
    minute: tr("in {0} min", [value]),
    hour: tr("in {0} h", [value]),
    day: tr("in {0} d", [value]),
    month: tr("in {0} mo", [value]),
    year: tr("in {0} y", [value]),
  };
  return (ahead ? later : ago)[unit];
}
export { isProxy } from "./kinds";
export const bytes = (mib?: number | null) =>
  typeof mib === "number" && Number.isFinite(mib)
    ? `${(mib / 1024).toFixed(mib % 1024 ? 1 : 0)} GB`
    : "—";
export const used = (mib?: number | null) =>
  typeof mib === "number" && Number.isFinite(mib)
    ? (mib / 1024).toFixed(mib % 1024 ? 1 : 0)
    : "—";
export const uptime = (seconds?: number) =>
  seconds == null
    ? "—"
    : seconds >= 3600
      ? tr("{0} h {1} min", [Math.floor(seconds / 3600), Math.floor((seconds % 3600) / 60)])
      : tr("{0} min", [Math.floor(seconds / 60)]);
export const date = (seconds: number) =>
  new Intl.DateTimeFormat(locale(), {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
export const clock = (milliseconds: number) =>
  new Intl.DateTimeFormat(locale(), {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(milliseconds));

export const today = () => {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
};

export const realVersion = (text: string | null | undefined) =>
  !!text && /^[\x21-\x7e]+$/.test(text.trim()) && /^[0-9]/.test(text.trim());

export const isImage = (name: string) =>
  ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg"].includes(
    (name.split(".").pop() ?? "").toLowerCase(),
  );
