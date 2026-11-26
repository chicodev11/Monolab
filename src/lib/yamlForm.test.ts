import { describe, expect, it } from "vitest";
import { readYamlFields, setYamlField } from "./yamlForm";

const SAMPLE = [
  "# Shop settings",
  "enabled: true",
  "",
  "economy:",
  "  # Money a new player starts with",
  "  start: 100 # coins",
  "  symbol: '$'",
  "  name: Gold Coins",
  "worlds:",
  "  - world",
  "  - world_nether",
  "banned: [tnt, bedrock]",
  "motd: |",
  "  hello",
].join("\r\n");

describe("plugin config fields", () => {
  it("reads values, kinds, sections and comment hints", () => {
    const fields = readYamlFields(SAMPLE);
    const at = (path: string) => fields.find((one) => one.path === path)!;
    expect(at("enabled")).toMatchObject({ kind: "bool", value: "true", section: "", hint: "Shop settings" });
    expect(at("economy.start")).toMatchObject({ kind: "number", value: "100", section: "economy", hint: "Money a new player starts with" });
    expect(at("economy.symbol")).toMatchObject({ kind: "text", value: "$", quoted: "'" });
    expect(at("economy.name")).toMatchObject({ kind: "text", value: "Gold Coins" });
    expect(at("worlds")).toMatchObject({ kind: "list", items: 2 });
    expect(at("banned")).toMatchObject({ kind: "list", items: 2 });
    expect(at("motd").kind).toBe("block");
  });

  it("changes one value and keeps comments, quotes and line endings", () => {
    const fields = readYamlFields(SAMPLE);
    const start = fields.find((one) => one.path === "economy.start")!;
    const symbol = fields.find((one) => one.path === "economy.symbol")!;
    const name = fields.find((one) => one.path === "economy.name")!;
    let text = setYamlField(SAMPLE, start, "250");
    text = setYamlField(text, symbol, "it's");
    text = setYamlField(text, name, "Coins: gold");
    const lines = text.split("\r\n");
    expect(lines[5]).toBe("  start: 250 # coins");
    expect(lines[6]).toBe("  symbol: 'it''s'");
    expect(lines[7]).toBe("  name: 'Coins: gold'");
    expect(lines.length).toBe(SAMPLE.split("\r\n").length);
    expect(readYamlFields(text).find((one) => one.path === "economy.name")!.value).toBe("Coins: gold");
  });
});

describe("mod toml fields", () => {
  const TOML = ["# Mod settings", "[general]", "  # Seconds between checks", "  interval = 30 # s", '  name = "Big Mod"', "  enabled = true", "  tags = [\"a\", \"b\"]"].join("\n");
  it("reads sections, kinds and hints, and writes one value", async () => {
    const { readTomlFields, setTomlField } = await import("./yamlForm");
    const fields = readTomlFields(TOML);
    const interval = fields.find((one) => one.path === "general.interval")!;
    expect(interval).toMatchObject({ kind: "number", value: "30", section: "general", hint: "Seconds between checks" });
    expect(fields.find((one) => one.path === "general.name")).toMatchObject({ kind: "text", value: "Big Mod" });
    expect(fields.find((one) => one.path === "general.tags")).toMatchObject({ kind: "list", items: 2 });
    const changed = setTomlField(setTomlField(TOML, interval, "45"), fields.find((one) => one.path === "general.name")!, 'Say "hi"');
    expect(changed.split("\n")[3]).toBe("  interval = 45 # s");
    expect(changed.split("\n")[4]).toBe('  name = "Say \\"hi\\""');
  });
});
