import { addonVersion, deliver, gradle } from "./addon-gradle.mjs";

gradle("paper");
deliver("paper", `Monolab-${addonVersion()}.jar`);
