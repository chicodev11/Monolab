import { readFileSync } from "node:fs";
import { join } from "node:path";
import { addonVersion, deliver, gradle, root } from "./addon-gradle.mjs";

const { loader, targets } = JSON.parse(
  readFileSync(join(root, "addon", "fabric", "targets.json"), "utf8"),
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

function mappings(target) {
  return target.yarn
    ? `net.fabricmc:yarn:${target.yarn}:v2`
    : "net.fabricmc:intermediary:0.0.0:v2";
}

for (const target of making) {
  console.log(`\n=== Minecraft ${target.id} ===`);
  gradle("fabric", {
    seMc: target.id,
    seBridge: target.bridge,
    seMapped: target.yarn ? "yarn" : "mojang",
    seMappings: mappings(target),
    seLoader: loader,
    seFabricApi: target.api,
  });
  deliver("fabric", `Monolab-fabric-${target.id}-${addonVersion()}.jar`);
}
