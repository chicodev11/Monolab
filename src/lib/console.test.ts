import { beforeAll, describe, expect, test } from "vitest";
import { setLanguage } from "./i18n";
import {
  readPlayerCommand,
  tidyLogLine,
  withoutLogStamp,
  ghostSuffix,
  usageSlot,
  wantsPlayerName,
  groupConsole,
  isErrorTail,
  pumpkinLine,
  readChat,
  readLevel,
  readNotice,
  splitConsole,
} from "./console";

describe("readLevel reads the level from the prefix only", () => {
  test.each([
    ['[13:45:22 INFO]: Done (12.345s)! For help, type "help"', null],
    ["[13:45:22 ERROR]: Command exception: /stop", "error"],
    ["[13:45:22 WARN]: Failed to get system info for Process Loads", "warn"],
    ["[01:05:31] [Server thread/INFO]: Steve joined the game", null],
    ["[01:05:31] [Server thread/WARN]: something", "warn"],
    ["[01:05:31] [Server thread/ERROR]: bad", "error"],
    ["\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:1)", null],
    ["Caused by: java.lang.NullPointerException: x", null],
    ["[13:45:22 INFO]: error in chat text", null],
    ["19:33:00  WARN pumpkin::net: something odd", "warn"],
    ["19:33:00  INFO Started server; took 57ms", null],
    ["[00:42:56] [main/WARN]: Pumpkin is currently under heavy development!", "warn"],
    ["[00:42:56] [tokio-rt-worker/ERROR]: bad", "error"],
    ["[main/WARN]: no timestamp", "warn"],
    ["plain line without prefix", null],
  ])("%s → %s", (line, level) => {
    expect(readLevel(line)).toBe(level);
  });
});

describe("pumpkinLine handles both Pumpkin prefixes", () => {
  test("current format with time and level", () => {
    expect(pumpkinLine("19:33:00  INFO Started server; took 57ms")).toEqual({
      time: "19:33:00",
      level: "INFO",
      body: "Started server; took 57ms",
    });
    expect(pumpkinLine("19:33:00  WARN pumpkin::net: something odd")).toEqual({
      time: "19:33:00",
      level: "WARN",
      body: "something odd",
    });
  });
  test("old format with date, thread and module", () => {
    expect(
      pumpkinLine(
        "2026-08-26 15:36:37  INFO main ThreadId(01) pumpkin::server: All worlds loaded",
      ),
    ).toEqual({
      time: "15:36:37",
      level: "INFO",
      body: ":server: All worlds loaded",
    });
  });
  test("is not a Minecraft line", () => {
    expect(pumpkinLine("[13:45:22 INFO]: Done (1s)!")).toBeNull();
    expect(pumpkinLine("plain")).toBeNull();
  });
});

describe("splitConsole splits time, level and text", () => {
  test.each([
    [
      '[13:45:22 INFO]: Done (12.345s)! For help, type "help"',
      { time: "13:45:22", level: "INFO", body: 'Done (12.345s)! For help, type "help"' },
    ],
    ["[01:05:31] [Server thread/ERROR]: bad", { time: "01:05:31", level: "ERROR", body: "bad" }],
    [
      "[00:42:56] [main/INFO]: Starting Pumpkin 0.2.2+26.3-26.63",
      { time: "00:42:56", level: "INFO", body: "Starting Pumpkin 0.2.2+26.3-26.63" },
    ],
    [
      "[00:42:56] [tokio-rt-worker/DEBUG]: [MyPlugin] hi",
      { time: "00:42:56", level: "DEBUG", body: "[MyPlugin] hi" },
    ],
    [
      "19:33:00  INFO Started server; took 57ms",
      { time: "19:33:00", level: "INFO", body: "Started server; took 57ms" },
    ],
    [
      "\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:1)",
      { body: "\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:1)" },
    ],
    ["plain line without prefix", { body: "plain line without prefix" }],
  ])("%s", (line, expected) => {
    expect(splitConsole(line)).toEqual(expected);
  });
});

