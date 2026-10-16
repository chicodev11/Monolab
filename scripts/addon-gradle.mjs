import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const resources = join(root, "src-tauri", "resources");

export function addonVersion() {
  const text = readFileSync(join(root, "gradle.properties"), "utf8");
  const found = text.match(/^\s*seAddonVersion\s*=\s*(\S+)/m);
  if (!found) {
    throw new Error("seAddonVersion is missing from gradle.properties.");
  }
  return found[1];
}

const gradlew = join(root, process.platform === "win32" ? "gradlew.bat" : "gradlew");

export function gradle(project, properties = {}) {
  const args = [
    `:${project}:clean`,
    `:${project}:build`,
    ...Object.entries(properties).map(([key, value]) => `-P${key}=${value}`),
    "--console=plain",
  ];
  const done = spawnSync(gradlew, args, { cwd: root, stdio: "inherit", shell: true });
  if (done.status !== 0) {
    throw new Error(`Gradle exited with ${done.status} (:${project}).`);
  }
}

export function deliver(project, name) {
  const built = join(root, "addon", project, "build", "libs", name);
  if (!existsSync(built)) {
    throw new Error(`${built} is missing. Gradle may have changed how it names jars.`);
  }
  mkdirSync(resources, { recursive: true });
  copyFileSync(built, join(resources, name));
  console.log(`Done: ${name} (${(statSync(built).size / 1024).toFixed(1)} KB)`);
}
