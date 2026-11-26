import { beforeEach, describe, expect, it, vi } from "vitest";

const sent: { command: string; args: Record<string, unknown> }[] = [];

vi.mock("./bridge", () => ({
  onMonolink: true,
  call: async (command: string, args: Record<string, unknown>) => {
    sent.push({ command, args });
    if (command === "__upload_begin") return { upload: "u1" };
    if (command === "__upload_end") return "123-4/plugin.jar";
    return 0;
  },
}));

const { uploadRaw } = await import("./pickFile");

function bytesOf(base64: string) {
  return Uint8Array.from(atob(base64), (letter) => letter.charCodeAt(0));
}

describe("uploading through Monolink", () => {
  beforeEach(() => {
    sent.length = 0;
  });

  it("sends a file in 1 MB pieces, in order, and gives the host's token", async () => {
    const data = new Uint8Array(2.5 * 1024 * 1024).map((_, at) => (at * 7) % 256);
    const token = await uploadRaw(new File([data], "plugin.jar"), "");
    expect(token).toBe("123-4/plugin.jar");
    expect(sent[0]).toEqual({ command: "__upload_begin", args: { name: "plugin.jar", size: data.length } });
    const parts = sent.filter((one) => one.command === "__upload_part");
    expect(parts.map((one) => one.args.at)).toEqual([0, 1024 * 1024, 2 * 1024 * 1024]);
    const pieces = parts.map((one) => bytesOf(one.args.data as string));
    const joined = new Uint8Array(pieces.reduce((sum, one) => sum + one.length, 0));
    pieces.reduce((at, one) => (joined.set(one, at), at + one.length), 0);
    expect(joined.length).toBe(data.length);
    expect(joined.every((value, at) => value === data[at])).toBe(true);
    expect(sent.at(-1)).toEqual({ command: "__upload_end", args: { upload: "u1" } });
  });

  it("sends an empty file as just a start and an end", async () => {
    await uploadRaw(new File([], "empty.txt"), "");
    expect(sent.map((one) => one.command)).toEqual(["__upload_begin", "__upload_end"]);
  });
});
