import { type CSSProperties, type DragEvent, type KeyboardEvent, type ReactNode, useState } from "react";
import { tr } from "../../lib/i18n";
import { DropdownMenu, SegmentedControl, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { listOf, StepList } from "./StudioActions";
import { CanvasView } from "./StudioCanvas";
import { StudioCanvasEditor } from "./StudioCanvasEditor";
import { StudioServer } from "./StudioItemFields";
import { ItemField } from "./StudioItemPicker";
import { ItemCell, linesOf, MiniText, textOf, type StudioData } from "./StudioPreview";
import { SCALE, useArt } from "./gameArt";
import { useStudioGlyphs } from "./studioGlyphs";
import { objectOf, type StudioNames } from "./studioKinds";
import "./StudioDialog.css";

/**
 * The built-in dialog editor (Minecraft 1.21.6+ dialog screens). Three panes over the same JSON the add-ons read:
 * an outline of the dialog's parts (drag to reorder), a stage that draws the dialog the way the game does (click a
 * part to pick it), and an inspector for the picked part. A canvas dialog swaps the stage for the canvas editor
 * while the canvas is picked. The keys are the ones StudioDialogView.java (Paper) and the Pumpkin add-on read.
 */

type Group = "body" | "inputs" | "buttons";
type Picked = { part: "dialog" } | { part: "title" } | { part: "canvas" } | { part: "exit" } | { part: Group; at: number };

const INPUT_TYPES = ["text", "bool", "number", "choice"] as const;
const KEY = /^[a-z0-9_]{1,32}$/;
const CHECK_ON = "gui/sprites/widget/checkbox_selected.png";
const CHECK_OFF = "gui/sprites/widget/checkbox.png";
const FIELD = "gui/sprites/widget/text_field.png";
const BUTTON = "gui/sprites/widget/button.png";

/** Text without MiniMessage tags, for outline labels. */
function plain(text: string): string {
  return text.replace(/<[^>]*>/g, "").trim();
}

function rowsOf(data: StudioData): unknown[] {
  return Array.isArray(data.body) ? data.body : linesOf(data, "body");
}

function listIn(data: StudioData, group: Group): unknown[] {
  return group === "body" ? rowsOf(data) : listOf(data[group]);
}

function itemRow(row: unknown): StudioData | null {
  return row && typeof row === "object" && !Array.isArray(row) ? (row as StudioData) : null;
}

function exitOf(data: StudioData): StudioData | null {
  return itemRow(data.exit);
}

type Option = { id: string; label: string };

function optionsOf(input: StudioData): Option[] {
  return (Array.isArray(input.options) ? input.options : []).map((one) =>
    typeof one === "string" ? { id: one, label: "" } : { id: textOf(objectOf(one), "id"), label: textOf(objectOf(one), "label") },
  );
}

/** An option goes back as a plain id unless it has a label of its own. */
function optionsBack(options: Option[]): unknown[] {
  return options.map((one) => (one.label && one.label !== one.id ? { id: one.id, label: one.label } : one.id));
}

/** The same checks the add-ons make when they load the file, so a problem shows on its part before saving. */
function problemsOf(data: StudioData): Map<string, string> {
  const out = new Map<string, string>();
  const seen = new Set<string>();
  listOf(data.inputs).forEach((input, at) => {
    const key = textOf(input, "key");
    if (!KEY.test(key)) out.set("inputs/" + at, tr("Use only a-z, 0-9 and _ in the key (32 at most)."));
    else if (seen.has(key)) out.set("inputs/" + at, tr("Another input already uses the key {0}.", [key]));
    else if ((textOf(input, "type") || "text") === "choice" && optionsOf(input).every((one) => !one.id.trim())) out.set("inputs/" + at, tr("A choice needs at least one option."));
    else if (!(INPUT_TYPES as readonly string[]).includes(textOf(input, "type") || "text")) out.set("inputs/" + at, tr("The kind must be text, bool, number or choice."));
    seen.add(key);
  });
  const badLink = (button: StudioData) => {
    const url = textOf(button, "url").trim();
    return url !== "" && !url.startsWith("https://") && !url.startsWith("http://");
  };
  listOf(data.buttons).forEach((button, at) => {
    if (badLink(button)) out.set("buttons/" + at, tr("The link must start with https:// or http://."));
  });
  const exit = exitOf(data);
  if (exit && badLink(exit)) out.set("exit", tr("The link must start with https:// or http://."));
  return out;
}

function keyOf(picked: Picked): string {
  return "at" in picked ? picked.part + "/" + picked.at : picked.part;
}

function same(a: Picked, b: Picked): boolean {
  return keyOf(a) === keyOf(b);
}

export function StudioDialogEditor({ data, onChange, names, serverId }: { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames; serverId?: string }) {
  const [picked, setPicked] = useState<Picked>({ part: "title" });
  const canvas = textOf(data, "layout") === "canvas";
  const problems = problemsOf(data);

  // A pick that no longer exists (removed, undone, another layout) falls back to the title.
  const exists = (one: Picked): boolean => {
    if ("at" in one) return one.at < listIn(data, one.part).length && !(canvas && one.part === "body");
    if (one.part === "exit") return exitOf(data) !== null;
    if (one.part === "canvas") return canvas;
    return true;
  };
  const shown: Picked = exists(picked) ? picked : { part: "title" };

  const setList = (group: Group, next: unknown[]) => onChange({ ...data, [group]: next });
  const setLayout = (value: string) => {
    const next = { ...data };
    if (value === "canvas") {
      next.layout = "canvas";
      if (!next.canvas || typeof next.canvas !== "object") next.canvas = { width: 176, height: 166, close: true, elements: [] };
    } else {
      delete next.layout;
    }
    onChange(next);
    setPicked(value === "canvas" ? { part: "canvas" } : { part: "title" });
  };
  const add = (group: Group, row: unknown) => {
    const list = listIn(data, group);
    setList(group, [...list, row]);
    setPicked({ part: group, at: list.length });
  };
  const move = (group: Group, from: number, to: number) => {
    const list = [...listIn(data, group)];
    if (from === to || to < 0 || to >= list.length) return;
    const [row] = list.splice(from, 1);
    list.splice(to, 0, row);
    setList(group, list);
    setPicked({ part: group, at: to });
  };
  const remove = (group: Group, at: number) => {
    const list = listIn(data, group);
    setList(group, list.filter((_, index) => index !== at));
    setPicked(list.length > 1 ? { part: group, at: Math.max(0, Math.min(at, list.length - 2)) } : { part: "title" });
  };
  const copy = (group: Group, at: number) => {
    const list = listIn(data, group);
    const row = structuredClone(list[at]);
    const one = itemRow(row);
    // A copied input needs its own key.
    if (group === "inputs" && one) {
      const keys = new Set(listOf(data.inputs).map((input) => textOf(input, "key")));
      const base = textOf(one, "key") || "input";
      let n = 2;
      while (keys.has(base + "_" + n)) n += 1;
      one.key = base + "_" + n;
    }
    setList(group, [...list.slice(0, at + 1), row, ...list.slice(at + 1)]);
    setPicked({ part: group, at: at + 1 });
  };
  const setExit = (on: boolean) => {
    const next = { ...data };
    if (on) next.exit = { label: tr("Close"), actions: [] };
    else delete next.exit;
    onChange(next);
    setPicked(on ? { part: "exit" } : { part: "title" });
  };
  const tools: OutlineTools = { picked: shown, pick: setPicked, problems, move, remove, copy };

  return (
    <StudioServer.Provider value={serverId}>
      <div className="sdlg">
        <div className="sdlg-bar">
          <SegmentedControl
            value={canvas ? "canvas" : "list"}
            onChange={setLayout}
            options={[
              { value: "list", label: tr("Lines and items") },
              { value: "canvas", label: tr("Canvas") },
            ]}
          />
          <span className="sdlg-bar-note">{canvas ? tr("Pictures and text anywhere, drawn with glyphs. Players need the studio pack.") : tr("The game's own layout, one part under another.")}</span>
          <span className="sdlg-bar-gap" />
          {problems.size > 0 && (
            <span className="sdlg-bar-problems">
              <Icon name="alert" />
              {problems.size === 1 ? tr("1 problem") : tr("{0} problems", [problems.size])}
            </span>
          )}
        </div>
        <div className={"sdlg-grid" + (shown.part === "canvas" ? " canvas" : "")}>
          <nav className="sdlg-outline" aria-label={tr("Dialog parts")}>
            <OutlineRow icon="settings" label={tr("Settings")} hint={textOf(data, "permission") || undefined} self={{ part: "dialog" }} tools={tools} />
            <OutlineRow icon="tag" label={plain(textOf(data, "title")) || tr("Title")} hint={tr("Title")} self={{ part: "title" }} tools={tools} />

            <OutlineHead label={canvas ? tr("Canvas") : tr("Body")}>
              {!canvas && (
                <DropdownMenu
                  align="end"
                  trigger={<button type="button" className="icon-button sdlg-add" aria-label={tr("Add to the body")} data-tip={tr("Add to the body")}><Icon name="plus" /></button>}
                  items={[
                    { label: tr("Text line"), icon: "list", onSelect: () => add("body", "") },
                    { label: tr("Item with words"), icon: "package", onSelect: () => add("body", { item: "diamond", count: 1, text: "" }) },
                  ]}
                />
              )}
            </OutlineHead>
            {canvas ? (
              <OutlineRow icon="image" label={tr("Canvas")} hint={tr("{0} parts", [listOf(objectOf(data.canvas).elements).length])} self={{ part: "canvas" }} tools={tools} />
            ) : (
              <OutlineGroup group="body" rows={rowsOf(data)} tools={tools} empty={tr("No text: just the title.")} describe={(row) => {
                const item = itemRow(row);
                return item
                  ? { icon: "package", label: plain(textOf(item, "text")) || textOf(item, "item") || tr("Item"), hint: textOf(item, "item") }
                  : { icon: "list", label: plain(typeof row === "string" ? row : "") || tr("Empty line"), hint: undefined };
              }} />
            )}

            <OutlineHead label={tr("Inputs")}>
              <DropdownMenu
                align="end"
                trigger={<button type="button" className="icon-button sdlg-add" aria-label={tr("Add input")} data-tip={tr("Add input")}><Icon name="plus" /></button>}
                items={INPUT_TYPES.map((type) => ({ label: type, hint: inputHint(type), onSelect: () => add("inputs", newInput(type, listOf(data.inputs))) }))}
              />
            </OutlineHead>
            <OutlineGroup group="inputs" rows={listOf(data.inputs)} tools={tools} empty={tr("No inputs.")} describe={(row) => {
              const input = objectOf(row);
              return { icon: "pencil", label: plain(textOf(input, "label")) || textOf(input, "key") || tr("Input"), hint: textOf(input, "type") || "text" };
            }} />

            <OutlineHead label={tr("Buttons")}>
              <button type="button" className="icon-button sdlg-add" aria-label={tr("Add button")} data-tip={tr("Add button")} onClick={() => add("buttons", { label: tr("OK"), actions: [] })}>
                <Icon name="plus" />
              </button>
            </OutlineHead>
            <OutlineGroup group="buttons" rows={listOf(data.buttons)} tools={tools} empty={canvas ? tr("None: only the canvas shows.") : tr("None: the game adds OK.")} describe={(row) => {
              const button = objectOf(row);
              return { icon: textOf(button, "url").trim() ? "link" : "check", label: plain(textOf(button, "label")) || tr("Button"), hint: undefined };
            }} />
            {exitOf(data) ? (
              <OutlineRow icon="exit" label={plain(textOf(exitOf(data) ?? {}, "label")) || tr("Exit button")} hint={tr("Exit")} self={{ part: "exit" }} tools={tools} />
            ) : (
              <button type="button" className="sdlg-ghost" onClick={() => setExit(true)}>
                <Icon name="plus" />
                {tr("Exit button")}
              </button>
            )}
          </nav>

          {shown.part === "canvas" ? (
            <CanvasPane data={data} onChange={onChange} names={names} serverId={serverId} onBack={() => setPicked({ part: "title" })} />
          ) : (
            <Stage data={data} picked={shown} pick={setPicked} problems={problems} serverId={serverId} />
          )}

          {shown.part !== "canvas" && (
            <aside className="sdlg-inspector" aria-label={tr("Inspector")}>
              <Inspector data={data} onChange={onChange} names={names} picked={shown} problems={problems} tools={tools} setExit={setExit} />
            </aside>
          )}
        </div>
      </div>
    </StudioServer.Provider>
  );
}

function inputHint(type: string): string {
  switch (type) {
    case "bool":
      return tr("A box to tick");
    case "number":
      return tr("A slider");
    case "choice":
      return tr("Pick one of a list");
    default:
      return tr("Typed text");
  }
}

function newInput(type: string, inputs: StudioData[]): StudioData {
  const keys = new Set(inputs.map((one) => textOf(one, "key")));
  let n = inputs.length + 1;
  while (keys.has("input" + n)) n += 1;
  const base: StudioData = { type, key: "input" + n, label: tr("Answer") };
  if (type === "choice") base.options = ["one", "two"];
  if (type === "number") Object.assign(base, { min: 0, max: 10, step: 1 });
  return base;
}

/* ---------- Outline ---------- */

type OutlineTools = {
  picked: Picked;
  pick: (next: Picked) => void;
  problems: Map<string, string>;
  move: (group: Group, from: number, to: number) => void;
  remove: (group: Group, at: number) => void;
  copy: (group: Group, at: number) => void;
};

function OutlineHead({ label, children }: { label: string; children?: ReactNode }) {
  return (
    <div className="sdlg-outline-head">
      <span>{label}</span>
      {children}
    </div>
  );
}

function OutlineRow({ icon, label, hint, self, tools }: { icon: string; label: string; hint?: string; self: Picked; tools: OutlineTools }) {
  const problem = tools.problems.get(keyOf(self));
  return (
    <button type="button" className={"sdlg-row" + (same(tools.picked, self) ? " picked" : "")} aria-pressed={same(tools.picked, self)} onClick={() => tools.pick(self)}>
      <Icon name={icon} />
      <span className="sdlg-row-label">{label}</span>
      {problem ? <span className="sdlg-row-problem" data-tip={problem} aria-label={problem}><Icon name="alert" /></span> : hint && <small>{hint}</small>}
    </button>
  );
}

/**
 * One list of the outline. Rows drag to a new place in the same list (a line shows where it lands); Alt+Up and
 * Alt+Down move the focused row for the keyboard.
 */
function OutlineGroup({ group, rows, tools, empty, describe }: { group: Group; rows: unknown[]; tools: OutlineTools; empty: string; describe: (row: unknown) => { icon: string; label: string; hint?: string } }) {
  const [drag, setDrag] = useState<number | null>(null);
  const [over, setOver] = useState<{ at: number; after: boolean } | null>(null);
  if (rows.length === 0) return <p className="sdlg-outline-empty">{empty}</p>;
  const land = (at: number, after: boolean) => {
    if (drag === null) return;
    let to = after ? at + 1 : at;
    if (drag < to) to -= 1;
    tools.move(group, drag, to);
  };
  return (
    <div className="sdlg-group" role="list">
      {rows.map((row, at) => {
        const self: Picked = { part: group, at };
        const shown = describe(row);
        const problem = tools.problems.get(keyOf(self));
        const onDragOver = (event: DragEvent<HTMLDivElement>) => {
          if (drag === null) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          const box = event.currentTarget.getBoundingClientRect();
          setOver({ at, after: event.clientY > box.top + box.height / 2 });
        };
        const onKey = (event: KeyboardEvent<HTMLButtonElement>) => {
          if (!event.altKey) return;
          if (event.key === "ArrowUp" && at > 0) {
            event.preventDefault();
            tools.move(group, at, at - 1);
          } else if (event.key === "ArrowDown" && at < rows.length - 1) {
            event.preventDefault();
            tools.move(group, at, at + 1);
          }
        };
        const mark = over && over.at === at && drag !== null ? (over.after ? " drop-after" : " drop-before") : "";
        return (
          <div
            key={at}
            role="listitem"
            className={"sdlg-drag" + (drag === at ? " dragging" : "") + mark}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", group + "/" + at);
              setDrag(at);
            }}
            onDragOver={onDragOver}
            onDragLeave={() => setOver((now) => (now && now.at === at ? null : now))}
            onDrop={(event) => {
              event.preventDefault();
              if (over) land(over.at, over.after);
              setDrag(null);
              setOver(null);
            }}
            onDragEnd={() => {
              setDrag(null);
              setOver(null);
            }}
          >
            <button
              type="button"
              className={"sdlg-row" + (same(tools.picked, self) ? " picked" : "")}
              aria-pressed={same(tools.picked, self)}
              aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
              onClick={() => tools.pick(self)}
              onKeyDown={onKey}
            >
              <span className="sdlg-grip" aria-hidden="true" />
              <Icon name={shown.icon} />
              <span className="sdlg-row-label">{shown.label}</span>
              {problem ? <span className="sdlg-row-problem" data-tip={problem} aria-label={problem}><Icon name="alert" /></span> : shown.hint && <small>{shown.hint}</small>}
            </button>
          </div>
        );
      })}
    </div>
  );
}

