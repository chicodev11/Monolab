import { describe, expect, it } from "vitest";
import { applyLine, emptyChat, markAnswered, settle, toolName, toolSummary, type ChatModel } from "./claudeChat";

const run = (lines: unknown[], start: ChatModel = emptyChat) =>
  lines.reduce<ChatModel>((model, line) => applyLine(model, typeof line === "string" ? line : JSON.stringify(line)), start);

describe("claudeChat parses stream-json lines", () => {
  it("streams text into one item and settles it when complete", () => {
    const model = run([
      { type: "system", subtype: "init", model: "claude-opus-5-5", session_id: "s" },
      { type: "user", message: { role: "user", content: "hi" } },
      { type: "stream_event", event: { type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "Hel" } } },
      { type: "stream_event", event: { type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "lo" } } },
    ]);
    expect(model.model).toBe("claude-opus-5-5");
    expect(model.busy).toBe(true);
    expect(model.items.map((one) => one.kind)).toEqual(["user", "assistant"]);
    expect(model.items[1]).toMatchObject({ text: "Hello", streaming: true });

    const done = run(
      [
        { type: "assistant", message: { content: [{ type: "text", text: "Hello!" }] } },
        { type: "result", subtype: "success", is_error: false, result: "Hello!" },
      ],
      model,
    );
    expect(done.busy).toBe(false);
    expect(done.items).toHaveLength(2);
    expect(done.items[1]).toMatchObject({ text: "Hello!", streaming: false });
  });

  it("attaches a tool result to its call", () => {
    const model = run([
      { type: "assistant", message: { content: [{ type: "tool_use", id: "t1", name: "Write", input: { file_path: "a.txt" } }] } },
      { type: "assistant", message: { content: [{ type: "tool_use", id: "t1", name: "Write", input: { file_path: "a.txt" } }] } },
      { type: "user", message: { content: [{ type: "tool_result", tool_use_id: "t1", content: [{ type: "text", text: "ok" }] }] } },
    ]);
    expect(model.items).toHaveLength(1);
    expect(model.items[0]).toMatchObject({ kind: "tool", name: "Write", result: "ok", failed: false });
  });

  it("shows permission requests as cards and marks the answer", () => {
    const line = {
      type: "control_request",
      request_id: "r1",
      request: { subtype: "can_use_tool", tool_name: "Bash", display_name: "Bash", input: { command: "ls" }, permission_suggestions: [{ type: "setMode" }] },
    };
    const model = run([line, line]);
    expect(model.items).toHaveLength(1);
    expect(model.items[0]).toMatchObject({ kind: "permission", requestId: "r1", tool: "Bash", suggestions: [{ type: "setMode" }] });
    expect(markAnswered(model, "r1", "allow").items[0]).toMatchObject({ answer: "allow" });
    expect(settle(model).items[0]).toMatchObject({ answer: "deny" });
  });

  it("turns failed results into notes and flags login problems", () => {
    const model = run([{ type: "result", subtype: "success", is_error: true, result: "Invalid API key · Please run /login" }]);
    expect(model.items[0]).toMatchObject({ kind: "note", tone: "danger", login: true });
  });

  it("skips broken, unknown and internal lines", () => {
    const model = run([
      "not json",
      { type: "rate_limit_event" },
      { type: "user", message: { content: [{ type: "text", text: "<command-name>/clear</command-name>" }] } },
    ]);
    expect(model.items).toHaveLength(0);
  });

  it("tool name and summary", () => {
    expect(toolName("mcp__monolab__read_file")).toBe("read_file");
    expect(toolSummary("Bash", { command: "gradle build" })).toBe("gradle build");
    expect(toolSummary("x", {})).toBe("");
  });
});

describe("usage", () => {
  it("reads 5-hour and 7-day usage from rate_limit_event", () => {
    const line = JSON.stringify({
      type: "rate_limit_event",
      rate_limit_info: { status: "allowed", unifiedWindows: { five_hour: { utilization: 0.39, resetsAt: 1791552600 }, seven_day: { utilization: 0.4, resetsAt: 1791828000 } } },
    });
    const next = applyLine(emptyChat, line);
    expect(next.usage).toEqual({ fiveHour: { used: 0.39, resetsAt: 1791552600 }, sevenDay: { used: 0.4, resetsAt: 1791828000 }, status: "allowed" });
  });
  it("follows the model name in replies", () => {
    const line = JSON.stringify({ type: "assistant", message: { model: "claude-haiku-5-5", content: [] } });
    expect(applyLine(emptyChat, line).model).toBe("claude-haiku-5-5");
  });
});

describe("summaries and todos", () => {
  it("folds the compact summary instead of showing it as a user message", () => {
    const line = JSON.stringify({ type: "user", isCompactSummary: true, message: { role: "user", content: "This session is being continued from a previous conversation that ran out of context." } });
    const model = applyLine(emptyChat, line);
    expect(model.items).toHaveLength(1);
    expect(model.items[0].kind).toBe("summary");
    expect(model.busy).toBe(false);
  });

  it("keeps the latest todo list and the session id", () => {
    let model = applyLine(emptyChat, JSON.stringify({ type: "system", subtype: "init", model: "m", session_id: "abc-1" }));
    model = applyLine(
      model,
      JSON.stringify({
        type: "assistant",
        message: { content: [{ type: "tool_use", id: "t1", name: "TodoWrite", input: { todos: [{ content: "Read", activeForm: "Reading", status: "in_progress" }, { content: "Fix", activeForm: "Fixing", status: "pending" }] } }] },
      }),
    );
    expect(model.session).toBe("abc-1");
    expect(model.todos?.map((one) => one.status)).toEqual(["in_progress", "pending"]);
  });
});
