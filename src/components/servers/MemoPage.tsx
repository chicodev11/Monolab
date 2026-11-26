import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { commands } from "../../lib/bindings";
import { call, onMonolink, onWeb } from "../../lib/bridge";
import { sha256Hex } from "../../lib/hash";
import { tr } from "../../lib/i18n";
import { DropdownMenu, SegmentedControl } from "../primitives";
import { Icon } from "../ui/Icon";
import "./MemoPage.css";

export type Status = "todo" | "doing" | "done";
type Todo = { id: string; text: string; done: boolean; status: Status; images: string[]; assignee?: string; by?: string; updated?: number };
type Note = { id: string; title: string; text: string; images: string[] };
type Memo = { todos: Todo[]; memos: Note[] };
const newId = () => `${Date.now()}-${Math.round(Math.random() * 1e6)}`;

const names = (value: unknown) => (Array.isArray(value) ? value.filter((one): one is string => typeof one === "string") : []);
const text = (value: unknown) => (typeof value === "string" ? value : "");

/** Reads memo.json; the single notes field of the first version becomes the first memo. */
export function parseMemo(raw: string): Memo {
  try {
    const doc = JSON.parse(raw) as Record<string, unknown>;
    const todos = (Array.isArray(doc.todos) ? doc.todos : [])
      .filter((one) => one && typeof one.text === "string")
      .map((one) => {
        const status = statusOf(one);
        const todo: Todo = { id: String(one.id ?? newId()), text: one.text as string, done: status === "done", status, images: names(one.images) };
        if (typeof one.assignee === "string" && one.assignee) todo.assignee = one.assignee;
        if (typeof one.by === "string" && one.by) todo.by = one.by;
        if (typeof one.updated === "number") todo.updated = one.updated;
        return todo;
      });
    let memos: Note[] = (Array.isArray(doc.memos) ? doc.memos : [])
      .filter((one) => one && typeof one === "object")
      .map((one) => ({ id: String(one.id ?? newId()), title: text(one.title), text: text(one.text), images: names(one.images) }));
    if (!Array.isArray(doc.memos) && (text(doc.notes).trim() || names(doc.images).length)) {
      memos = [{ id: newId(), title: "", text: text(doc.notes), images: names(doc.images) }];
    }
    return { todos, memos };
  } catch {
    return { todos: [], memos: [] };
  }
}

/** A to-do's board column; files from before the board only have `done` (F3). */
export function statusOf(one: { status?: unknown; done?: unknown }): Status {
  if (one.status === "todo" || one.status === "doing" || one.status === "done") return one.status;
  return one.done === true ? "done" : "todo";
}

/** Moves a to-do to a column, keeping `done` in step so an older app still reads it. */
export function withStatus(todo: Todo, status: Status): Todo {
  return { ...todo, status, done: status === "done", updated: Date.now() };
}

/**
 * Someone saved the memo meanwhile (F3): their version with mine on top. To-dos and memos are matched by id; a
 * to-do changed on both sides keeps the newer one, anything only one side has stays.
 */
export function mergeMemos(theirs: Memo, mine: Memo): Memo {
  const todos = [...theirs.todos];
  for (const one of mine.todos) {
    const at = todos.findIndex((other) => other.id === one.id);
    if (at < 0) todos.push(one);
    else if ((one.updated ?? 0) >= (todos[at].updated ?? 0)) todos[at] = one;
  }
  const memos = [...theirs.memos];
  for (const one of mine.memos) {
    const at = memos.findIndex((other) => other.id === one.id);
    if (at < 0) memos.push(one);
    else memos[at] = one;
  }
  return { todos, memos };
}

const COLUMNS: Status[] = ["todo", "doing", "done"];
function columnLabel(status: Status) {
  return status === "todo" ? tr("To do") : status === "doing" ? tr("Doing") : tr("Done");
}
function personLabel(name: string) {
  return name === "host" ? tr("the host") : name;
}

const PICTURE = new RegExp("^image/(png|jpeg|gif|webp|bmp)$");
const ACCEPT = "image/png,image/jpeg,image/gif,image/webp,image/bmp";

