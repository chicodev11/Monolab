import { afterEach, describe, expect, it } from "vitest";
import { renderHook } from "@testing-library/react";
import { resetJobs, setServerNames, tracked, useJobs } from "./jobs";

afterEach(() => resetJobs());

describe("list of running jobs", () => {
  it("shows a job until a listed command finishes", async () => {
    setServerNames((id) => (id === "s1" ? "Server 1" : ""));
    const { result, rerender } = renderHook(() => useJobs());
    let finish: () => void = () => {};
    const running = tracked("installModrinth", ["s1", "abc", "mod", null], () => new Promise<void>((done) => (finish = done)));
    rerender();
    expect(result.current).toHaveLength(1);
    expect(result.current[0]).toMatchObject({ detail: "Server 1", watchServer: "s1" });
    finish();
    await running;
    rerender();
    expect(result.current).toHaveLength(0);
  });

  it("clears the entry even on failure", async () => {
    const { result, rerender } = renderHook(() => useJobs());
    await expect(tracked("unzipServerPath", ["s1", "a.zip"], () => Promise.reject(new Error("x")))).rejects.toThrow("x");
    rerender();
    expect(result.current).toHaveLength(0);
  });

  it("passes through commands not in the table", async () => {
    const { result, rerender } = renderHook(() => useJobs());
    await tracked("listInstances", [], () => Promise.resolve(1));
    rerender();
    expect(result.current).toHaveLength(0);
  });

  it("finds the server from an input batch id", async () => {
    setServerNames(() => "Pack server");
    const { result, rerender } = renderHook(() => useJobs());
    let finish: () => void = () => {};
    const running = tracked("importMrpack", [{ id: "s2", path: "a.mrpack", includeClientOnly: false }], () => new Promise<void>((done) => (finish = done)));
    rerender();
    expect(result.current[0]).toMatchObject({ detail: "Pack server", watchServer: "s2" });
    finish();
    await running;
  });
});

describe("the same job asked twice", () => {
  it("runs once and shows one card", async () => {
    const { result, rerender } = renderHook(() => useJobs());
    let runs = 0;
    let finish: (value: number) => void = () => {};
    const work = () => {
      runs += 1;
      return new Promise<number>((done) => (finish = done));
    };
    const first = tracked("backupContents", ["s1", "a.zip"], work);
    const second = tracked("backupContents", ["s1", "a.zip"], work);
    rerender();
    expect(runs).toBe(1);
    expect(result.current).toHaveLength(1);
    expect(result.current[0]).toMatchObject({ kind: "backupContents", server: "s1", cancellable: true });
    finish(7);
    await expect(first).resolves.toBe(7);
    await expect(second).resolves.toBe(7);
    tracked("backupContents", ["s1", "a.zip"], () => Promise.resolve(1));
    expect(runs).toBe(1);
  });
});
