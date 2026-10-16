import { readFileSync } from "node:fs";
import { join } from "node:path";
import { addonVersion, deliver, gradle, root } from "./addon-gradle.mjs";

const { targets } = JSON.parse(
  readFileSync(join(root, "addon", "neoforge", "targets.json"), "utf8"),
);

const only = process.argv.slice(2);
const making = only.length ? targets.filter((target) => only.includes(target.id)) : targets;
if (!making.length) {
  throw new Error(
    `${only.join(", ")} not in targets.json. Available: ${targets
      .map((target) => target.id)
      .join(", ")}`,
  );
}

for (const target of making) {
  console.log(`\n=== ${target.id} — Minecraft ${target.games.join(", ")} ===`);
  gradle("neoforge", { seTarget: target.id });
  deliver("neoforge", `Monolab-mod-${target.id}-${addonVersion()}.jar`);
}
