import { tr } from "./i18n";

export const CATEGORIES = ["General", "Gameplay", "Worlds", "Players & security", "Network", "Performance", "Advanced"] as const;
export type Category = (typeof CATEGORIES)[number];

export type PropertyMeta = {
  key: string;
  label: string;
  description: string;
  category: Category;
  known: boolean;
};

type Row = readonly [key: string, category: Category, label: string, description: string];

const ROWS: readonly Row[] = [
  ["motd", "General", "Server description (MOTD)", "The line shown below the server name in the server list. Supports color codes (§)."],
  ["server-port", "General", "Port", "The port friends use to connect. Changing it also changes the connection address."],
  ["max-players", "General", "Max players", "The maximum number of players who can be online at once."],
  ["level-name", "General", "World folder name", "The world folder name inside the server folder. Changing it creates a new world."],
  ["enable-status", "General", "Show in server list", "When off, the server list shows no player count or description."],
  ["bug-report-link", "General", "Bug report URL", "The URL opened by the in-game pause menu's 'Report bug' button."],
  ["gamemode", "Gameplay", "Game mode", "The game mode for first-time players."],
  ["force-gamemode", "Gameplay", "Force game mode", "When on, players return to the default game mode each time they join."],
  ["difficulty", "Gameplay", "Difficulty", "Controls mob damage, hunger and similar settings."],
  ["hardcore", "Gameplay", "Hardcore", "On death, you become a spectator and can't respawn."],
  ["pvp", "Gameplay", "PvP", "When off, players can't attack each other."],
  ["allow-flight", "Gameplay", "Allow flight", "Allows flying in survival without being kicked, including modded flight that might be mistaken for cheating."],
  ["spawn-monsters", "Gameplay", "Mob spawning", "When off, hostile mobs do not spawn."],
  ["enable-command-block", "Gameplay", "Enable command blocks", "Lets command blocks run."],
  ["spawn-protection", "Gameplay", "Spawn protection radius", "The radius around spawn where only operators can edit blocks. 0 disables protection."],
  ["player-idle-timeout", "Gameplay", "Idle kick", "Kicks players after this many minutes of inactivity. 0 disables idle kicks."],
  ["pause-when-empty-seconds", "Gameplay", "Stop when empty", "Pauses server ticks after this many seconds with no players online."],
  ["level-seed", "Worlds", "World seed", "Used when creating a new world. Has no effect on an existing one."],
  ["level-type", "Worlds", "Level type", "The terrain for new worlds, such as flat, large biomes or amplified."],
  ["generate-structures", "Worlds", "Generate structures", "Whether villages, strongholds and other structures generate."],
  ["generator-settings", "Worlds", "Generator settings", "Detailed terrain settings (JSON), such as flat world options."],
  ["allow-nether", "Worlds", "Allow Nether", "When off, Nether portals don't work."],
  ["max-world-size", "Worlds", "Max world size", "Radius of the world border in blocks."],
  ["initial-enabled-packs", "Worlds", "Initially enabled data packs", "Data packs enabled from the start in a new world."],
  ["initial-disabled-packs", "Worlds", "Initially disabled data packs", "Data packs disabled from the start in a new world."],
  ["region-file-compression", "Worlds", "Region file compression", "How world files are compressed."],
  ["online-mode", "Players & security", "Online mode", "When off, anyone can join with any name. Keep the whitelist on if you disable this."],
  ["white-list", "Players & security", "Whitelist", "When on, only players on the whitelist can join."],
  ["enforce-whitelist", "Players & security", "Enforce whitelist", "When on, players removed from the whitelist are kicked immediately."],
  ["op-permission-level", "Players & security", "Operator permission level", "The command permission level for operators (1–4)."],
  ["function-permission-level", "Players & security", "Function permission level", "The permission level for data pack functions (1–4)."],
  ["enforce-secure-profile", "Players & security", "Require signed chat", "When on, only signed chat is accepted. Some mods and proxies may be incompatible."],
  ["hide-online-players", "Players & security", "Hide players", "Hides online players from the server list."],
  ["prevent-proxy-connections", "Players & security", "Block proxy connections", "Blocks connections through VPNs and proxies."],
  ["log-ips", "Players & security", "Log IPs", "Whether to record IP addresses in connection logs."],
  ["accepts-transfers", "Players & security", "Receive server transfer", "Whether to accept players transferred from another server."],
  ["text-filtering-config", "Players & security", "Chat filter settings", "The configuration file for a chat filtering service. Usually left empty."],
  ["text-filtering-version", "Players & security", "Chat filter version", "The version number of the chat filter configuration."],
  ["enable-code-of-conduct", "Players & security", "Show code of conduct", "When on, first-time players must accept the code of conduct."],
  ["broadcast-console-to-ops", "Players & security", "Send console output to ops", "Also shows console command output to operators."],
  ["broadcast-rcon-to-ops", "Players & security", "Send RCON output to ops", "Also shows RCON command output to operators."],
  ["chat-spam-threshold-seconds", "Players & security", "Chat spam window (s)", "Talking too much within this window counts as spam."],
  ["command-spam-threshold-seconds", "Players & security", "Command spam window (s)", "Running too many commands within this window counts as spam."],
  ["server-ip", "Network", "Server IP", "The address to listen on if the computer has several. Usually left empty."],
  ["enable-rcon", "Network", "Enable RCON", "Allows console commands from outside the app. Useful for servers the app didn't start."],
  ["rcon.port", "Network", "RCON port", "Port RCON listens on."],
  ["rcon.password", "Network", "RCON password", "The RCON password. RCON stays off if this is empty."],
  ["enable-query", "Network", "Enable query", "Opens the query protocol that reports server status."],
  ["query.port", "Network", "Query port", "Port the query protocol listens on."],
  ["network-compression-threshold", "Network", "Network compression threshold", "Compresses packets larger than this size in bytes. -1 disables compression."],
  ["rate-limit", "Network", "Rate limit", "The number of packets each player can send per second. 0 means no limit."],
  ["use-native-transport", "Network", "Use native transport", "Uses the faster native networking on Linux."],
  ["resource-pack", "Network", "Resource pack URL", "The resource pack URL offered to players when they join."],
  ["resource-pack-sha1", "Network", "Resource pack hash", "The resource pack file's SHA-1 hash, used to check for changes."],
  ["resource-pack-id", "Network", "Resource pack ID", "UUID that identifies the resource pack."],
  ["resource-pack-prompt", "Network", "Resource pack prompt", "The message shown when players are asked to download the resource pack."],
  ["require-resource-pack", "Network", "Require resource pack", "Players who decline the resource pack can't join."],
  ["status-heartbeat-interval", "Network", "Status heartbeat interval", "How often (seconds) the server announces its status."],
  ["view-distance", "Performance", "View distance", "The radius of chunks sent to each player. Lower values reduce server load."],
  ["simulation-distance", "Performance", "Simulation distance", "The radius of chunks where mobs and crops tick. Keep this below the view distance."],
  ["entity-broadcast-range-percentage", "Performance", "Entity broadcast range", "How far mobs are visible, as a percentage of the default."],
  ["max-tick-time", "Performance", "Max tick time", "Shuts down the server if a tick takes longer than this many milliseconds. -1 disables this check."],
  ["max-chained-neighbor-updates", "Performance", "Max chained neighbor updates", "The maximum number of chained neighbor updates a block can trigger."],
  ["sync-chunk-writes", "Performance", "Sync chunk writes", "Whether to save chunks to disk immediately. Turning this off is faster, but a crash may cause some data loss."],
  ["debug", "Advanced", "Debug", "Records detailed logs for debugging."],
  ["enable-jmx-monitoring", "Advanced", "JMX monitoring", "Exposes server status over JMX."],
  ["management-server-enabled", "Advanced", "Enable management server", "A management server that external tools can connect to."],
  ["management-server-host", "Advanced", "Management server address", "Address the management server listens on."],
  ["management-server-port", "Advanced", "Management server port", "Port the management server listens on."],
  ["management-server-secret", "Advanced", "Management server secret", "Key used to connect to the management server."],
  ["management-server-allowed-origins", "Advanced", "Management server allowed origins", "Origins allowed to connect to the management server."],
  ["management-server-tls-enabled", "Advanced", "Management server TLS", "Whether the management server connection is encrypted."],
  ["management-server-tls-keystore", "Advanced", "Management server keystore", "The file containing the TLS certificate."],
  ["management-server-tls-keystore-password", "Advanced", "Management server keystore password", "The keystore file's password."],
  ["seed", "Worlds", "Seed", "Used when creating a new world."],
  ["default_level_name", "Worlds", "World folder name", "Name of the world folder inside the server folder."],
  ["world.autosave_ticks", "Worlds", "Autosave interval (ticks)", "The interval between world saves, in ticks."],
  ["default_difficulty", "Gameplay", "Difficulty", "Controls mob damage, hunger and similar settings."],
  ["default_gamemode", "Gameplay", "Game mode", "The game mode for first-time players."],
  ["force_gamemode", "Gameplay", "Force game mode", "When on, players return to the default game mode each time they join."],
  ["allow_nether", "Gameplay", "Allow Nether", "When off, Nether portals don't work."],
  ["allow_end", "Gameplay", "Allow the End", "When off, End portals don't work."],
  ["pvp.enabled", "Gameplay", "PvP", "When off, players can't attack each other."],
  ["tps", "Performance", "Target TPS", "The target number of ticks per second. Usually 20."],
  ["white_list", "Players & security", "Whitelist", "When on, only players on the whitelist can join."],
  ["enforce_whitelist", "Players & security", "Enforce whitelist", "When on, players removed from the whitelist are kicked immediately."],
  ["op_permission_level", "Players & security", "OP permission level", "The command permission level for operators."],
  ["scrub_ips", "Players & security", "Hide IPs in logs", "Masks IP addresses in logs."],
  ["allow_chat_reports", "Players & security", "Allow chat reports", "Allows players to report chat messages."],
  ["use_favicon", "General", "Use server icon", "Whether to show an icon in the server list."],
  ["favicon_path", "General", "Server icon file", "The path to the server icon image."],
  ["networking.java.address", "Network", "Java address", "Address and port Java Edition players connect to."],
  ["networking.java.online_mode", "Players & security", "Online mode", "When off, anyone can join under any name."],
  ["networking.java.max_players", "General", "Max players", "The maximum number of players who can be online at once."],
  ["networking.java.view_distance", "Performance", "View distance", "Chunk radius sent to each player."],
  ["networking.java.simulation_distance", "Performance", "Simulation distance", "The radius of chunks where mobs and crops tick."],
  ["networking.java.motd", "General", "Server description (MOTD)", "The line shown under the name in the server list."],
  ["networking.java.encryption", "Network", "Encryption", "Whether connections are encrypted."],
  ["networking.bedrock.enabled", "Network", "Bedrock connections", "Lets Bedrock Edition players join too."],
  ["networking.bedrock.max_players", "Network", "Bedrock max players", "How many Bedrock players can be online."],
  ["networking.bedrock.motd", "Network", "Bedrock MOTD", "The line shown in the Bedrock server list."],
  ["networking.rcon.enabled", "Network", "RCON", "Lets commands be sent from outside."],
  ["networking.rcon.address", "Network", "RCON address", "Address and port RCON listens on."],
  ["networking.rcon.password", "Network", "RCON password", "Password for RCON."],
  ["networking.query.enabled", "Network", "Query", "Opens the query protocol that reports server status."],
  ["networking.lan_broadcast.enabled", "Network", "LAN broadcast", "Announces the server on the local network."],
  ["chat.format", "Gameplay", "Chat format", "The format of chat messages."],
  ["plugins.enabled", "Advanced", "Enable plugins", "Whether plugins are loaded."],
  ["plugins.hot_reload", "Advanced", "Hot-reload plugins", "Reloads plugins when their files change."],
  ["logging.level", "Advanced", "Log level", "How detailed the log is."],
];

const byKey = new Map<string, Row>(ROWS.map((row) => [row[0], row]));

export const KNOWN_KEYS: readonly string[] = ROWS.map((row) => row[0]);

export function propertyMeta(key: string): PropertyMeta {
  const row = byKey.get(key);
  if (!row) return { key, label: key, description: "", category: "Advanced", known: false };
  return { key, label: tr(row[2]), description: tr(row[3]), category: row[1], known: true };
}

export function propertyMatches(meta: PropertyMeta, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return (
    meta.key.toLowerCase().includes(needle) ||
    meta.label.toLowerCase().includes(needle) ||
    meta.description.toLowerCase().includes(needle)
  );
}

export function propertyOrder(key: string): number {
  const at = KNOWN_KEYS.indexOf(key);
  return at < 0 ? Number.MAX_SAFE_INTEGER : at;
}
