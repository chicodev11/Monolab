/** LuckPerms console commands the Permissions tab runs (only when the user presses something). */

export const LP_NAME = /^[A-Za-z0-9_]{1,16}$/;
export const LP_GROUP = /^[A-Za-z0-9_-]{1,36}$/;
export const LP_NODE = /^[A-Za-z0-9_.*-]{1,128}$/;

export type LpTarget = { kind: "group" | "user"; name: string };

function target(one: LpTarget): string {
  const ok = one.kind === "group" ? LP_GROUP.test(one.name) : LP_NAME.test(one.name);
  if (!ok) throw new Error(`Bad ${one.kind} name: ${one.name}`);
  return `${one.kind} ${one.kind === "group" ? one.name.toLowerCase() : one.name}`;
}

function node(value: string): string {
  if (!LP_NODE.test(value)) throw new Error(`Bad permission: ${value}`);
  return value;
}

function group(value: string): string {
  if (!LP_GROUP.test(value)) throw new Error(`Bad group name: ${value}`);
  return value.toLowerCase();
}

/** `lp` (or `lpv` on Velocity) plus one change. Throws on names LuckPerms would read as more than one word. */
export const lp = {
  setPermission: (root: string, who: LpTarget, permission: string, value: boolean) => `${root} ${target(who)} permission set ${node(permission)} ${value}`,
  unsetPermission: (root: string, who: LpTarget, permission: string) => `${root} ${target(who)} permission unset ${node(permission)}`,
  addParent: (root: string, who: LpTarget, parent: string) => `${root} ${target(who)} parent add ${group(parent)}`,
  removeParent: (root: string, who: LpTarget, parent: string) => `${root} ${target(who)} parent remove ${group(parent)}`,
  createGroup: (root: string, name: string) => `${root} creategroup ${group(name)}`,
  deleteGroup: (root: string, name: string) => `${root} deletegroup ${group(name)}`,
  setWeight: (root: string, name: string, weight: number) => `${root} group ${group(name)} setweight ${Math.max(0, Math.round(weight))}`,
  setPrefix: (root: string, name: string, prefix: string, priority: number) =>
    prefix.trim() ? `${root} group ${group(name)} meta setprefix ${Math.max(0, Math.round(priority))} "${prefix.replace(/"/g, "'")}"` : `${root} group ${group(name)} meta removeprefix ${Math.max(0, Math.round(priority))}`,
};
