
export type AnsiStyle = {
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
};

export type AnsiPiece = { text: string; style: AnsiStyle };

const BASIC = [
  "#4d4d4d", "#e5534b", "#57ab5a", "#c69026", "#539bf5", "#b083f0", "#39c5cf", "#c9d1d9",
  "#768390", "#ff6b64", "#6bc46d", "#e6c35c", "#6cb6ff", "#dcbdfb", "#56d4dd", "#ffffff",
];

const MINECRAFT: Record<string, string> = {
  "0": "#000000", "1": "#0000aa", "2": "#00aa00", "3": "#00aaaa",
  "4": "#aa0000", "5": "#aa00aa", "6": "#ffaa00", "7": "#aaaaaa",
  "8": "#555555", "9": "#5555ff", a: "#55ff55", b: "#55ffff",
  c: "#ff5555", d: "#ff55ff", e: "#ffff55", f: "#ffffff",
};

function color256(n: number): string | undefined {
  if (n < 0 || n > 255) return undefined;
  if (n < 16) return BASIC[n];
  if (n < 232) {
    const at = n - 16;
    const step = (v: number) => (v === 0 ? 0 : 55 + v * 40);
    return `rgb(${step(Math.floor(at / 36))}, ${step(Math.floor(at / 6) % 6)}, ${step(at % 6)})`;
  }
  const grey = 8 + (n - 232) * 10;
  return `rgb(${grey}, ${grey}, ${grey})`;
}

function applySgr(style: AnsiStyle, params: number[]): AnsiStyle {
  let next = { ...style };
  const codes = params.length ? params : [0];
  for (let i = 0; i < codes.length; i++) {
    const code = codes[i];
    if (code === 0) next = {};
    else if (code === 1) next.bold = true;
    else if (code === 2) next.dim = true;
    else if (code === 3) next.italic = true;
    else if (code === 4) next.underline = true;
    else if (code === 9) next.strike = true;
    else if (code === 22) next.bold = next.dim = false;
    else if (code === 23) next.italic = false;
    else if (code === 24) next.underline = false;
    else if (code === 29) next.strike = false;
    else if (code >= 30 && code <= 37) next.color = BASIC[code - 30];
    else if (code >= 90 && code <= 97) next.color = BASIC[code - 90 + 8];
    else if (code === 39) delete next.color;
    else if (code >= 40 && code <= 47) next.background = BASIC[code - 40];
    else if (code >= 100 && code <= 107) next.background = BASIC[code - 100 + 8];
    else if (code === 49) delete next.background;
    else if (code === 38 || code === 48) {
      const key = code === 38 ? "color" : "background";
      if (codes[i + 1] === 5) {
        const found = color256(codes[i + 2] ?? -1);
        if (found) next[key] = found;
        i += 2;
      } else if (codes[i + 1] === 2) {
        const [r, g, b] = [codes[i + 2], codes[i + 3], codes[i + 4]].map((v) => Math.max(0, Math.min(255, v ?? 0)));
        next[key] = `rgb(${r}, ${g}, ${b})`;
        i += 4;
      }
    }
  }
  return next;
}

function applySection(style: AnsiStyle, code: string): AnsiStyle {
  const lower = code.toLowerCase();
  if (MINECRAFT[lower]) return { color: MINECRAFT[lower] };
  if (lower === "r") return {};
  if (lower === "l") return { ...style, bold: true };
  if (lower === "o") return { ...style, italic: true };
  if (lower === "n") return { ...style, underline: true };
  if (lower === "m") return { ...style, strike: true };
  return style;
}

export function parseAnsi(line: string): AnsiPiece[] {
  const pieces: AnsiPiece[] = [];
  let style: AnsiStyle = {};
  let text = "";
  const flush = () => {
    if (text) pieces.push({ text, style });
    text = "";
  };
  for (let i = 0; i < line.length; i++) {
    const one = line[i];
    if (one === "\u001b" && line[i + 1] === "[") {
      let end = i + 2;
      while (end < line.length && !(line.charCodeAt(end) >= 0x40 && line.charCodeAt(end) <= 0x7e)) end++;
      if (line[end] === "m") {
        flush();
        const params = line
          .slice(i + 2, end)
          .split(/[;:]/)
          .filter((part) => part !== "")
          .map(Number)
          .filter((n) => Number.isFinite(n));
        style = applySgr(style, params);
      }
      i = end;
      continue;
    }
    if (one === "§" && i + 1 < line.length && /[0-9a-fk-or]/i.test(line[i + 1])) {
      flush();
      style = applySection(style, line[i + 1]);
      i += 1;
      continue;
    }
    text += one;
  }
  flush();
  return pieces;
}

export function hasStyle(pieces: AnsiPiece[]): boolean {
  return pieces.some((piece) => Object.values(piece.style).some(Boolean));
}
