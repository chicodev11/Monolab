import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { BLURRED_SLOWDOWN, period, useAttention } from "./polling";

describe("period", () => {
  test("full speed when visible, slower when blurred, paused when hidden", () => {
    expect(period(1000, "active")).toBe(1000);
    expect(period(1000, "blurred")).toBe(1000 * BLURRED_SLOWDOWN);
    expect(period(1000, "hidden")).toBeNull();
  });
});

describe("useAttention", () => {
  afterEach(() => vi.restoreAllMocks());

  test("hidden when covered, blurred without focus, active again on return", () => {
    let visibility: DocumentVisibilityState = "visible";
    vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
    const focus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const { result } = renderHook(() => useAttention());
    expect(result.current).toBe("active");

    act(() => {
      focus.mockReturnValue(false);
      window.dispatchEvent(new Event("blur"));
    });
    expect(result.current).toBe("blurred");

    act(() => {
      visibility = "hidden";
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(result.current).toBe("hidden");

    act(() => {
      visibility = "visible";
      focus.mockReturnValue(true);
      document.dispatchEvent(new Event("visibilitychange"));
      window.dispatchEvent(new Event("focus"));
    });
    expect(result.current).toBe("active");
  });
});
