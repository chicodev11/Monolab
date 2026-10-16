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
