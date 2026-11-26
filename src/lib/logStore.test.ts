import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { LIMIT, logStore, useServerLogs, type LogLine } from "./logStore";

const line = (serverId: string, text: string): LogLine => ({
  serverId,
  ansi: "",
  line: text,
  stream: "stdout",
  noise: false,
});

beforeEach(() => logStore.reset());

describe("logStore", () => {
  test("splits a batch by server", () => {
    logStore.append([line("a", "1"), line("b", "x"), line("a", "2")]);
    expect(logStore.get("a").map((l) => l.line)).toEqual(["1", "2"]);
    expect(logStore.get("b").map((l) => l.line)).toEqual(["x"]);
    expect(logStore.get("c")).toEqual([]);
  });

  test("drops the oldest lines past the limit", () => {
    const many = Array.from({ length: LIMIT + 10 }, (_, n) => line("a", String(n)));
    logStore.append(many);
    const kept = logStore.get("a");
    expect(kept).toHaveLength(LIMIT);
    expect(kept[0].line).toBe("10");
    expect(kept[kept.length - 1].line).toBe(String(LIMIT + 9));
  });

  test("keeps the same array for unchanged servers", () => {
    logStore.append([line("a", "1")]);
    const before = logStore.get("a");
    logStore.append([line("b", "x")]);
    expect(logStore.get("a")).toBe(before);
  });

  test("replace and clear", () => {
    logStore.append([line("a", "old")]);
    logStore.replace("a", [line("a", "new")]);
    expect(logStore.get("a").map((l) => l.line)).toEqual(["new"]);
    logStore.clear("a");
    expect(logStore.get("a")).toEqual([]);
  });

  test("calls subscribers once per change and skips empty batches", () => {
    const heard = vi.fn();
    const stop = logStore.subscribe(heard);
    logStore.append([]);
    expect(heard).not.toHaveBeenCalled();
    logStore.append([line("a", "1")]);
    logStore.clear("zzz");
    expect(heard).toHaveBeenCalledTimes(1);
    stop();
    logStore.append([line("a", "2")]);
    expect(heard).toHaveBeenCalledTimes(1);
  });
});

describe("useServerLogs", () => {
  test("only rerenders for its own server", () => {
    let renders = 0;
    const { result, rerender } = renderHook(() => {
      renders += 1;
      return useServerLogs("a");
    });
    expect(result.current).toEqual([]);
    const before = renders;
    act(() => logStore.append([line("b", "x")]));
    expect(renders).toBe(before);
    act(() => logStore.append([line("a", "1")]));
    expect(result.current.map((l) => l.line)).toEqual(["1"]);
    rerender();
    expect(result.current.map((l) => l.line)).toEqual(["1"]);
  });

  test("is empty for an unknown server", () => {
    const { result } = renderHook(() => useServerLogs(null));
    expect(result.current).toEqual([]);
  });
});
