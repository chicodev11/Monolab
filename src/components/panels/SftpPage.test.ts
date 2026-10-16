import { describe, expect, it } from "vitest";
import { crumbs, visibleRows } from "./SftpPage";

describe("SFTP breadcrumbs", () => {
  it("local starts at the drive list", () => {
    expect(crumbs("C:\\Users\\User", "local").map((one) => one.path)).toEqual(["::drives", "C:\\", "C:\\Users", "C:\\Users\\User"]);
    expect(crumbs("", "local")).toHaveLength(1);
    expect(crumbs("\\\\nas\\share\\folder", "local").map((one) => one.path)).toEqual(["::drives", "\\\\nas\\share", "\\\\nas\\share\\folder"]);
  });

  it("SSH starts at /", () => {
    expect(crumbs("/home/alex/monolab", "ssh").map((one) => one.path)).toEqual(["/", "/home", "/home/alex", "/home/alex/monolab"]);
  });

  it("panel paths are relative to the server folder", () => {
    expect(crumbs("plugins/Foo", "panel").map((one) => one.path)).toEqual(["", "plugins", "plugins/Foo"]);
    expect(crumbs("/plugins", "panel").map((one) => one.path)).toEqual(["", "plugins"]);
  });
});

describe("SFTP visible rows", () => {
  const row = (name: string, directory = false, sizeBytes = 0, modifiedAt = 0) => ({ name, directory, sizeBytes, modifiedAt }) as never;
  const rows = [row("b.txt", false, 5, 2), row(".git", true), row("A", true, 0, 1), row("a.txt", false, 9, 3)];
  it("hides dotfiles, folders first, by name", () => {
    expect(visibleRows(rows, { hidden: false, query: "", sort: "name", desc: false }).map((one) => one.name)).toEqual(["A", "a.txt", "b.txt"]);
  });
  it("largest first, search, show hidden", () => {
    expect(visibleRows(rows, { hidden: true, query: "", sort: "size", desc: true }).map((one) => one.name)).toEqual([".git", "A", "a.txt", "b.txt"]);
    expect(visibleRows(rows, { hidden: false, query: "TXT", sort: "name", desc: false }).map((one) => one.name)).toEqual(["a.txt", "b.txt"]);
  });
});
