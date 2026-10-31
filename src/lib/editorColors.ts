import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";

const LEGACY: Record<string, string> = {
  "0": "#000000", "1": "#0000AA", "2": "#00AA00", "3": "#00AAAA", "4": "#AA0000", "5": "#AA00AA", "6": "#FFAA00", "7": "#AAAAAA",
  "8": "#555555", "9": "#5555FF", a: "#55FF55", b: "#55FFFF", c: "#FF5555", d: "#FF55FF", e: "#FFFF55", f: "#FFFFFF",
};
const NAMED: Record<string, string> = {
  black: "0", dark_blue: "1", dark_green: "2", dark_aqua: "3", dark_red: "4", dark_purple: "5", gold: "6", gray: "7", grey: "7",
  dark_gray: "8", dark_grey: "8", blue: "9", green: "a", aqua: "b", red: "c", light_purple: "d", yellow: "e", white: "f",
};
// #RRGGBB(AA), 0xRRGGBB, &c / §c codes, and MiniMessage tags like <red> or <color:gold>.
export const FIND = /(?<![\w#])(?:#|0x)([0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?)(?![\w])|[&§]([0-9a-fA-F])|<(?:colou?r:)?([a-z_]+)>/g;

/** The colour a match stands for, as CSS, or null. */
export function colourOf(match: RegExpExecArray): string | null {
  if (match[1]) return `#${match[1]}`;
  if (match[2]) return LEGACY[match[2].toLowerCase()];
  const code = match[3] ? NAMED[match[3]] : undefined;
  return code ? LEGACY[code] : null;
}

class Swatch extends WidgetType {
  constructor(readonly colour: string) {
    super();
  }
  eq(other: Swatch) {
    return other.colour === this.colour;
  }
  toDOM() {
    const box = document.createElement("span");
    box.className = "cm-swatch";
    box.style.backgroundColor = this.colour;
    box.title = this.colour;
    return box;
  }
  ignoreEvent() {
    return false;
  }
}

function swatches(view: EditorView): DecorationSet {
  const out = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    const text = view.state.doc.sliceString(from, to);
    FIND.lastIndex = 0;
    for (let match = FIND.exec(text); match; match = FIND.exec(text)) {
      const colour = colourOf(match);
      if (colour) out.add(from + match.index, from + match.index, Decoration.widget({ widget: new Swatch(colour), side: -1 }));
    }
  }
  return out.finish();
}

export const colourSwatches = [
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = swatches(view);
      }
      update(update: ViewUpdate) {
        if (update.docChanged || update.viewportChanged) this.decorations = swatches(update.view);
      }
    },
    { decorations: (plugin) => plugin.decorations },
  ),
  EditorView.baseTheme({
    ".cm-swatch": {
      display: "inline-block",
      width: "0.8em",
      height: "0.8em",
      marginRight: "3px",
      verticalAlign: "-0.05em",
      borderRadius: "2px",
      boxShadow: "0 0 0 1px var(--line-strong)",
    },
  }),
];
