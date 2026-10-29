/** Returns the old value when the new one has the same JSON, so React skips the re-render. */
/** Like keepSame, but ignores the timestamp, which changes on every read even when nothing else did. */
export function keepSameLive<T extends { at: number } | null>(old: T, next: T): T {
  if (!old || !next) return next;
  try {
    return JSON.stringify({ ...old, at: 0 }) === JSON.stringify({ ...next, at: 0 }) ? old : next;
  } catch {
    return next;
  }
}

export function keepSame<T>(old: T, next: T): T {
  if (old === next) return old;
  try {
    return JSON.stringify(old) === JSON.stringify(next) ? old : next;
  } catch {
    return next;
  }
}
