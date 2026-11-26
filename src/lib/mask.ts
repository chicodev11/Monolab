/**
 * Finds the parts of a console or log line that shouldn't sit on screen in the open: IP addresses,
 * the path of links (the site stays readable), e-mail addresses and values after words like
 * "token=" or "password:". The screen blurs them until clicked.
 */
export type Part = { text: string; hidden?: boolean };

const SECRET = /\b(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|auth|rcon\.password|management-server-secret)\b\s*[=:]\s*("[^"]*"|'[^']*'|\S+)/gi;
const URL = /\bhttps?:\/\/[^\s<>"'`]+/gi;
const EMAIL = /\b[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b/g;
const IPV4 = /\b(?:\d{1,3}\.){3}\d{1,3}(?::\d{1,5})?\b/g;
const IPV6 = /\[[0-9a-f:]{2,39}(?:%\w+)?\](?::\d{1,5})?|\b(?:[0-9a-f]{1,4}:){7}[0-9a-f]{1,4}\b/gi;

/** Addresses that say nothing about anyone: this machine, or "any address". */
function harmless(ip: string): boolean {
  const bare = ip.replace(/:\d+$/, "");
  return bare === "127.0.0.1" || bare === "0.0.0.0" || bare === "255.255.255.255";
}

function realIpv4(match: string): boolean {
  return match
    .replace(/:\d+$/, "")
    .split(".")
    .every((part) => Number(part) <= 255);
}

type Span = { start: number; end: number };

function spans(text: string): Span[] {
  const found: Span[] = [];
  const add = (start: number, end: number) => {
    if (end > start) found.push({ start, end });
  };
  for (const match of text.matchAll(SECRET)) {
    const value = match[1];
    const at = match.index + match[0].length - value.length;
    add(at, at + value.length);
  }
  for (const match of text.matchAll(URL)) {
    // Keep "https://example.com/" readable and hide what comes after it.
    const head = /^https?:\/\/[^/?#]+[/?#]?/i.exec(match[0])?.[0] ?? match[0];
    const host = head.replace(/^https?:\/\//i, "").replace(/[/?#]$/, "");
    if (/^(?:\d{1,3}\.){3}\d{1,3}(?::\d+)?$/.test(host) && !harmless(host)) add(match.index + head.indexOf(host), match.index + head.indexOf(host) + host.length);
    add(match.index + head.length, match.index + match[0].length);
  }
  for (const match of text.matchAll(EMAIL)) add(match.index, match.index + match[0].length);
  for (const match of text.matchAll(IPV4)) {
    if (realIpv4(match[0]) && !harmless(match[0])) add(match.index, match.index + match[0].length);
  }
  for (const match of text.matchAll(IPV6)) {
    if (!/^\[?::1\]?/.test(match[0])) add(match.index, match.index + match[0].length);
  }
  found.sort((a, b) => a.start - b.start || b.end - a.end);
  const merged: Span[] = [];
  for (const one of found) {
    const last = merged[merged.length - 1];
    if (last && one.start <= last.end) last.end = Math.max(last.end, one.end);
    else merged.push({ ...one });
  }
  return merged;
}

/** The line in order, with the parts to hide marked. */
export function maskParts(text: string): Part[] {
  if (!text) return [{ text }];
  const found = spans(text);
  if (!found.length) return [{ text }];
  const out: Part[] = [];
  let at = 0;
  for (const one of found) {
    if (one.start > at) out.push({ text: text.slice(at, one.start) });
    out.push({ text: text.slice(one.start, one.end), hidden: true });
    at = one.end;
  }
  if (at < text.length) out.push({ text: text.slice(at) });
  return out;
}
