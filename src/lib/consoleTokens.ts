/**
 * A typed console command split for showing: the command, target selectors (`@a`), numbers and coordinates
 * (`12`, `~`, `^-3`), and the rest. Shades only, never width: the text sits under the input and has to line up.
 */
export function consoleTokens(text: string): { text: string; kind: "space" | "command" | "selector" | "number" | "word" }[] {
  let seenCommand = false;
  return text.split(/(\s+)/).filter((one) => one.length > 0).map((part) => {
    if (/^\s+$/.test(part)) return { text: part, kind: "space" as const };
    if (!seenCommand) {
      seenCommand = true;
      return { text: part, kind: "command" as const };
    }
    if (/^@[aeprs](\[.*)?$/.test(part)) return { text: part, kind: "selector" as const };
    if (/^[~^]?-?\d*\.?\d+$|^[~^]$/.test(part)) return { text: part, kind: "number" as const };
    return { text: part, kind: "word" as const };
  });
}
