import { useCallback, useEffect, useState } from "react";
import type { RoleView } from "./bindings";
import { tr } from "./i18n";
import { commands } from "./ipc";

/** The permissions a host can tick for a role, in the order the editor shows them (same as `PERMS` in `web/auth.rs`). */
export const PERMS = [
  "console.view",
  "console.send",
  "power",
  "files.read",
  "files.write",
  "secrets",
  "plugins",
  "backups",
  "studio.read",
  "studio.write",
  "players",
  "settings",
] as const;

export function permLabel(perm: string): string {
  switch (perm) {
    case "console.view":
      return tr("View the console and logs");
    case "console.send":
      return tr("Send console commands");
    case "power":
      return tr("Start, stop and restart");
    case "files.read":
      return tr("Open files");
    case "files.write":
      return tr("Edit, upload and delete files");
    case "secrets":
      return tr("See passwords and download whole files");
    case "plugins":
      return tr("Plugins, mods and datapacks");
    case "backups":
      return tr("Make and restore backups");
    case "studio.read":
      return tr("Open the studio");
    case "studio.write":
      return tr("Save studio changes");
    case "players":
      return tr("Manage players");
    case "settings":
      return tr("Server settings and tasks");
    default:
      return perm;
  }
}

/** Built-in roles, for when the list can't be read (a web panel guest). */
const BUILT_IN: RoleView[] = [
  { id: "Admin", name: "Admin", builtin: true, perms: [...PERMS], people: 0 },
  { id: "Manager", name: "Manager", builtin: true, perms: [...PERMS], people: 0 },
  { id: "Viewer", name: "Viewer", builtin: true, perms: ["console.view", "files.read", "studio.read"], people: 0 },
];

/** A role's name to show: built-in names translated, a custom role by its name, a deleted one as such. */
export function roleName(id: string, roles: RoleView[]): string {
  if (id === "Admin") return tr("Admin");
  if (id === "Manager") return tr("Manager");
  if (id === "Viewer") return tr("Viewer");
  const found = roles.find((one) => one.id === id);
  if (found) return found.name;
  return id.startsWith("role:") ? tr("Deleted role") : id;
}

/** What a role may do, in a few words, for hints under pickers. */
export function roleHint(role: RoleView | undefined): string {
  if (!role) return "";
  if (role.id === "Admin") return tr("Every server, and the app's own settings");
  if (role.id === "Manager") return tr("Can start and stop it, use the console, and edit files and settings");
  if (role.id === "Viewer") return tr("Can watch the console, logs, files and players, but change nothing");
  if (!role.perms.length) return tr("Can open the server's overview, nothing more");
  return role.perms.map(permLabel).join(" · ");
}

const CHANGED = "monolab-roles-changed";

/** Tells every open role list to read the roles again. */
export function rolesChanged() {
  window.dispatchEvent(new Event(CHANGED));
}

/** Every role (built-in first, then the host's own), kept fresh when the roles editor saves. */
export function useRoles(): { roles: RoleView[]; reload: () => Promise<void> } {
  const [roles, setRoles] = useState<RoleView[]>(BUILT_IN);
  const reload = useCallback(async () => {
    try {
      setRoles(await commands.roleCatalog());
    } catch {
      setRoles(BUILT_IN);
    }
  }, []);
  useEffect(() => {
    void reload();
    const again = () => void reload();
    window.addEventListener(CHANGED, again);
    return () => window.removeEventListener(CHANGED, again);
  }, [reload]);
  return { roles, reload };
}

/** Roles that can be given on chosen servers: Manager, Viewer and the host's own (never Admin). */
export function serverRoles(roles: RoleView[]): RoleView[] {
  return roles.filter((one) => one.id !== "Admin");
}