/* ---------- Stage ---------- */

/** The dialog as the game draws it; every part is a hit area that picks it. */
function Stage({ data, picked, pick, problems, serverId }: { data: StudioData; picked: Picked; pick: (next: Picked) => void; problems: Map<string, string>; serverId?: string }) {
  const { files } = useArt();
  const { glyphs } = useStudioGlyphs(serverId);
  const canvas = textOf(data, "layout") === "canvas";
  const buttons = listOf(data.buttons);
  const exit = exitOf(data);
  const columns = typeof data.columns === "number" ? Math.min(4, Math.max(1, Math.round(data.columns))) : 2;
  const look: CSSProperties | undefined = files[BUTTON] ? { borderImage: "url(" + files[BUTTON] + ") 3 fill / " + 3 * SCALE + "px", borderStyle: "solid", borderWidth: 3 * SCALE, background: "none" } : undefined;
  const field: CSSProperties = files[FIELD] ? { borderImage: "url(" + files[FIELD] + ") 1 fill / " + SCALE + "px", borderStyle: "solid", borderWidth: SCALE, background: "none" } : {};
  const hit = (self: Picked, className: string, children: ReactNode, style?: CSSProperties) => (
    <div
      role="button"
      tabIndex={-1}
      className={"sdlg-hit " + className + (same(picked, self) ? " picked" : "") + (problems.has(keyOf(self)) ? " problem" : "")}
      style={style}
      onClick={(event) => {
        event.stopPropagation();
        pick(self);
      }}
    >
      {children}
    </div>
  );
  return (
    <div className="sdlg-stage" onClick={() => pick({ part: "dialog" })}>
      <div className="sdlg-stage-label">
        <span>{tr("Preview")}</span>
        <small>{tr("Click a part to edit it")}</small>
      </div>
      <div className="sdlg-screen mc-scene">
        <div className={"mc-dialog sdlg-dialog" + (canvas ? " canvas" : "")}>
          {hit({ part: "title" }, "mc-dialog-title", <MiniText text={textOf(data, "title") || " "} />)}
          {canvas && hit({ part: "canvas" }, "mc-dialog-canvas", <CanvasView canvas={objectOf(data.canvas)} glyphs={glyphs} renderText={(text, shadow) => <MiniText text={text} shadow={shadow} />} />)}
          {!canvas &&
            rowsOf(data).map((row, at) => {
              const item = itemRow(row);
              return item
                ? hit(
                    { part: "body", at },
                    "mc-dialog-item",
                    <>
                      <ItemCell name={textOf(item, "item")} amount={typeof item.count === "number" ? item.count : 1} />
                      <span className="mc-inline">
                        <MiniText text={textOf(item, "text")} />
                      </span>
                    </>,
                  )
                : hit({ part: "body", at }, "mc-dialog-text", <MiniText text={typeof row === "string" && row ? row : " "} />);
            })}
          {listOf(data.inputs).map((input, at) => {
            const self: Picked = { part: "inputs", at };
            const type = textOf(input, "type") || "text";
            const label = (
              <span className="mc-inline">
                <MiniText text={textOf(input, "label") || textOf(input, "key")} />
              </span>
            );
            if (type === "bool") {
              const art = files[input.initial === true ? CHECK_ON : CHECK_OFF];
              return hit(self, "mc-dialog-check", <>{art ? <img className="mc-dialog-box-art" src={art} alt="" /> : <span className={"mc-dialog-box" + (input.initial === true ? " on" : "")} />}{label}</>);
            }
            if (type === "choice") {
              const options = optionsOf(input).filter((one) => one.id.trim());
              const first = options.find((one) => one.id === textOf(input, "initial")) ?? options[0];
              return hit(self, "mc-dialog-button wide", <>{label}<span className="mc-inline"><MiniText text={": " + (first ? first.label || first.id : "")} /></span></>, look);
            }
            if (type === "number") {
              const least = typeof input.min === "number" ? input.min : 0;
              const start = typeof input.initial === "number" ? input.initial : least;
              return hit(self, "mc-dialog-button wide", <>{label}<span className="mc-inline"><MiniText text={": " + start} /></span></>, look);
            }
            const rows = typeof input.lines === "number" ? Math.min(20, Math.max(1, Math.round(input.lines))) : 1;
            return hit(
              self,
              "mc-dialog-field",
              <>
                <span>{label}</span>
                <span className="mc-dialog-input" style={{ ...(rows > 1 ? { minHeight: 18 + (rows - 1) * 10 } : {}), ...field }}>
                  {textOf(input, "initial")}
                </span>
              </>,
            );
          })}
          {(buttons.length > 0 || (!exit && !canvas)) && (
            <div className="mc-dialog-buttons" style={{ gridTemplateColumns: "repeat(" + (buttons.length > 1 || exit ? columns : 1) + ", minmax(0, 1fr))" }}>
              {buttons.length
                ? buttons.map((button, at) => <div key={at} className="sdlg-cell">{hit({ part: "buttons", at }, "mc-dialog-button", <MiniText text={textOf(button, "label") || "OK"} />, look)}</div>)
                : <div className="mc-dialog-button sdlg-auto" style={look}><MiniText text="OK" /></div>}
            </div>
          )}
          {exit && hit({ part: "exit" }, "mc-dialog-button mc-dialog-exit", <MiniText text={textOf(exit, "label") || "OK"} />, look)}
        </div>
      </div>
      <small className="sdlg-stage-note">{tr("Placeholders use sample values: {0}", ["Steve, 12/20, 20.0 TPS"])}</small>
    </div>
  );
}

