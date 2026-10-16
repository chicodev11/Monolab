/* eslint-disable @typescript-eslint/no-explicit-any */
import { commands } from "./ipc";

export type Json = Record<string, any>;

export async function pcall<T = Json>(hostId: string, method: string, path: string, body?: unknown): Promise<T> {
  const text = await commands.panelCall(hostId, method, path, body === undefined ? null : JSON.stringify(body));
  return (text && text !== "null" ? JSON.parse(text) : null) as T;
}

export async function plist(hostId: string, path: string): Promise<Json[]> {
  const out: Json[] = [];
  const join = path.includes("?") ? "&" : "?";
  for (let page = 1; page <= 50; page++) {
    const got = await pcall<Json>(hostId, "GET", `${path}${join}per_page=100&page=${page}`);
    const rows: Json[] = got?.data ?? [];
    out.push(...rows.map((row) => row.attributes ?? row));
    const pages = got?.meta?.pagination?.total_pages ?? 1;
    if (page >= pages || !rows.length) break;
  }
  return out;
}

export function serverBase(daemon: string, ident: string) {
  return `/api/client/servers/${daemon || "elytra"}/${ident}`;
}

export function isoSecs(value: unknown): number {
  const at = typeof value === "string" ? Date.parse(value) : NaN;
  return Number.isFinite(at) ? Math.floor(at / 1000) : 0;
}
