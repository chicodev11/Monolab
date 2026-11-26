import { commands } from "./ipc";

/** A server on a teammate's PC that this user's agent can work on, through Monolink. */
export type SharedServer = { ref: string; name: string; host: string; team: string; online: boolean };

type Summary = { id: string; name: string };
type TeamServer = { name: string; instance_id: string; host_device: string; host: string | null; host_online: boolean; mine: boolean };

/** The id an agent uses for a shared server; the app reads it back in `team_agent::parse`. */
export function sharedRef(team: string, hostDevice: string, instance: string) {
  return `team~${team}~${hostDevice}~${instance}`;
}

export function isSharedRef(server: string) {
  return server.startsWith("team~");
}

async function get<T>(path: string): Promise<T> {
  return JSON.parse(await commands.accountApi("GET", path, null)) as T;
}

/** Servers other team members share with this user (not this PC's own), or none when signed out. */
export async function sharedServers(): Promise<SharedServer[]> {
  const state = await commands.accountState();
  if (state.status !== "signed_in") return [];
  const me = await get<{ teams: Summary[] }>("/v2/me");
  const teams = await Promise.all(
    me.teams.map((team) => get<{ name: string; servers?: TeamServer[] }>(`/v2/teams/${team.id}`).then((full) => ({ id: team.id, full }), () => null)),
  );
  return teams.flatMap((team) =>
    team
      ? (team.full.servers ?? [])
          .filter((one) => !one.mine && one.host)
          .map((one) => ({ ref: sharedRef(team.id, one.host_device, one.instance_id), name: one.name, host: one.host ?? "", team: team.full.name, online: one.host_online }))
      : [],
  );
}