/** The canvas editor takes the whole editor while the canvas is picked; its back button returns to the outline. */
function CanvasPane({ data, onChange, names, serverId, onBack }: { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames; serverId?: string; onBack: () => void }) {
  return (
    <div className="sdlg-canvas">
      <StudioCanvasEditor dialog={data} onChange={onChange} names={names} serverId={serverId} onBack={onBack} />
    </div>
  );
}

/* ---------- Inspector ---------- */

function TextField({ label, value, onChange, placeholder, hint, many = false }: { label: string; value: string; onChange: (next: string) => void; placeholder?: string; hint?: ReactNode; many?: boolean }) {
  return (
    <label className="field studio-field">
      <span>{label}</span>
      {many ? (
        <textarea rows={Math.min(Math.max(value.split("\n").length, 2), 8)} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} spellCheck={false} />
      ) : (
        <input value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} spellCheck={false} />
      )}
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** A number box; blank removes the key when optional (the add-on then uses its own default). */
function NumberField({ label, data, name, fallback, least, most, whole = false, optional = false, onChange }: { label: string; data: StudioData; name: string; fallback: number; least?: number; most?: number; whole?: boolean; optional?: boolean; onChange: (next: StudioData) => void }) {
  const value = typeof data[name] === "number" ? (data[name] as number) : optional ? "" : fallback;
  return (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        max={most}
        step={whole ? 1 : "any"}
        value={value}
        placeholder={optional ? String(fallback) : undefined}
        onChange={(event) => {
          const raw = event.target.value.trim();
          if (raw === "") {
            if (optional) {
              const next = { ...data };
              delete next[name];
              onChange(next);
            }
            return;
          }
          let next = Number(raw);
          if (!Number.isFinite(next)) return;
          if (whole) next = Math.round(next);
          if (least !== undefined) next = Math.max(least, next);
          if (most !== undefined) next = Math.min(most, next);
          onChange({ ...data, [name]: next });
        }}
      />
    </label>
  );
}

