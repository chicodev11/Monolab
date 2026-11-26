import { tr } from "./i18n";

const DEFAULT = /^(?:Server|\uC11C\uBC84) (\d+)$/;

/**
 * A server keeps the default name "Server N" until the user names it; that default is shown in the
 * app's language. A name the user picked (`named`) is shown as it is.
 */
export function shownServerName(name: string, named?: boolean | null): string {
  if (named) return name;
  const match = DEFAULT.exec(name);
  return match ? tr("Server {0}", [match[1]]) : name;
}

export function withShownNames<T extends { name: string; named?: boolean | null }>(list: T[]): T[] {
  return list.map((one) => {
    const shown = shownServerName(one.name, one.named);
    return shown === one.name ? one : { ...one, name: shown };
  });
}
