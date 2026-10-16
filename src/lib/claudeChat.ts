
export type ChatItem =
  | { kind: "user"; key: string; text: string; images: number }
  | { kind: "assistant"; key: string; text: string; streaming: boolean }
  | { kind: "tool"; key: string; id: string; name: string; input: Record<string, unknown>; result?: string; failed?: boolean }
  | {
      kind: "permission";
      key: string;
      requestId: string;
      tool: string;
      title: string;
      description: string;
      input: Record<string, unknown>;
      suggestions: unknown[];
      answer?: "allow" | "always" | "deny";
    }
  | { kind: "note"; key: string; text: string; tone: "muted" | "danger"; login?: boolean };

export type ChatLimit = { used: number; resetsAt: number };
export type ChatUsage = { fiveHour?: ChatLimit; sevenDay?: ChatLimit; status?: string };

export type ChatModel = {
  items: ChatItem[];
  usage?: ChatUsage;
  busy: boolean;
  model: string;
  serial: number;
};

export const emptyChat: ChatModel = { items: [], busy: false, model: "", serial: 0 };

type Json = Record<string, unknown>;

const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null && !Array.isArray(value);
const text = (value: unknown) => (typeof value === "string" ? value : "");

function resultText(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .map((one) => (isObject(one) ? (one.type === "text" ? text(one.text) : one.type === "image" ? "[image]" : "") : ""))
    .filter(Boolean)
    .join("\n");
}

export function looksLikeLogin(message: string) {
  return /\/login|log ?in|not logged|api key|authenticat|OAuth/i.test(message);
}