describe("readChat only treats `<name> text` as chat", () => {
  test("chat", () => {
    expect(readChat("[13:45:22 INFO]: <Steve> hello there")).toEqual({
      player: "Steve",
      text: "hello there",
    });
  });
  test.each([
    "[13:45:22 INFO]: <Steve>",
    "[13:45:22 INFO]: <two words> hi",
    "[13:45:22 INFO]: Steve joined the game",
    "plain line without prefix",
  ])("not chat: %s", (line) => {
    expect(readChat(line)).toBeNull();
  });
});

describe("isErrorTail only matches Java stack lines", () => {
  test.each([
    ["\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:1)", true],
    ["Caused by: java.lang.NullPointerException: x", true],
    ["java.lang.NullPointerException: Cannot invoke", true],
    ["... 12 more", true],
    ["\tSome mod list line", false],
    ["[13:45:22 ERROR]: Command exception: /stop", false],
    ["plain line without prefix", false],
  ])("%s → %s", (line, tail) => {
    expect(isErrorTail(line)).toBe(tail);
  });
});

describe("readNotice in English", () => {
  beforeAll(() => setLanguage("en"));
  test("only reports start, join and leave", () => {
    expect(readNotice("Done (12.345s)! For help")).toEqual({ say: "Server started!", tone: "good" });
    expect(readNotice("Steve joined the game")).toEqual({ say: "Steve joined", tone: "good" });
    expect(readNotice("Alex left the game")).toEqual({ say: "Alex left", tone: "info" });
    expect(readNotice("hello")).toBeNull();
  });
});

describe("groupConsole groups error lines", () => {
  test("groups errors with stacks and hides noise", () => {
    const rows = [
      { line: "[13:45:22 INFO]: ok" },
      { line: "[13:45:22 ERROR]: boom" },
      { line: "\tat a.b.C(C.java:1)" },
      { line: "Caused by: x.Y: z" },
      { line: "[13:45:22 INFO]: after" },
      { line: "[13:45:22 ERROR]: noisy", noise: true },
      { line: "\tat noise.tail(N.java:2)" },
      { line: "[13:45:22 INFO]: last" },
    ];
    const out = groupConsole(rows).map((chunk) =>
      chunk.kind === "error"
        ? { kind: "error", lines: chunk.rows.map((row) => row.line), key: chunk.key }
        : { kind: "line", line: chunk.row.line, key: chunk.key },
    );
    expect(out).toEqual([
      { kind: "line", line: "[13:45:22 INFO]: ok", key: "0-[13:45:22 INFO]: ok" },
      {
        kind: "error",
        lines: ["[13:45:22 ERROR]: boom", "\tat a.b.C(C.java:1)", "Caused by: x.Y: z"],
        key: "3-err-3",
      },
      { kind: "line", line: "[13:45:22 INFO]: after", key: "4-[13:45:22 INFO]: after" },
      { kind: "line", line: "[13:45:22 INFO]: last", key: "7-[13:45:22 INFO]: last" },
    ]);
  });
  test("empty list", () => {
    expect(groupConsole([])).toEqual([]);
  });
});

describe("withoutLogStamp only drops a standalone time bracket", () => {
  test("NeoForge prefixes, with and without a date", () => {
    expect(withoutLogStamp("[10Oct2026 20:08:01.249] [Server thread/INFO] [x/]: hi")).toBe("[Server thread/INFO] [x/]: hi");
    expect(withoutLogStamp("[21:29:20.649] [Server thread/INFO] [x/]: hi")).toBe("[Server thread/INFO] [x/]: hi");
  });
  test("keeps a leading ANSI color code", () => {
    expect(withoutLogStamp("[32m[1Oct2026 09:00:00.001] ok")).toBe("[32mok");
  });
  test("old logs from a Korean locale", () => {
    expect(withoutLogStamp("[209\uC6D42026 12:34:56.789] [main/INFO]: a")).toBe("[main/INFO]: a");
  });
  test("keeps Paper and Pumpkin time + level", () => {
    expect(withoutLogStamp("[20:08:01 INFO]: Done")).toBe("[20:08:01 INFO]: Done");
    expect(withoutLogStamp("19:33:00  INFO pumpkin: Started")).toBe("19:33:00  INFO pumpkin: Started");
  });
  test("keeps times and dates inside the text", () => {
    expect(withoutLogStamp("<Steve> [10Oct2026 20:08] hi")).toBe("<Steve> [10Oct2026 20:08] hi");
    expect(withoutLogStamp("at 12:00:00 we meet")).toBe("at 12:00:00 we meet");
  });
});

