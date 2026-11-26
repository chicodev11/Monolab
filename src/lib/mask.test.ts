import { describe, expect, it } from "vitest";
import { maskParts } from "./mask";

const hidden = (text: string) => maskParts(text).filter((one) => one.hidden).map((one) => one.text);

describe("hiding private parts of console lines", () => {
  it("hides player IPs but not the time or versions", () => {
    expect(hidden("[12:05:10 INFO]: mina_k[/203.0.113.7:51234] logged in with entity id 12")).toEqual(["203.0.113.7:51234"]);
    expect(hidden("Starting minecraft server version 1.21.4")).toEqual([]);
    expect(hidden("[12:03:38 INFO]: Done (4.213s)!")).toEqual([]);
  });

  it("leaves this machine's addresses alone", () => {
    expect(hidden("Starting Minecraft server on 0.0.0.0:25565")).toEqual([]);
    expect(hidden("RCON running on 127.0.0.1:25575")).toEqual([]);
  });

  it("keeps a link's site and hides the rest", () => {
    const parts = maskParts("See https://example.com/invite/abc123?key=9 now");
    expect(parts.map((one) => one.text).join("")).toBe("See https://example.com/invite/abc123?key=9 now");
    expect(hidden("See https://example.com/invite/abc123?key=9 now")).toEqual(["invite/abc123?key=9"]);
    expect(hidden("Go to https://example.com")).toEqual([]);
  });

  it("hides secrets, e-mails and IPv6", () => {
    expect(hidden("token=abc.def password: hunter2")).toEqual(["abc.def", "hunter2"]);
    expect(hidden("mail me at someone@example.org")).toEqual(["someone@example.org"]);
    expect(hidden("joined from [2001:db8::7]:5000")).toEqual(["[2001:db8::7]:5000"]);
  });
});