function SwitchField({ label, checked, onChange }: { label: string; checked: boolean; onChange: (next: boolean) => void }) {
  return (
    <label className="studio-toggle">
      <span>{label}</span>
      <Switch checked={checked} onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="sdlg-section">
      <h4>{title}</h4>
      {children}
    </section>
  );
}

function Inspector({ data, onChange, names, picked, problems, tools, setExit }: { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames; picked: Picked; problems: Map<string, string>; tools: OutlineTools; setExit: (on: boolean) => void }) {
  const problem = problems.get(keyOf(picked));
  const keys = listOf(data.inputs).map((one) => textOf(one, "key")).filter((key) => KEY.test(key));
  const mini = tr("MiniMessage, like <red>, <b> and :glyph:.");

  let head: { icon: string; title: string };
  let body: ReactNode;
  if (picked.part === "dialog") {
    head = { icon: "settings", title: tr("Settings") };
    body = (
      <>
        <TextField label={tr("Permission")} value={textOf(data, "permission")} placeholder={tr("Everyone")} hint={tr("Only players with it can open the dialog.")} onChange={(permission) => onChange({ ...data, permission })} />
        <NumberField label={tr("Button columns")} data={data} name="columns" fallback={2} least={1} most={4} whole onChange={onChange} />
        <SwitchField label={tr("Esc closes it")} checked={data.canClose !== false} onChange={(on) => onChange({ ...data, canClose: on })} />
        <Section title={tr("When it opens")}>
          <StepList
            label={tr("Actions")}
            empty={tr("Nothing. Runs before the dialog shows.")}
            value={data.onOpen}
            names={names}
            onChange={(list) => {
              const next = { ...data };
              if (list.length) next.onOpen = list;
              else delete next.onOpen;
              onChange(next);
            }}
          />
        </Section>
        <p className="sdlg-note">{tr("Open it with the Open a dialog action, from a rule, command, NPC, item or menu. Needs Minecraft 1.21.6 or newer.")}</p>
      </>
    );
  } else if (picked.part === "title") {
    head = { icon: "tag", title: tr("Title") };
    body = <TextField label={tr("Title")} value={textOf(data, "title")} hint={mini} onChange={(title) => onChange({ ...data, title })} />;
  } else if (picked.part === "body") {
    const rows = rowsOf(data);
    const row = rows[picked.at];
    const item = itemRow(row);
    const setRow = (next: unknown) => onChange({ ...data, body: rows.map((one, index) => (index === picked.at ? next : one)) });
    head = { icon: item ? "package" : "list", title: item ? tr("Item with words") : tr("Text line") };
    body = (
      <>
        <SegmentedControl
          className="even"
          value={item ? "item" : "text"}
          onChange={(value) => setRow(value === "item" ? { item: "diamond", count: 1, text: typeof row === "string" ? row : "" } : textOf(item ?? {}, "text"))}
          options={[
            { value: "text", label: tr("Text") },
            { value: "item", label: tr("Item") },
          ]}
        />
        {item ? (
          <>
            <ItemField label={tr("Item")} value={textOf(item, "item")} onChange={(value) => setRow({ ...item, item: value })} />
            <NumberField label={tr("Amount")} data={item} name="count" fallback={1} least={1} most={99} whole onChange={setRow} />
            <TextField label={tr("Beside it")} value={textOf(item, "text")} hint={mini} onChange={(text) => setRow({ ...item, text })} />
          </>
        ) : (
          <TextField label={tr("Text")} value={typeof row === "string" ? row : ""} hint={mini} onChange={setRow} />
        )}
      </>
    );
  } else if (picked.part === "inputs") {
    const inputs = listOf(data.inputs);
    const input = inputs[picked.at] ?? {};
    const type = textOf(input, "type") || "text";
    const setInput = (next: StudioData) => onChange({ ...data, inputs: inputs.map((one, index) => (index === picked.at ? next : one)) });
    head = { icon: "pencil", title: tr("Input") };
    body = (
      <>
        <label className="field studio-field">
          <span>{tr("Kind")}</span>
          <Dropdown
            value={type}
            options={INPUT_TYPES.map((one) => ({ value: one, label: one, hint: inputHint(one) }))}
            onChange={(value) => {
              const next: StudioData = { ...input, type: value };
              delete next.initial;
              if (value === "choice" && !Array.isArray(next.options)) next.options = ["one", "two"];
              setInput(next);
            }}
          />
        </label>
        <TextField label={tr("Key")} value={textOf(input, "key")} hint={"<input:" + (textOf(input, "key") || "key") + ">"} onChange={(key) => setInput({ ...input, key })} />
        <TextField label={tr("Label")} value={textOf(input, "label")} hint={mini} onChange={(label) => setInput({ ...input, label })} />
        {type === "text" && (
          <>
            <TextField label={tr("Starts with")} value={textOf(input, "initial")} onChange={(initial) => setInput({ ...input, initial })} />
            <div className="sdlg-pair">
              <NumberField label={tr("Lines")} data={input} name="lines" fallback={1} least={1} most={20} whole onChange={setInput} />
              <NumberField label={tr("Most letters")} data={input} name="maxLength" fallback={32} least={1} most={2048} whole onChange={setInput} />
            </div>
          </>
        )}
        {type === "bool" && <SwitchField label={tr("Ticked at first")} checked={input.initial === true} onChange={(on) => setInput({ ...input, initial: on })} />}
        {type === "number" && (
          <>
            <div className="sdlg-pair">
              <NumberField label={tr("From")} data={input} name="min" fallback={0} onChange={setInput} />
              <NumberField label={tr("To")} data={input} name="max" fallback={10} onChange={setInput} />
            </div>
            <div className="sdlg-pair">
              <NumberField label={tr("Step")} data={input} name="step" fallback={1} least={0.01} onChange={setInput} />
              <NumberField label={tr("Starts at")} data={input} name="initial" fallback={typeof input.min === "number" ? input.min : 0} optional onChange={setInput} />
            </div>
          </>
        )}
        {type === "choice" && <ChoiceList input={input} onChange={setInput} />}
      </>
    );
  } else {
    const isExit = picked.part === "exit";
    const index = "at" in picked ? picked.at : 0;
    const buttons = listOf(data.buttons);
    const button = isExit ? (exitOf(data) ?? {}) : (buttons[index] ?? {});
    const setButton = (next: StudioData) => onChange(isExit ? { ...data, exit: next } : { ...data, buttons: buttons.map((one, at) => (at === index ? next : one)) });
    const link = textOf(button, "url").trim() !== "";
    head = { icon: isExit ? "exit" : "check", title: isExit ? tr("Exit button") : tr("Button") };
    body = (
      <>
        <TextField label={tr("Label")} value={textOf(button, "label")} hint={isExit ? tr("Under the other buttons. Esc presses it too.") : mini} onChange={(label) => setButton({ ...button, label })} />
        <TextField label={tr("Tooltip")} value={textOf(button, "tooltip")} onChange={(tooltip) => setButton({ ...button, tooltip })} />
        <div className="sdlg-pair">
          <TextField label={tr("Permission")} value={textOf(button, "permission")} placeholder={tr("Everyone")} onChange={(permission) => setButton({ ...button, permission })} />
          <NumberField label={tr("Width")} data={button} name="width" fallback={150} least={1} most={1024} whole optional onChange={setButton} />
        </div>
        <TextField label={tr("Link")} value={textOf(button, "url")} placeholder="https://" hint={tr("Opens the link instead of running actions.")} onChange={(url) => setButton({ ...button, url })} />
        {!link && (
          <Section title={tr("When clicked")}>
            <StepList label={tr("Actions")} empty={tr("Clicking just closes it.")} value={button.actions} names={names} onChange={(actions) => setButton({ ...button, actions })} />
            {keys.length > 0 && (
              <p className="sdlg-note">
                {tr("What the player entered:")} {keys.map((key) => <code key={key}>{"<input:" + key + ">"}</code>)}
                <br />
                {tr("In a command, <input:key> only takes letters, numbers, _ . and -; <input:key|raw> puts in what was typed as it is.")}
              </p>
            )}
          </Section>
        )}
      </>
    );
  }

  const group: Group | null = "at" in picked ? picked.part : null;
  const at = "at" in picked ? picked.at : 0;
  const size = group ? listIn(data, group).length : 0;
  return (
    <>
      <div className="sdlg-inspector-head">
        <Icon name={head.icon} />
        <strong>{head.title}</strong>
        {group && <small>{at + 1} / {size}</small>}
        {group && (
          <span className="sdlg-inspector-tools">
            <button type="button" className="icon-button" aria-label={tr("Move up")} data-tip={tr("Move up")} disabled={at === 0} onClick={() => tools.move(group, at, at - 1)}>
              <Icon name="arrow-up" />
            </button>
            <button type="button" className="icon-button" aria-label={tr("Move down")} data-tip={tr("Move down")} disabled={at >= size - 1} onClick={() => tools.move(group, at, at + 1)}>
              <Icon name="arrow-down" />
            </button>
            <button type="button" className="icon-button" aria-label={tr("Duplicate")} data-tip={tr("Duplicate")} onClick={() => tools.copy(group, at)}>
              <Icon name="copy" />
            </button>
            <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => tools.remove(group, at)}>
              <Icon name="trash" />
            </button>
          </span>
        )}
        {picked.part === "exit" && (
          <span className="sdlg-inspector-tools">
            <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setExit(false)}>
              <Icon name="trash" />
            </button>
          </span>
        )}
      </div>
      {problem && (
        <p className="sdlg-problem">
          <Icon name="alert" />
          {problem}
        </p>
      )}
      <div className="sdlg-inspector-body">{body}</div>
    </>
  );
}

