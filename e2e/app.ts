import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const PORT = 25566;
export const BASE = `http://127.0.0.1:${PORT}`;
export const ACCOUNT = { name: "e2e", password: "e2e-password-1234" };
const HASH =
  "$argon2id$v=19$m=19456,t=2,p=1$jAY3KZxbc2chhi0relMV9A$SfZqMCtHgTHjHN2XCCo/PZklRp/kuDTfRWaq054M9es";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// The app's data folder (src-tauri/src/data_root.rs): ~/.monolab since 2026-11 (it moves the old %APPDATA% one there).
export const dataDir = join(process.env.USERPROFILE ?? process.env.HOME ?? "", ".monolab");
const settingsPath = join(dataDir, "settings.json");
const backupPath = join(dataDir, "settings.json.e2e-backup");
const pidPath = join(root, "e2e", ".app.pid");
export const exe = join(root, "src-tauri", "target", "debug", "monolab.exe");

export function appRunning(): boolean {
  const out = execFileSync("tasklist", ["/FI", "IMAGENAME eq monolab.exe", "/NH"], {
    encoding: "utf8",
  });
  return /monolab\.exe/i.test(out);
}

export function installTestSettings() {
  if (!existsSync(dataDir)) {
    // A fresh machine (CI) has never run the app; anywhere else a missing folder means the wrong place.
    if (!process.env.CI) throw new Error(`No app data folder: ${dataDir}`);
    mkdirSync(dataDir, { recursive: true });
  }
  if (existsSync(backupPath)) restoreSettings();
  if (existsSync(settingsPath)) renameSync(settingsPath, backupPath);
  const previous = existsSync(backupPath)
    ? (JSON.parse(readFileSync(backupPath, "utf8")) as Record<string, unknown>)
    : {};
  const settings = {
    ...previous,
    webPanel: true,
    webPanelPort: PORT,
    webAccounts: [{ name: ACCOUNT.name, hash: HASH, role: "Admin", servers: [] }],
  };
  writeFileSync(settingsPath, JSON.stringify(settings, null, 2), "utf8");
}

export function restoreSettings() {
  if (existsSync(backupPath)) {
    rmSync(settingsPath, { force: true });
    renameSync(backupPath, settingsPath);
  } else {
    rmSync(settingsPath, { force: true });
  }
}

export async function startApp() {
  if (!existsSync(exe)) {
    throw new Error(`${exe} not found. Run \`cd src-tauri && cargo build\` once first.`);
  }
  const log = openSync(join(root, "e2e", ".app.log"), "w");
  const child = spawn(exe, [], { detached: true, stdio: ["ignore", log, log], windowsHide: true });
  child.unref();
  writeFileSync(pidPath, String(child.pid), "utf8");
  const started = Date.now();
  const deadline = started + 120_000;
  while (Date.now() < deadline) {
    try {
      const answer = await fetch(`${BASE}/`);
      if (answer.ok) return;
    } catch {
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  stopApp();
  restoreSettings();
  throw new Error("The web panel didn't answer within 120 seconds.");
}

export function stopApp() {
  if (!existsSync(pidPath)) return;
  const pid = readFileSync(pidPath, "utf8").trim();
  rmSync(pidPath, { force: true });
  if (!pid) return;
  try {
    execFileSync("taskkill", ["/PID", pid, "/T", "/F"], { stdio: "ignore" });
  } catch {
  }
  const deadline = Date.now() + 10_000;
  while (appRunning() && Date.now() < deadline) {
    execFileSync("powershell", ["-NoProfile", "-Command", "Start-Sleep -Milliseconds 300"], { stdio: "ignore" });
  }
}

/** Signs in to the web panel and returns a caller for its API. */
export async function webApi() {
  const login = await fetch(`${BASE}/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: ACCOUNT.name, password: ACCOUNT.password }),
  });
  const { session } = (await login.json()) as { session?: string };
  if (!session) throw new Error("The test account couldn't sign in to the web panel.");
  const headers = { "Content-Type": "application/json", "X-SE-Session": session };
  return async (command: string, body: unknown = {}) => {
    const answer = (await (
      await fetch(`${BASE}/api/${command}?key=`, { method: "POST", headers, body: JSON.stringify(body) })
    ).json()) as { value?: unknown; error?: string };
    if (answer.error) throw new Error(`${command}: ${answer.error}`);
    return answer.value;
  };
}

/** On a machine with no servers (CI), makes one Paper server for the smoke test to start and stop. */
export async function ensureTestServer() {
  const call = await webApi();
  const list = ((await call("list_instances")) ?? []) as unknown[];
  if (list.length > 0) return;
  if (!process.env.CI && !process.env.SE_E2E_CREATE) {
    throw new Error("This app has no servers. Make one first, or set SE_E2E_CREATE=1 to let the tests make one.");
  }
  await call("create_instance", {
    input: { name: "e2e", core: "Paper", version: "1.21.4", maxMemoryMib: 1024, minMemoryMib: 1024, eulaAccepted: true },
  });
}
