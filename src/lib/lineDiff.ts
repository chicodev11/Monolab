export type DiffLine = { kind: "same" | "add" | "remove"; text: string };

/** Line-by-line difference from `before` to `after`, or null when the changed middle is too large to compare. */
export function lineDiff(before: string, after: string, limit = 4_000_000): DiffLine[] | null {
  const a = before.replace(/\r\n/g, "\n").split("\n");
  const b = after.replace(/\r\n/g, "\n").split("\n");
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start += 1;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA -= 1;
    endB -= 1;
  }
  const midA = a.slice(start, endA);
  const midB = b.slice(start, endB);
  const n = midA.length;
  const m = midB.length;
  if (n * m > limit) return null;
  const width = m + 1;
  const table = new Uint32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      table[i * width + j] =
        midA[i] === midB[j] ? table[(i + 1) * width + j + 1] + 1 : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
    }
  }
  const out: DiffLine[] = a.slice(0, start).map((text) => ({ kind: "same", text }));
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && midA[i] === midB[j]) {
      out.push({ kind: "same", text: midA[i] });
      i += 1;
      j += 1;
    } else if (i < n && (j === m || table[(i + 1) * width + j] >= table[i * width + j + 1])) {
      out.push({ kind: "remove", text: midA[i] });
      i += 1;
    } else {
      out.push({ kind: "add", text: midB[j] });
      j += 1;
    }
  }
  for (const text of a.slice(endA)) out.push({ kind: "same", text });
  return out;
}

export type DiffRow = DiffLine | { kind: "gap"; count: number };

/** Keeps `context` unchanged lines around each change and folds the rest into gaps. */
export function foldDiff(lines: DiffLine[], context = 2): DiffRow[] {
  const near = lines.map(() => false);
  lines.forEach((line, at) => {
    if (line.kind === "same") return;
    for (let k = Math.max(0, at - context); k <= Math.min(lines.length - 1, at + context); k += 1) near[k] = true;
  });
  const out: DiffRow[] = [];
  let hidden = 0;
  lines.forEach((line, at) => {
    if (near[at]) {
      if (hidden) out.push({ kind: "gap", count: hidden });
      hidden = 0;
      out.push(line);
    } else hidden += 1;
  });
  if (hidden) out.push({ kind: "gap", count: hidden });
  return out;
}