/** A file as base64 without the data URL prefix. */
function base64Of(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

const dropFiles = (event: DragEvent, take: (files: File[]) => void) => {
  const files = [...event.dataTransfer.files];
  if (!files.some((one) => PICTURE.test(one.type))) return;
  event.preventDefault();
  event.stopPropagation();
  take(files);
};
const allowDrop = (event: DragEvent) => {
  if ([...event.dataTransfer.items].some((one) => one.kind === "file")) event.preventDefault();
};
const pasteFiles = (event: ClipboardEvent, take: (files: File[]) => void) => {
  const files = [...event.clipboardData.files];
  if (!files.some((one) => PICTURE.test(one.type))) return;
  event.preventDefault();
  take(files);
};

function MemoImage({ serverId, name, big, onOpen, onClose }: { serverId: string; name: string; big?: boolean; onOpen?: () => void; onClose: () => void }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    void commands.memoImage(serverId, name).then(setUrl, () => setUrl(""));
  }, [serverId, name]);
  return (
    <div className={"memo-image" + (big ? " big" : "")}>
      {url ? (
        onOpen ? (
          <button type="button" className="memo-image-open" aria-label={tr("View")} onClick={onOpen}>
            <img src={url} alt="" />
          </button>
        ) : (
          <img src={url} alt="" />
        )
      ) : (
        <span className="memo-image-missing">
          <Icon name="image" />
        </span>
      )}
      <button type="button" className="wb-icon ghost memo-image-close" aria-label={big ? tr("Close") : tr("Delete")} data-tip={big ? tr("Close") : tr("Delete")} onClick={onClose}>
        <Icon name="close" />
      </button>
    </div>
  );
}

/** Thumbnails with a larger view of the one picked; `small` drops the add tile for to-do rows. */
function Pictures({ serverId, list, small, onAdd, onRemove }: { serverId: string; list: string[]; small?: boolean; onAdd: (files: File[]) => void; onRemove: (name: string) => void }) {
  const [big, setBig] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  const shown = big && list.includes(big) ? big : null;
  if (small && !list.length) return null;
  return (
    <>
      {shown && <MemoImage key={shown} serverId={serverId} name={shown} big onClose={() => setBig(null)} />}
      <div className={"memo-images" + (small ? " small" : "")}>
        {list.map((name) => (
          <MemoImage key={name} serverId={serverId} name={name} onOpen={() => setBig(shown === name ? null : name)} onClose={() => onRemove(name)} />
        ))}
        {!small && (
          <>
            <button type="button" className="memo-image-add" onClick={() => picker.current?.click()}>
              <Icon name="image" />
              <span>{tr("Add picture")}</span>
            </button>
            <input
              ref={picker}
              type="file"
              accept={ACCEPT}
              multiple
              hidden
              onChange={(event) => {
                onAdd([...(event.target.files ?? [])]);
                event.target.value = "";
              }}
            />
          </>
        )}
      </div>
    </>
  );
}

