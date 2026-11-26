import { tr } from "./i18n";

/** The Settings row a portable setting belongs to (keys from PORTABLE_SETTINGS in src-tauri/src/lib.rs). */
export function settingLabel(key: string): string {
  const labels: Record<string, string> = {
    autoBackupOnSave: tr("Back up before saving files"),
    openConsoleOnStart: tr("Open the console on start"),
    defaultMemoryMib: tr("Default memory for new servers"),
    keepBackupDays: tr("Keep backups for"),
    keepAutoBackups: tr("Scheduled backups to keep"),
    consoleLineLimit: tr("Console history size (lines)"),
    backupRoot: tr("Backup location"),
    backupWorldsOnly: tr("Scheduled backups: worlds only"),
    backupIncludeGit: tr("Include git history in backups"),
    closeToTray: tr("Hide to the tray"),
    discordPresence: tr("Discord status"),
    updateChannel: tr("Update channel"),
  };
  return labels[key] ?? key;
}

/** A setting's JSON value as a person reads it. */
export function settingValue(json: string): string {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    return json;
  }
  if (value === true) return tr("on");
  if (value === false) return tr("off");
  if (value === "" || value === null) return tr("default");
  return typeof value === "string" ? value : JSON.stringify(value);
}
