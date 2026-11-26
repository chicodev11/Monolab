/** Team notices (F4): which ones this window shows. Pure, so it can be tested without the app. */

export type TeamNotice = { serverId: string; kind: string; to: string; text: string };
export type NoticeSwitches = { assigned: boolean; savedOver: boolean; assetAdded: boolean; proposals: boolean };

export const DEFAULT_SWITCHES: NoticeSwitches = { assigned: true, savedOver: true, assetAdded: true, proposals: true };
const KEY = "monolab-team-notices";

/** The switches on this PC (Settings → Notifications), all on until changed. */
export function readSwitches(): NoticeSwitches {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<NoticeSwitches>;
    return { ...DEFAULT_SWITCHES, ...saved };
  } catch {
    return DEFAULT_SWITCHES;
  }
}

export function saveSwitches(next: NoticeSwitches) {
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // No storage: the choice lasts until the app closes.
  }
}

/** Whether a notice is for me (`to` empty = everyone) and its switch is on. `me` is "@handle" (or "" unknown). */
export function wantsNotice(notice: TeamNotice, me: string, switches: NoticeSwitches): boolean {
  if (notice.to && notice.to !== me) return false;
  switch (notice.kind) {
    case "assigned":
      return switches.assigned;
    case "savedOver":
      return switches.savedOver;
    case "assetAdded":
      return switches.assetAdded;
    case "proposalNew":
    case "proposalDone":
      return switches.proposals;
    default:
      return false;
  }
}
