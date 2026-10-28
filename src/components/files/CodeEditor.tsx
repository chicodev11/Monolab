import { useEffect, useRef } from "react";
import { autocompletion, closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment } from "@codemirror/commands";
import { json } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";
import {
  bracketMatching,
  foldGutter,
  foldKeymap,
  HighlightStyle,
  indentOnInput,
  StreamLanguage,
  syntaxHighlighting,
} from "@codemirror/language";
import { properties } from "@codemirror/legacy-modes/mode/properties";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { toml } from "@codemirror/legacy-modes/mode/toml";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { languageKind, languageName } from "../../lib/fileLang";

export function languageOf(path: string): { name: string; extension: Extension | null } {
  const name = languageName(path);
  switch (languageKind(path)) {
    case "json":
      return { name, extension: json() };
    case "yaml":
      return { name, extension: yaml() };
    case "toml":
      return { name, extension: StreamLanguage.define(toml) };
    case "properties":
      return { name, extension: StreamLanguage.define(properties) };
    case "shell":
      return { name, extension: StreamLanguage.define(shell) };
    default:
      return { name, extension: null };
  }
}

const colours = HighlightStyle.define([
  { tag: [tags.propertyName, tags.attributeName, tags.definition(tags.propertyName), tags.heading], color: "var(--syn-key)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--syn-string)" },
  { tag: [tags.number, tags.integer, tags.float], color: "var(--syn-number)" },
  { tag: [tags.bool, tags.null, tags.atom, tags.keyword, tags.labelName], color: "var(--syn-flag)" },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], color: "var(--syn-comment)", fontStyle: "italic" },
  { tag: [tags.punctuation, tags.separator, tags.bracket, tags.operator], color: "var(--syn-punct)" },
  { tag: [tags.variableName, tags.meta], color: "var(--syn-var)" },
]);

const look = EditorView.theme({
  "&": { height: "100%", color: "var(--text)", backgroundColor: "var(--inset)", fontSize: "13.5px" },
  ".cm-scroller": { fontFamily: "var(--code)", lineHeight: "1.6" },
  ".cm-content": { caretColor: "var(--text)", padding: "8px 0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)", borderLeftWidth: "2px" },
  ".cm-gutters": {
    backgroundColor: "var(--inset)",
    color: "var(--text-4)",
    border: "none",
    borderRight: "1px solid var(--line-soft)",
  },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 12px 0 16px", minWidth: "44px" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text)" },
  ".cm-activeLine": { backgroundColor: "var(--editor-line)" },
  "&.cm-focused": { outline: "none" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--editor-select) !important",
  },
  ".cm-selectionMatch": { backgroundColor: "var(--editor-match)" },
  "&.cm-focused .cm-matchingBracket": { backgroundColor: "var(--editor-match)", outline: "1px solid var(--line-strong)" },
  ".cm-foldGutter .cm-gutterElement": { padding: "0 4px", color: "var(--text-4)", cursor: "pointer" },
  ".cm-foldPlaceholder": { backgroundColor: "var(--raise)", border: "1px solid var(--line)", color: "var(--text-3)" },
  ".cm-searchMatch": { backgroundColor: "var(--editor-find)", outline: "1px solid var(--warn)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--editor-find-on)" },
  ".cm-panels": { backgroundColor: "var(--panel-2)", color: "var(--text)", borderBottom: "1px solid var(--line)" },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--line)" },
  ".cm-panel.cm-search": { padding: "8px 10px", fontFamily: "var(--sans)", fontSize: "12.5px" },
  ".cm-panel.cm-search input, .cm-panel.cm-search button": { fontFamily: "var(--sans)", fontSize: "12.5px" },
  ".cm-tooltip": { backgroundColor: "var(--panel-2)", border: "1px solid var(--line)", color: "var(--text)" },
});

export type Cursor = { line: number; column: number; selected: number; lines: number };

/** Keep a file's own line breaks: CodeMirror would otherwise turn CRLF into LF, so a file looks edited the moment it opens. */
function lineBreakOf(text: string) {
  return text.includes("\r\n") ? EditorState.lineSeparator.of("\r\n") : [];
}

export function CodeEditor({
  path,
  value,
  readOnly,
  onChange,
  onSave,
  onCursor,
}: {
  path: string;
  value: string;
  readOnly: boolean;
  onChange: (text: string) => void;
  onSave: () => void;
  onCursor: (cursor: Cursor) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const handlers = useRef({ onChange, onSave, onCursor });
  handlers.current = { onChange, onSave, onCursor };
  const locking = useRef(new Compartment());
  const breaks = useRef(new Compartment());

  useEffect(() => {
    if (!host.current) return;
    const language = languageOf(path).extension;
    const tell = (state: EditorState) => {
      const head = state.selection.main.head;
      const line = state.doc.lineAt(head);
      handlers.current.onCursor({
        line: line.number,
        column: head - line.from + 1,
        selected: state.selection.ranges.reduce((sum, one) => sum + (one.to - one.from), 0),
        lines: state.doc.lines,
      });
    };
    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        foldGutter({ openText: "⌄", closedText: "›" }),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        indentOnInput(),
        syntaxHighlighting(colours, { fallback: true }),
        bracketMatching(),
        closeBrackets(),
        autocompletion({ activateOnTyping: false }),
        rectangularSelection(),
        crosshairCursor(),
        highlightActiveLine(),
        highlightSelectionMatches(),
        search({ top: true }),
        keymap.of([
          { key: "Mod-s", preventDefault: true, run: () => { handlers.current.onSave(); return true; } },
          { key: "Mod-/", run: toggleComment },
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...foldKeymap,
          indentWithTab,
        ]),
        look,
        locking.current.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
        breaks.current.of(lineBreakOf(value)),
        ...(language ? [language] : []),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) handlers.current.onChange(update.state.sliceDoc());
          if (update.docChanged || update.selectionSet) tell(update.state);
        }),
      ],
    });
    const made = new EditorView({ state, parent: host.current });
    view.current = made;
    tell(state);
    made.focus();
    return () => {
      made.destroy();
      view.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const now = view.current;
    if (!now) return;
    if (now.state.lineBreak !== (value.includes("\r\n") ? "\r\n" : "\n")) {
      now.dispatch({ effects: breaks.current.reconfigure(lineBreakOf(value)) });
    }
    if (now.state.sliceDoc() === value) return;
    now.dispatch({ changes: { from: 0, to: now.state.doc.length, insert: value } });
  }, [value]);

  useEffect(() => {
    view.current?.dispatch({
      effects: locking.current.reconfigure([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
    });
  }, [readOnly]);

  return <div className="code-editor" ref={host} />;
}