export function MemoPage({
  serverId,
  onError,
  confirm,
  extra,
}: {
  serverId: string;
  /** More cards for the grid (the server's shared AGENTS.md). */
  extra?: ReactNode;
  onError: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [memo, setMemoState] = useState<Memo | null>(null);
  // The newest memo, for picture uploads that finish after more edits.
  const latest = useRef<Memo | null>(null);
  const [adding, setAdding] = useState(0);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [saved, setSaved] = useState(true);
  const pending = useRef<Memo | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const todoPicker = useRef<HTMLInputElement>(null);
  const todoTarget = useRef<string | null>(null);
  // F3: the hash of the memo as last read or saved, so the host refuses to save over someone else's change.
  const opened = useRef<string | null>(null);
  const [view, setView] = useState<"list" | "board">(() => {
    try {
      return localStorage.getItem("monolab-memo-view") === "board" ? "board" : "list";
    } catch {
      return "list";
    }
  });
  const [people, setPeople] = useState<string[]>([]);
  const [dragged, setDragged] = useState<string | null>(null);
  // The page shown on the right: "todo", "rules" or a memo's id.
  const [picked, setPicked] = useState("todo");

  const reload = async () => {
    const raw = await commands.serverMemo(serverId);
    opened.current = await sha256Hex(raw);
    const got = parseMemo(raw);
    latest.current = got;
    setMemoState(got);
    return got;
  };
  useEffect(() => {
    void reload().catch((cause) => onError(String(cause)));
  }, [serverId, onError]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (onWeb && !onMonolink) return;
    void (onMonolink ? call<string[]>("__team_people", { id: serverId }) : commands.memoPeople(serverId)).then(setPeople, () => setPeople([]));
  }, [serverId]);
  const chooseView = (next: "list" | "board") => {
    setView(next);
    try {
      localStorage.setItem("monolab-memo-view", next);
    } catch {
      // No storage: the choice lasts until the tab closes.
    }
  };

  // The agent can add or check off to-dos; reload unless an edit here is still waiting to be saved.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    // The host's window gets the server id; teammates' windows get `{ serverId }`.
    void listen<string | { serverId?: string }>("memo-changed", (event) => {
      const id = typeof event.payload === "string" ? event.payload : event.payload?.serverId;
      if (id !== serverId || pending.current) return;
      void reload().catch(() => {});
    })
      .then((unlisten) => {
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  }, [serverId]); // eslint-disable-line react-hooks/exhaustive-deps -- reload reads the current serverId

  const flush = () => {
    window.clearTimeout(timer.current);
    const next = pending.current;
    if (!next) return;
    pending.current = null;
    const save = async () => {
      try {
        opened.current = await commands.setServerMemo(serverId, JSON.stringify(next), opened.current);
      } catch (cause) {
        if (!String(cause).includes("saved this memo after you opened it")) throw cause;
        // Someone saved meanwhile: put my changes on top of theirs and save once more.
        const raw = await commands.serverMemo(serverId);
        const merged = mergeMemos(parseMemo(raw), next);
        latest.current = merged;
        setMemoState(merged);
        opened.current = await commands.setServerMemo(serverId, JSON.stringify(merged), await sha256Hex(raw));
      }
      setSaved(true);
    };
    void save().catch((cause) => onError(String(cause)));
  };
  useEffect(() => flush, []); // eslint-disable-line react-hooks/exhaustive-deps

  const change = (next: Memo, soon = false) => {
    latest.current = next;
    setMemoState(next);
    setSaved(false);
    pending.current = next;
    window.clearTimeout(timer.current);
    if (soon) flush();
    else timer.current = window.setTimeout(flush, 700);
  };
  const edit = (update: (now: Memo) => Memo, soon = false) => {
    if (latest.current) change(update(latest.current), soon);
  };
  const setNote = (id: string, patch: (note: Note) => Partial<Note>, soon = false) =>
    edit((now) => ({ ...now, memos: now.memos.map((one) => (one.id === id ? { ...one, ...patch(one) } : one)) }), soon);
  const setTodo = (id: string, patch: (todo: Todo) => Partial<Todo>) =>
    edit((now) => ({ ...now, todos: now.todos.map((one) => (one.id === id ? { ...one, ...patch(one) } : one)) }), true);

  /** Saves each picture and hands its name to `place` as soon as it is stored. */
  const upload = async (files: File[], place: (name: string) => void) => {
    const pictures = files.filter((one) => PICTURE.test(one.type));
    setAdding((was) => was + pictures.length);
    for (const file of pictures) {
      try {
        place(await commands.addMemoImage(serverId, file.type.split("/")[1].replace("jpeg", "jpg"), await base64Of(file)));
      } catch (cause) {
        onError(String(cause));
      } finally {
        setAdding((was) => was - 1);
      }
    }
  };
  const forget = (list: string[]) => {
    for (const name of list) void commands.deleteMemoImage(serverId, name).catch((cause) => onError(String(cause)));
  };
  const askRemove = async (name: string, drop: () => void) => {
    if (!(await confirm(tr("Delete picture?"), tr("This picture is removed from the memo."), tr("Delete"), true))) return;
    drop();
    forget([name]);
  };
  const noteImages = (id: string) => ({
    add: (files: File[]) => void upload(files, (name) => setNote(id, (note) => ({ images: [...note.images, name] }), true)),
    remove: (name: string) => void askRemove(name, () => setNote(id, (note) => ({ images: note.images.filter((one) => one !== name) }), true)),
  });
  const todoImages = (id: string) => ({
    add: (files: File[]) => void upload(files, (name) => setTodo(id, (todo) => ({ images: [...todo.images, name] }))),
    remove: (name: string) => void askRemove(name, () => setTodo(id, (todo) => ({ images: todo.images.filter((one) => one !== name) }))),
  });
  const removeNote = async (note: Note) => {
    const empty = !note.title.trim() && !note.text.trim() && !note.images.length;
    if (!empty && !(await confirm(tr("Delete memo?"), tr("This memo and its pictures are deleted."), tr("Delete"), true))) return;
    edit((now) => ({ ...now, memos: now.memos.filter((one) => one.id !== note.id) }), true);
    forget(note.images);
  };
  const removeTodos = (gone: Todo[]) => {
    const ids = new Set(gone.map((one) => one.id));
    edit((now) => ({ ...now, todos: now.todos.filter((one) => !ids.has(one.id)) }), true);
    forget(gone.flatMap((one) => one.images));
  };

  if (!memo) return <div className="tool-content memo-page" />;
  const open = memo.todos.filter((one) => !one.done);
  const done = memo.todos.filter((one) => one.done);
  const add = () => {
    const words = draft.trim();
    if (!words) return;
    edit((now) => ({ ...now, todos: [...now.todos, { id: newId(), text: words, done: false, status: "todo", images: [], updated: Date.now() }] }), true);
    setDraft("");
  };
  const row = (one: Todo) => {
    const pics = todoImages(one.id);
    return (
      <li key={one.id} className={one.done ? "done" : ""} onDragOver={allowDrop} onDrop={(event) => dropFiles(event, pics.add)}>
        <div className="memo-todo-line">
          <button type="button" className="memo-check" role="checkbox" aria-checked={one.done} aria-label={one.text} onClick={() => setTodo(one.id, (todo) => withStatus(todo, todo.done ? "todo" : "done"))}>
            {one.done && <Icon name="check" />}
          </button>
          {one.assignee && <span className="chip subtle memo-assignee">{personLabel(one.assignee)}</span>}
          {editing === one.id ? (
            <input
              autoFocus
              defaultValue={one.text}
              aria-label={tr("Edit")}
              onBlur={(event) => {
                const words = event.target.value.trim();
                setEditing(null);
                if (words && words !== one.text) setTodo(one.id, () => ({ text: words }));
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
                if (event.key === "Escape") setEditing(null);
              }}
              onPaste={(event) => pasteFiles(event, pics.add)}
            />
          ) : (
            <span className="memo-text" onDoubleClick={() => setEditing(one.id)}>
              {one.text}
            </span>
          )}
          <button
            type="button"
            className="wb-icon ghost"
            aria-label={tr("Add picture")}
            data-tip={tr("Add picture")}
            onClick={() => {
              todoTarget.current = one.id;
              todoPicker.current?.click();
            }}
          >
            <Icon name="image" />
          </button>
          <button type="button" className="wb-icon ghost" aria-label={tr("Edit")} data-tip={tr("Edit")} onClick={() => setEditing(one.id)}>
            <Icon name="pencil" />
          </button>
          <button type="button" className="wb-icon ghost" aria-label={tr("Delete")} data-tip={tr("Delete")} onClick={() => removeTodos([one])}>
            <Icon name="close" />
          </button>
          {people.length > 0 && todoMenu(one)}
        </div>
        <Pictures serverId={serverId} list={one.images} small onAdd={pics.add} onRemove={pics.remove} />
      </li>
    );
  };

  const todoMenu = (one: Todo) => (
    <DropdownMenu
      trigger={
        <button type="button" className="wb-icon ghost" aria-label={tr("More")} data-tip={tr("More")}>
          <Icon name="dots" />
        </button>
      }
      items={[
        ...COLUMNS.filter((column) => column !== one.status).map((column) => ({
          label: tr("Move to {0}", [columnLabel(column)]),
          icon: column === "done" ? "check" : "arrow",
          onSelect: () => setTodo(one.id, (todo) => withStatus(todo, column)),
        })),
        ...people
          .filter((person) => person !== one.assignee)
          .map((person) => ({ label: tr("Give to {0}", [personLabel(person)]), icon: "users", onSelect: () => setTodo(one.id, () => ({ assignee: person, updated: Date.now() })) })),
        ...(one.assignee ? [{ label: tr("Give to no one"), icon: "close", onSelect: () => setTodo(one.id, () => ({ assignee: "", updated: Date.now() })) }] : []),
        { label: tr("Delete"), icon: "trash", onSelect: () => removeTodos([one]) },
      ]}
    />
  );
  const board = (
    <div className="memo-board" role="list">
      {COLUMNS.map((column) => {
        const cards = memo.todos.filter((one) => one.status === column);
        return (
          <section
            key={column}
            className={`memo-column ${dragged ? "dropping" : ""}`}
            aria-label={columnLabel(column)}
            onDragOver={(event) => {
              if (dragged) event.preventDefault();
            }}
            onDrop={(event) => {
              event.preventDefault();
              if (dragged) setTodo(dragged, (todo) => withStatus(todo, column));
              setDragged(null);
            }}
          >
            <header>
              <strong>{columnLabel(column)}</strong>
              <span className="memo-count">{cards.length}</span>
            </header>
            {cards.map((one) => (
              <article
                key={one.id}
                className="memo-card"
                role="listitem"
                draggable
                onDragStart={() => setDragged(one.id)}
                onDragEnd={() => setDragged(null)}
              >
                <p>{one.text}</p>
                <footer>
                  {one.assignee && <span className="chip subtle">{personLabel(one.assignee)}</span>}
                  {one.by && <small className="muted">{tr("by {0}", [personLabel(one.by)])}</small>}
                  {todoMenu(one)}
                </footer>
              </article>
            ))}
          </section>
        );
      })}
    </div>
  );

  const todoPane = (
    <section className="memo-pane memo-todo-pane" aria-label={tr("To-do")}>
      <header className="memo-pane-head">
        <div className="memo-pane-title">
          <h2>{tr("To-do")}</h2>
          <span className="memo-count">{open.length}</span>
        </div>
        <SegmentedControl
          value={view}
          onChange={chooseView}
          options={[
            { value: "list", label: tr("List") },
            { value: "board", label: tr("Board") },
          ]}
        />
      </header>
      <form
        className="memo-add"
        onSubmit={(event) => {
          event.preventDefault();
          add();
        }}
      >
        <input value={draft} placeholder={tr("Add a to-do")} aria-label={tr("Add a to-do")} onChange={(event) => setDraft(event.target.value)} />
        <button type="submit" className="button primary small" disabled={!draft.trim()}>
          <Icon name="plus" />
          {tr("Add")}
        </button>
      </form>
      {view === "board" ? (
        board
      ) : (
        <>
          {open.length ? (
            <ul className="memo-todos">{open.map(row)}</ul>
          ) : (
            <div className="memo-blank">
              <Icon name="check" />
              <p>{tr("Nothing to do yet.")}</p>
            </div>
          )}
          {done.length > 0 && (
            <details className="memo-done">
              <summary>
                <Icon name="chevron" />
                {tr("Completed")} <span className="memo-count">{done.length}</span>
              </summary>
              <ul className="memo-todos">{done.map(row)}</ul>
              <button type="button" className="button ghost small" onClick={() => removeTodos(done)}>
                <Icon name="trash" />
                {tr("Delete completed")}
              </button>
            </details>
          )}
        </>
      )}
      <input
        ref={todoPicker}
        type="file"
        accept={ACCEPT}
        multiple
        hidden
        onChange={(event) => {
          const id = todoTarget.current;
          if (id) todoImages(id).add([...(event.target.files ?? [])]);
          event.target.value = "";
        }}
      />
    </section>
  );

  const note = memo.memos.find((one) => one.id === picked);
  const shownPane = picked === "rules" && extra ? "rules" : note ? "note" : "todo";
  const notePane = (one: Note) => {
    const pics = noteImages(one.id);
    return (
      <section key={one.id} className="memo-pane memo-note" aria-label={one.title || tr("Untitled")} onDragOver={allowDrop} onDrop={(event) => dropFiles(event, pics.add)}>
        <header className="memo-note-head">
          <input
            className="memo-note-title"
            value={one.title}
            placeholder={tr("Untitled")}
            aria-label={tr("Title")}
            onChange={(event) => setNote(one.id, () => ({ title: event.target.value }))}
            onBlur={flush}
          />
          <button type="button" className="wb-icon ghost" aria-label={tr("Delete memo")} data-tip={tr("Delete memo")} onClick={() => void removeNote(one)}>
            <Icon name="trash" />
          </button>
        </header>
        <textarea
          className="memo-notes"
          value={one.text}
          placeholder={tr("Anything to remember about this server. Paste or drop pictures here.")}
          aria-label={one.title || tr("Notes")}
          onChange={(event) => setNote(one.id, () => ({ text: event.target.value }))}
          onBlur={flush}
          onPaste={(event) => pasteFiles(event, pics.add)}
        />
        <Pictures serverId={serverId} list={one.images} onAdd={pics.add} onRemove={pics.remove} />
      </section>
    );
  };
  const addNote = () => {
    const fresh: Note = { id: newId(), title: "", text: "", images: [] };
    edit((now) => ({ ...now, memos: [fresh, ...now.memos] }), true);
    setPicked(fresh.id);
  };
  const firstLine = (one: Note) => one.text.split("\n").find((line) => line.trim())?.trim() ?? "";

  return (
    <div className="tool-content memo-page">
      <div className="page-body">
        <div className="memo-shell">
          <nav className="memo-rail" aria-label={tr("Notes")}>
            <div className="memo-rail-group">
              <button type="button" className={"memo-rail-item" + (shownPane === "todo" ? " on" : "")} aria-current={shownPane === "todo" ? "page" : undefined} onClick={() => setPicked("todo")}>
                <Icon name="check" />
                <span className="memo-rail-name">{tr("To-do")}</span>
                {open.length > 0 && <span className="memo-count">{open.length}</span>}
              </button>
              {extra && (
                <button type="button" className={"memo-rail-item" + (shownPane === "rules" ? " on" : "")} aria-current={shownPane === "rules" ? "page" : undefined} onClick={() => setPicked("rules")}>
                  <Icon name="users" />
                  <span className="memo-rail-name">{tr("Team rules (AGENTS.md)")}</span>
                </button>
              )}
            </div>
            <div className="memo-rail-head">
              <span>{tr("Memos")}</span>
              <button type="button" className="wb-icon ghost" aria-label={tr("New memo")} data-tip={tr("New memo")} onClick={addNote}>
                <Icon name="plus" />
              </button>
            </div>
            <div className="memo-rail-group memo-rail-notes">
              {memo.memos.length === 0 && <p className="memo-rail-empty">{tr("No memos yet. Each memo can have its own pictures.")}</p>}
              {memo.memos.map((one) => (
                <button
                  key={one.id}
                  type="button"
                  className={"memo-rail-item memo-rail-note" + (shownPane === "note" && one.id === picked ? " on" : "")}
                  aria-current={shownPane === "note" && one.id === picked ? "page" : undefined}
                  onClick={() => setPicked(one.id)}
                >
                  <Icon name="memo" />
                  <span className="memo-rail-text">
                    <span className={"memo-rail-name" + (one.title.trim() ? "" : " untitled")}>{one.title.trim() || tr("Untitled")}</span>
                    {firstLine(one) && <span className="memo-rail-line">{firstLine(one)}</span>}
                  </span>
                  {one.images.length > 0 && <Icon name="image" />}
                </button>
              ))}
            </div>
            <div className="memo-rail-foot">
              <button type="button" className="button secondary small" onClick={addNote}>
                <Icon name="plus" />
                {tr("New memo")}
              </button>
              <span className="memo-state" aria-live="polite">
                {adding ? tr("Adding pictures…") : saved ? tr("Saved") : tr("Saving…")}
              </span>
            </div>
          </nav>
          <div className="memo-main">
            {shownPane === "todo" && todoPane}
            {shownPane === "note" && note && notePane(note)}
            {shownPane === "rules" && <div className="memo-rules">{extra}</div>}
          </div>
        </div>
      </div>
    </div>
  );
}
