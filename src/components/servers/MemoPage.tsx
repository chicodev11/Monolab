import { useEffect, useRef, useState } from "react";
import { commands } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Card } from "../primitives";
import { Icon } from "../ui/Icon";

type Todo = { id: string; text: string; done: boolean };
type Memo = { notes: string; todos: Todo[] };
const newId = () => `${Date.now()}-${Math.round(Math.random() * 1e6)}`;

export function parseMemo(text: string): Memo {
  try {
    const doc = JSON.parse(text) as Partial<Memo>;
    const todos = Array.isArray(doc.todos)
      ? doc.todos.filter((one) => one && typeof one.text === "string").map((one) => ({ id: String(one.id ?? newId()), text: one.text, done: one.done === true }))
      : [];
    return { notes: typeof doc.notes === "string" ? doc.notes : "", todos };
  } catch {
    return { notes: "", todos: [] };
  }
}

export function MemoPage({ serverId, onError }: { serverId: string; onError: (message: string) => void }) {
  const [memo, setMemo] = useState<Memo | null>(null);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [saved, setSaved] = useState(true);
  const pending = useRef<Memo | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    void commands.serverMemo(serverId).then((text) => setMemo(parseMemo(text)), (cause) => onError(String(cause)));
  }, [serverId, onError]);

  const flush = () => {
    window.clearTimeout(timer.current);
    const next = pending.current;
    if (!next) return;
    pending.current = null;
    void commands.setServerMemo(serverId, JSON.stringify(next)).then(() => setSaved(true), (cause) => onError(String(cause)));
  };
  useEffect(() => flush, []); // eslint-disable-line react-hooks/exhaustive-deps

  const change = (next: Memo, soon = false) => {
    setMemo(next);
    setSaved(false);
    pending.current = next;
    window.clearTimeout(timer.current);
    if (soon) flush();
    else timer.current = window.setTimeout(flush, 700);
  };

  if (!memo) return <div className="tool-content memo-page" />;
  const open = memo.todos.filter((one) => !one.done);
  const done = memo.todos.filter((one) => one.done);
  const setTodo = (id: string, patch: Partial<Todo>) => change({ ...memo, todos: memo.todos.map((one) => (one.id === id ? { ...one, ...patch } : one)) }, true);
  const add = () => {
    const text = draft.trim();
    if (!text) return;
    change({ ...memo, todos: [...memo.todos, { id: newId(), text, done: false }] }, true);
    setDraft("");
  };
  const row = (one: Todo) => (
    <li key={one.id} className={one.done ? "done" : ""}>
      <button type="button" className="memo-check" role="checkbox" aria-checked={one.done} aria-label={one.text} onClick={() => setTodo(one.id, { done: !one.done })}>
        {one.done && <Icon name="check" />}
      </button>
      {editing === one.id ? (
        <input
          autoFocus
          defaultValue={one.text}
          aria-label={tr("Edit")}
          onBlur={(event) => {
            const text = event.target.value.trim();
            setEditing(null);
            if (text && text !== one.text) setTodo(one.id, { text });
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
            if (event.key === "Escape") setEditing(null);
          }}
        />
      ) : (
        <span className="memo-text" onDoubleClick={() => setEditing(one.id)}>
          {one.text}
        </span>
      )}
      <button type="button" className="wb-icon ghost" aria-label={tr("Edit")} data-tip={tr("Edit")} onClick={() => setEditing(one.id)}>
        <Icon name="pencil" />
      </button>
      <button type="button" className="wb-icon ghost" aria-label={tr("Delete")} data-tip={tr("Delete")} onClick={() => change({ ...memo, todos: memo.todos.filter((other) => other.id !== one.id) }, true)}>
        <Icon name="close" />
      </button>
    </li>
  );

  return (
    <div className="tool-content memo-page">
      <div className="page-body">
        <div className="memo-grid">
          <Card title={tr("To-do")} aside={<span className="memo-count">{open.length}</span>}>
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
            {open.length ? <ul className="memo-todos">{open.map(row)}</ul> : <p className="muted memo-empty">{tr("Nothing to do yet.")}</p>}
            {done.length > 0 && (
              <details className="memo-done">
                <summary>
                  {tr("Completed")} <span className="memo-count">{done.length}</span>
                </summary>
                <ul className="memo-todos">{done.map(row)}</ul>
                <button type="button" className="button ghost small" onClick={() => change({ ...memo, todos: open }, true)}>
                  <Icon name="trash" />
                  {tr("Delete completed")}
                </button>
              </details>
            )}
          </Card>
          <Card title={tr("Notes")} aside={<span className="memo-state">{saved ? tr("Saved") : tr("Saving…")}</span>}>
            <textarea
              className="memo-notes"
              value={memo.notes}
              placeholder={tr("Anything to remember about this server")}
              aria-label={tr("Notes")}
              onChange={(event) => change({ ...memo, notes: event.target.value })}
              onBlur={flush}
            />
          </Card>
        </div>
      </div>
    </div>
  );
}
