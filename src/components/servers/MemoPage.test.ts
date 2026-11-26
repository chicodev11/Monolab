import { describe, expect, it } from "vitest";
import { mergeMemos, parseMemo, statusOf, withStatus } from "./MemoPage";

describe("parseMemo", () => {
  it("turns the old single note and its pictures into the first memo", () => {
    const memo = parseMemo(JSON.stringify({ notes: "hello", images: ["1.png"], todos: [{ id: "a", text: "x", done: true }] }));
    expect(memo.memos).toHaveLength(1);
    expect(memo.memos[0]).toMatchObject({ title: "", text: "hello", images: ["1.png"] });
    expect(memo.todos[0]).toEqual({ id: "a", text: "x", done: true, status: "done", images: [] });
  });
  it("keeps memos and to-do pictures as saved", () => {
    const memo = parseMemo(JSON.stringify({ memos: [{ id: "m", title: "Ports", text: "", images: ["2.png"] }], todos: [{ id: "t", text: "y", done: false, images: ["3.png"] }] }));
    expect(memo.memos).toEqual([{ id: "m", title: "Ports", text: "", images: ["2.png"] }]);
    expect(memo.todos[0].images).toEqual(["3.png"]);
  });
  it("reads_old_todos_as_status", () => {
    expect(statusOf({ done: true })).toBe("done");
    expect(statusOf({ done: false })).toBe("todo");
    expect(statusOf({ done: false, status: "doing" })).toBe("doing");
  });
  it("saving_keeps_done_in_step", () => {
    const todo = parseMemo(JSON.stringify({ todos: [{ id: "a", text: "x", done: false }] })).todos[0];
    expect(withStatus(todo, "done")).toMatchObject({ status: "done", done: true });
    expect(withStatus(todo, "doing")).toMatchObject({ status: "doing", done: false });
  });
  it("merge_keeps_both_sides_new_todos", () => {
    const theirs = parseMemo(JSON.stringify({ todos: [{ id: "a", text: "old", done: false, updated: 5 }, { id: "t", text: "theirs", done: false }] }));
    const mine = parseMemo(JSON.stringify({ todos: [{ id: "a", text: "mine", done: false, updated: 9 }, { id: "m", text: "mine new", done: false }] }));
    const merged = mergeMemos(theirs, mine);
    expect(merged.todos.map((one) => one.id)).toEqual(["a", "t", "m"]);
    expect(merged.todos[0].text).toBe("mine");
  });
  it("starts empty from nothing or junk", () => {
    expect(parseMemo("")).toEqual({ todos: [], memos: [] });
    expect(parseMemo(JSON.stringify({ memos: [] }))).toEqual({ todos: [], memos: [] });
  });
});
