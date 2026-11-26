import { describe, expect, it } from "vitest";
import { isSavedOver, sha256Hex } from "./hash";

describe("sha256Hex", () => {
  it("matches the standard test vector", async () => {
    expect(await sha256Hex("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  });
  it("hashes CRLF text as it is", async () => {
    expect(await sha256Hex("a\r\n")).not.toBe(await sha256Hex("a\n"));
  });
});

describe("isSavedOver", () => {
  it("knows the host's refusal", () => {
    expect(isSavedOver("Error: @amy saved this file after you opened it.")).toBe(true);
    expect(isSavedOver("Could not save the text file")).toBe(false);
  });
});
