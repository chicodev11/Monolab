import { describe, expect, it } from "vitest";
import { safeHtml } from "./safeHtml";

describe("safeHtml", () => {
  it("drops scripts and event handlers", () => {
    const out = safeHtml(`<p>hi</p><img src=x onerror="alert(1)"><script>alert(2)</script><svg><animate onbegin="alert(3)"/></svg>`);
    expect(out).toContain("<p>hi</p>");
    expect(out).not.toMatch(/onerror|onbegin|<script|alert/);
  });

  it("drops javascript links, frames and forms", () => {
    const out = safeHtml(`<a href="javascript:alert(1)">a</a><iframe src="https://x"></iframe><form action="https://x"><input></form>`);
    expect(out).not.toMatch(/javascript:|<iframe|<form|<input/);
  });

  it("keeps ordinary project pages", () => {
    const out = safeHtml(`<h2>Install</h2><a href="https://modrinth.com/x">link</a><img src="https://cdn.modrinth.com/a.png" alt="a"><code>/op</code>`);
    expect(out).toContain(`href="https://modrinth.com/x"`);
    expect(out).toContain(`src="https://cdn.modrinth.com/a.png"`);
    expect(out).toContain("<code>/op</code>");
  });

  it("handles empty input", () => {
    expect(safeHtml(null)).toBe("");
    expect(safeHtml(undefined)).toBe("");
  });
});
