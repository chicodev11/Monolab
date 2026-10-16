import { ACCOUNT, BASE, restoreSettings, stopApp } from "./app";

async function stopRunningServers() {
  try {
    const login = await fetch(`${BASE}/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: ACCOUNT.name, password: ACCOUNT.password }),
    });
    const { session } = (await login.json()) as { session?: string };
    if (!session) return;
    const headers = { "Content-Type": "application/json", "X-SE-Session": session };
    const call = async (command: string, body: unknown = {}) =>
      (await (
        await fetch(`${BASE}/api/${command}?key=`, { method: "POST", headers, body: JSON.stringify(body) })
      ).json()) as { value?: unknown };
    const list = ((await call("list_instances")).value ?? []) as { id: string; status: string }[];
    for (const one of list.filter((each) => each.status === "running" || each.status === "stopping")) {
      await call("stop_instance", { id: one.id });
    }
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      const now = ((await call("list_instances")).value ?? []) as { status: string }[];
      if (now.every((each) => each.status === "stopped" || each.status === "crashed")) return;
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
  } catch {
  }
}

export default async function globalTeardown() {
  await stopRunningServers();
  stopApp();
  restoreSettings();
}
