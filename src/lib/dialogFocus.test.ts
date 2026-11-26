import { afterEach, describe, expect, it } from "vitest";
import { focusables, nextInside, topDialog, watchDialogFocus } from "./dialogFocus";

afterEach(() => {
  document.body.innerHTML = "";
});

function dialog(html: string) {
  const box = document.createElement("div");
  box.setAttribute("role", "dialog");
  box.setAttribute("aria-modal", "true");
  box.innerHTML = html;
  document.body.append(box);
  return box;
}

describe("dialog focus", () => {
  it("finds the topmost dialog and what can take focus", () => {
    dialog("<button>a</button>");
    const top = dialog('<button>b</button><button disabled>c</button><input /><span tabindex="-1">d</span>');
    expect(topDialog()).toBe(top);
    expect(focusables(top).map((one) => one.tagName)).toEqual(["BUTTON", "INPUT"]);
  });

  it("wraps Tab at both ends", () => {
    const box = dialog("<button>a</button><button>b</button><button>c</button>");
    const [a, b, c] = [...box.querySelectorAll("button")];
    expect(nextInside(box, c, false)).toBe(a);
    expect(nextInside(box, a, true)).toBe(c);
    expect(nextInside(box, b, false)).toBeNull();
    expect(nextInside(box, document.body, false)).toBe(a);
  });

  it("gives focus back when the dialog closes", async () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const stop = watchDialogFocus();
    const box = dialog("<button>inside</button>");
    await new Promise((done) => setTimeout(done, 30));
    expect(document.activeElement).toBe(box.querySelector("button"));
    box.remove();
    await new Promise((done) => setTimeout(done, 30));
    expect(document.activeElement).toBe(opener);
    stop();
  });
});