describe("readPlayerCommand finds in-game commands", () => {
  test("Paper lines", () => {
    expect(readPlayerCommand("[21:29:20 INFO]: Steve issued server command: /tp 0 64 0")).toEqual({ player: "Steve", command: "/tp 0 64 0" });
  });
  test("chat and other lines are not commands", () => {
    expect(readPlayerCommand("[21:29:20 INFO]: <Steve> issued server command: /op me")).toBeNull();
    expect(readPlayerCommand("[21:29:20 INFO]: Done (3.2s)!")).toBeNull();
  });
});

describe("tidyLogLine reshapes NeoForge lines like Paper", () => {
  test("vanilla lines only get a level", () => {
    expect(tidyLogLine("[10Oct2026 21:29:20.649] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: Done (3.2s)!")).toBe("[21:29:20 INFO]: Done (3.2s)!");
    expect(tidyLogLine("[21:29:20.649] [Server thread/WARN] [minecraft/MinecraftServer]: Can't keep up!")).toBe("[21:29:20 WARN]: Can't keep up!");
  });
  test("mod lines keep the name", () => {
    expect(tidyLogLine("[21:17:44.432] [Server thread/INFO] [dev.ryanhcode.sable.Sable/]: Saving")).toBe("[21:17:44 INFO]: [Sable] Saving");
    expect(tidyLogLine("[21:29:36.469] [Chunksmith-minecraft:overworld Thread/INFO] [Chunksmith/]: rate")).toBe("[21:29:36 INFO]: [Chunksmith] rate");
  });
  test("lines without a logger", () => {
    expect(tidyLogLine("[21:29:20.649] [main/INFO]: Loading")).toBe("[21:29:20 INFO]: Loading");
  });
  test("leaves Paper and stack lines alone", () => {
    expect(tidyLogLine("[21:29:20 INFO]: Done")).toBe("[21:29:20 INFO]: Done");
    expect(tidyLogLine("\tat net.minecraft.Foo.bar(Foo.java:1)")).toBe("\tat net.minecraft.Foo.bar(Foo.java:1)");
  });
});

describe("ghostSuffix shows only what the completion adds", () => {
  test.each([
    ["gam", "gamemode", "emode"],
    ["GaM", "gamemode", "emode"],
    ["gamemode ", "gamemode creative", "creative"],
    ["gamemode c", "gamemode creative", "reative"],
    ["gamemode", "gamemode", ""],
    ["tp", "gamemode", ""],
    ["", "gamemode", ""],
    ["gam", undefined, ""],
    ["gamemode creative", "gamemode", ""],
  ])("%j + %j -> %j", (typed, hint, rest) => {
    expect(ghostSuffix(typed, hint)).toBe(rest);
  });
});

describe("usageSlot reads what goes at an argument", () => {
  test.each([
    ["/kick <targets> [<reason>]", 0, [], true],
    ["/kick <targets> [<reason>]", 1, [], false],
    ["/difficulty (peaceful|easy|normal|hard)", 0, ["peaceful", "easy", "normal", "hard"], false],
    ["/tp (<targets>|<location>)", 0, [], true],
    ["/gamemode (survival|creative) [<target>]", 1, [], true],
    ["/whitelist (add <targets>|remove <targets>|list)", 0, ["add", "remove", "list"], false],
    ["/say <message>", 0, [], false],
    ["/say <message>", 3, [], false],
  ] as const)("%s at %i", (usage, index, words, players) => {
    expect(usageSlot(usage, index)).toEqual({ words: [...words], players });
  });

  test("no usage offers nothing", () => {
    expect(usageSlot(null, 0)).toEqual({ words: [], players: false });
  });

  test("player argument names", () => {
    expect(wantsPlayerName("targets")).toBe(true);
    expect(wantsPlayerName("Player")).toBe(true);
    expect(wantsPlayerName("player_name")).toBe(true);
    expect(wantsPlayerName("message")).toBe(false);
    expect(wantsPlayerName(null)).toBe(false);
  });
});
