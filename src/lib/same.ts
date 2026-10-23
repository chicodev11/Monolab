/** Returns the old value when the new one has the same JSON, so React skips the re-render. */
export function keepSame<T>(old: T, next: T): T {
  if (old === next) return old;
  try {
    return JSON.stringify(old) === JSON.stringify(next) ? old : next;
  } catch {
    return next;
  }
}
