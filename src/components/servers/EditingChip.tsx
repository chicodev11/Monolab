import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, monolinkWhoAmI, onMonolink, onWeb } from "../../lib/bridge";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

type OpenFile = { path: string; people: string[] };
type Changed = { serverId: string; path: string; people: string[] };

/** How often an open file is said to be still open (the host forgets it after a minute). */
const EVERY_MS = 30_000;

/** How a name reads in the chip: the host's own window is "the host". */
export function personLabel(name: string): string {
  return name === "host" ? tr("the host") : name;
}

/** Everyone but me, in a short line: "@amy is editing", "@amy and 1 more are editing". */
export function editingLine(people: string[], me: string): string | null {
  const others = people.filter((one) => one !== me);
  if (!others.length) return null;
  if (others.length === 1) return tr("{0} is editing", [personLabel(others[0])]);
  return tr("{0} and {1, plural, one {# more} other {# more}} are editing", [personLabel(others[0]), others.length - 1]);
}

/**
 * F2: says this window has `path` open on the host (every 30 seconds, and "closed" when it goes), and shows who
 * else has it open. In the host's window and teammates' windows; the web panel doesn't take part.
 */
export function EditingChip({ serverId, path }: { serverId: string; path: string }) {
  const [people, setPeople] = useState<string[]>([]);
  const [me, setMe] = useState(onMonolink ? "" : "host");

  useEffect(() => {
    if (!onMonolink) return;
    void monolinkWhoAmI().then((who) => setMe(who.name), () => {});
  }, []);

  useEffect(() => {
    if ((onWeb && !onMonolink) || !path) return;
    let gone = false;
    const tell = (closed: boolean) =>
      (onMonolink ? call<OpenFile[]>("__editing", { id: serverId, path, closed }) : call<OpenFile[]>("file_editing", { id: serverId, path, closed })).then(
        (open) => {
          if (!gone && !closed) setPeople(open.find((one) => one.path === path.replace(/^\/+/, ""))?.people ?? []);
        },
        () => {},
      );
    void tell(false);
    const timer = window.setInterval(() => void tell(false), EVERY_MS);
    let stop: (() => void) | undefined;
    void listen<Changed>("file-editing", (event) => {
      if (event.payload.serverId === serverId && event.payload.path === path.replace(/^\/+/, "")) setPeople(event.payload.people);
    }).then((off) => {
      if (gone) off();
      else stop = off;
    }, () => {});
    return () => {
      gone = true;
      window.clearInterval(timer);
      stop?.();
      void tell(true);
      setPeople([]);
    };
  }, [serverId, path]);

  const line = editingLine(people, me);
  if (!line) return null;
  return (
    <span className="chip subtle editing-chip" role="status" data-tip={people.filter((one) => one !== me).map(personLabel).join(", ")}>
      <Icon name="users" />
      {line}
    </span>
  );
}