export function applyLine(model: ChatModel, line: string): ChatModel {
  let message: Json;
  try {
    const parsed: unknown = JSON.parse(line);
    if (!isObject(parsed)) return model;
    message = parsed;
  } catch {
    return model;
  }
  const items = [...model.items];
  let serial = model.serial;
  const key = () => `c${++serial}`;
  let { busy, model: name } = model;
  let usage = model.usage;

  switch (message.type) {
    case "system": {
      if (message.subtype === "init") name = text(message.model) || name;
      break;
    }
    case "user": {
      const body = isObject(message.message) ? message.message : {};
      const content = body.content;
      if (typeof content === "string") {
        if (content.trim()) items.push({ kind: "user", key: key(), text: content, images: 0 });
        busy = true;
        break;
      }
      if (!Array.isArray(content)) break;
      const words: string[] = [];
      let images = 0;
      let said = false;
      for (const block of content) {
        if (!isObject(block)) continue;
        if (block.type === "tool_result") {
          const id = text(block.tool_use_id);
          const at = items.findIndex((one) => one.kind === "tool" && one.id === id);
          if (at >= 0) {
            const tool = items[at] as Extract<ChatItem, { kind: "tool" }>;
            items[at] = { ...tool, result: resultText(block.content), failed: block.is_error === true };
          }
        } else if (block.type === "text") {
          words.push(text(block.text));
          said = true;
        } else if (block.type === "image") {
          images += 1;
          said = true;
        }
      }
      if (said) {
        const joined = words.join("\n").trim();
        if (!/^<[a-z-]+>/.test(joined) || images) {
          items.push({ kind: "user", key: key(), text: joined, images });
          busy = true;
        }
      }
      break;
    }
    case "stream_event": {
      const event = isObject(message.event) ? message.event : {};
      if (event.type === "content_block_delta" && isObject(event.delta) && event.delta.type === "text_delta") {
        const piece = text(event.delta.text);
        const last = items[items.length - 1];
        if (last?.kind === "assistant" && last.streaming) items[items.length - 1] = { ...last, text: last.text + piece };
        else items.push({ kind: "assistant", key: key(), text: piece, streaming: true });
      }
      break;
    }
    case "assistant": {
      const body = isObject(message.message) ? message.message : {};
      name = text(body.model) || name;
      const content = Array.isArray(body.content) ? body.content : [];
      for (const block of content) {
        if (!isObject(block)) continue;
        if (block.type === "text") {
          const full = text(block.text);
          const at = items.findIndex((one) => one.kind === "assistant" && one.streaming);
          if (at >= 0) items[at] = { ...(items[at] as Extract<ChatItem, { kind: "assistant" }>), text: full, streaming: false };
          else if (full.trim()) items.push({ kind: "assistant", key: key(), text: full, streaming: false });
        } else if (block.type === "tool_use") {
          const id = text(block.id);
          if (!items.some((one) => one.kind === "tool" && one.id === id)) {
            items.push({ kind: "tool", key: key(), id, name: text(block.name), input: isObject(block.input) ? block.input : {} });
          }
        }
      }
      break;
    }
    case "control_request": {
      const request = isObject(message.request) ? message.request : {};
      if (request.subtype !== "can_use_tool") break;
      const requestId = text(message.request_id);
      if (items.some((one) => one.kind === "permission" && one.requestId === requestId)) break;
      items.push({
        kind: "permission",
        key: key(),
        requestId,
        tool: text(request.tool_name),
        title: text(request.display_name) || text(request.tool_name),
        description: text(request.description),
        input: isObject(request.input) ? request.input : {},
        suggestions: Array.isArray(request.permission_suggestions) ? request.permission_suggestions : [],
      });
      break;
    }
    case "result": {
      busy = false;
      for (let at = 0; at < items.length; at += 1) {
        const one = items[at];
        if (one.kind === "assistant" && one.streaming) items[at] = { ...one, streaming: false };
      }
      if (message.is_error === true || (message.subtype && message.subtype !== "success")) {
        const said = text(message.result) || (Array.isArray(message.errors) ? message.errors.map(text).join("\n") : "") || text(message.subtype);
        if (said) items.push({ kind: "note", key: key(), text: said, tone: "danger", login: looksLikeLogin(said) });
      }
      break;
    }
    case "rate_limit_event": {
      const info = isObject(message.rate_limit_info) ? message.rate_limit_info : {};
      const windows = isObject(info.unifiedWindows) ? info.unifiedWindows : {};
      const limit = (value: unknown): ChatLimit | undefined =>
        isObject(value) && typeof value.utilization === "number"
          ? { used: value.utilization, resetsAt: typeof value.resetsAt === "number" ? value.resetsAt : 0 }
          : undefined;
      usage = {
        fiveHour: limit(windows.five_hour) ?? usage?.fiveHour,
        sevenDay: limit(windows.seven_day) ?? usage?.sevenDay,
        status: text(info.status) || usage?.status,
      };
      break;
    }
    case "monolab_stderr": {
      const said = text(message.text);
      if (said) items.push({ kind: "note", key: key(), text: said, tone: looksLikeLogin(said) ? "danger" : "muted", login: looksLikeLogin(said) });
      break;
    }
    default:
      return model;
  }
  return { items, busy, model: name, serial, usage };
}

export function markAnswered(model: ChatModel, requestId: string, answer: "allow" | "always" | "deny"): ChatModel {
  return {
    ...model,
    items: model.items.map((one) => (one.kind === "permission" && one.requestId === requestId ? { ...one, answer } : one)),
  };
}

export function settle(model: ChatModel): ChatModel {
  return {
    ...model,
    busy: false,
    items: model.items.map((one) =>
      one.kind === "assistant" && one.streaming
        ? { ...one, streaming: false }
        : one.kind === "permission" && !one.answer
          ? { ...one, answer: "deny" as const }
          : one,
    ),
  };
}

export function toolSummary(_name: string, input: Record<string, unknown>): string {
  const pick = (...keys: string[]) => {
    for (const one of keys) if (typeof input[one] === "string" && input[one]) return input[one] as string;
    return "";
  };
  const said = pick("command", "file_path", "path", "pattern", "url", "query", "description", "notebook_path", "server", "name");
  return said.length > 140 ? `${said.slice(0, 140)}…` : said;
}

export function toolName(name: string) {
  return name.replace(/^mcp__monolab__/, "");
}