/** A choice's options: the id goes into <input:key>, the label is what the player sees; one is picked at first. */
function ChoiceList({ input, onChange }: { input: StudioData; onChange: (next: StudioData) => void }) {
  const options = optionsOf(input);
  const initial = textOf(input, "initial");
  const set = (next: Option[]) => onChange({ ...input, options: optionsBack(next) });
  const setOne = (at: number, next: Option) => set(options.map((one, index) => (index === at ? next : one)));
  return (
    <Section title={tr("Options")}>
      <div className="sdlg-options">
        <div className="sdlg-option sdlg-option-head" aria-hidden="true">
          <span>{tr("First")}</span>
          <span>{tr("ID")}</span>
          <span>{tr("Shown as")}</span>
          <span />
        </div>
        {options.map((one, at) => (
          <div key={at} className="sdlg-option">
            <input
              type="radio"
              name={"sdlg-first-" + textOf(input, "key")}
              checked={initial ? initial === one.id : at === 0}
              aria-label={tr("Picked at first")}
              onChange={() => onChange({ ...input, initial: one.id })}
            />
            <input value={one.id} aria-label={tr("ID")} spellCheck={false} onChange={(event) => setOne(at, { ...one, id: event.target.value })} />
            <input value={one.label} aria-label={tr("Shown as")} placeholder={one.id} spellCheck={false} onChange={(event) => setOne(at, { ...one, label: event.target.value })} />
            <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => set(options.filter((_, index) => index !== at))}>
              <Icon name="trash" />
            </button>
          </div>
        ))}
      </div>
      <button type="button" className="button ghost small sdlg-option-add" onClick={() => set([...options, { id: "option" + (options.length + 1), label: "" }])}>
        <Icon name="plus" />
        {tr("Add option")}
      </button>
    </Section>
  );
}
