import { beforeAll, describe, expect, test } from "vitest";
import { ko } from "../test/lang";
import { setLanguage } from "./i18n";
import { CATEGORIES, KNOWN_KEYS, propertyMatches, propertyMeta } from "./properties";

describe("table", () => {
  test("has unique keys and valid sections", () => {
    expect(new Set(KNOWN_KEYS).size).toBe(KNOWN_KEYS.length);
    for (const key of KNOWN_KEYS) {
      const meta = propertyMeta(key);
      expect(CATEGORIES).toContain(meta.category);
      expect(meta.known).toBe(true);
      expect(meta.label.length, key).toBeGreaterThan(0);
      expect(meta.description.length, key).toBeGreaterThan(0);
    }
  });

  test("knows every key in a real Paper 26.2 server.properties", () => {
    const real = "accepts-transfers allow-flight broadcast-console-to-ops broadcast-rcon-to-ops bug-report-link chat-spam-threshold-seconds command-spam-threshold-seconds debug difficulty enable-code-of-conduct enable-jmx-monitoring enable-query enable-rcon enable-status enforce-secure-profile enforce-whitelist entity-broadcast-range-percentage force-gamemode function-permission-level gamemode generate-structures generator-settings hardcore hide-online-players initial-disabled-packs initial-enabled-packs level-name level-seed level-type log-ips management-server-allowed-origins management-server-enabled management-server-host management-server-port management-server-secret management-server-tls-enabled management-server-tls-keystore management-server-tls-keystore-password max-chained-neighbor-updates max-players max-tick-time max-world-size motd network-compression-threshold online-mode op-permission-level pause-when-empty-seconds player-idle-timeout prevent-proxy-connections query.port rcon.password rcon.port region-file-compression require-resource-pack resource-pack resource-pack-id resource-pack-prompt resource-pack-sha1 server-ip server-port simulation-distance spawn-protection status-heartbeat-interval sync-chunk-writes text-filtering-config text-filtering-version use-native-transport view-distance white-list".split(" ");
    const unknown = real.filter((key) => !propertyMeta(key).known);
    expect(unknown).toEqual([]);
  });

  test("unknown keys go to advanced with their own name", () => {
    const meta = propertyMeta("some-mod-option");
    expect(meta).toEqual({ key: "some-mod-option", label: "some-mod-option", description: "", category: "Advanced", known: false });
  });

  test("every label and description has a Korean entry", async () => {
    await setLanguage("en");
    const missing: string[] = [];
    for (const key of KNOWN_KEYS) {
      const meta = propertyMeta(key);
      for (const text of [meta.label, meta.description]) if (!(text in ko)) missing.push(text);
    }
    expect(missing).toEqual([]);
  });
});

describe("search", () => {
  beforeAll(() => setLanguage("en"));
  test("searches labels, raw names and descriptions", () => {
    const meta = propertyMeta("view-distance");
    expect(propertyMatches(meta, "")).toBe(true);
    expect(propertyMatches(meta, meta.label.slice(0, 4))).toBe(true);
    expect(propertyMatches(meta, "VIEW-dist")).toBe(true);
    expect(propertyMatches(meta, meta.description.split(" ").slice(-2).join(" "))).toBe(true);
    expect(propertyMatches(meta, "no-such-word")).toBe(false);
  });
});
