import { type CSSProperties, type ReactNode, useContext, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "../ui/Icon";
import { tr } from "../../lib/i18n";
import { enchantLevel } from "../../lib/items";
import { miniMessageLines, SAMPLE_VALUES, type Run } from "../../lib/miniMessage";
import { GameArtProvider, GameText, Piece, SCALE, useArt } from "./gameArt";
import { StudioServer, useItemPicture } from "./StudioItemFields";
import { CanvasView } from "./StudioCanvas";
import { byName, glyphPieces, glyphSize, nameOf, usePicture, useStudioGlyphs } from "./studioGlyphs";

export type StudioData = Record<string, unknown>;

export function textOf(data: StudioData, key: string): string {
  const value = data[key];
  return typeof value === "string" ? value : "";
}

export function linesOf(data: StudioData, key: string): string[] {
  const value = data[key];
  if (Array.isArray(value)) return value.map((one) => (typeof one === "string" ? one : ""));
  return typeof value === "string" ? [value] : [];
}

function runStyle(run: Run, base: string): CSSProperties {
  return {
    color: run.color ?? base,
    fontWeight: run.bold ? 700 : undefined,
    fontStyle: run.italic ? "italic" : undefined,
    textDecoration: [run.underlined && "underline", run.strikethrough && "line-through"].filter(Boolean).join(" ") || undefined,
  };
}

function PlainRuns({ line, base }: { line: Run[]; base: string }) {
  return (
    <>
      {line.map((run, index) => (
        <span key={index} className={run.obfuscated ? "mc-obf" : undefined} style={runStyle(run, base)}>
          {run.text}
        </span>
      ))}
    </>
  );
}

/** One MiniMessage text drawn as game text. `base` is the color text has when no tag sets one. */
export function MiniText({ text, base = "#ffffff", values = SAMPLE_VALUES, shadow = true }: { text: string; base?: string; values?: Record<string, string>; shadow?: boolean }) {
  const lines = miniMessageLines(text, values);
  const { font } = useArt();
  const { glyphs } = useStudioGlyphs(useContext(StudioServer));
  const known = byName(glyphs);
  const cut = /:[a-z0-9_]{1,32}:/.test(text) && glyphs.length ? lines.map((line) => glyphPieces(line, known)) : null;
  if (cut?.some((line) => line.some((piece) => "glyph" in piece))) {
    return (
      <>
        {cut.map((line, at) => (
          <span key={at} className="mc-line mc-glyph-line">
            {line.map((piece, index) => {
              if ("glyph" in piece) {
                const { glyph } = piece;
                return (
                  <span key={index} className="mc-glyph" style={{ width: glyph.advance * SCALE }}>
                    <img src={glyph.url} alt="" style={{ top: (7 - glyph.ascent) * SCALE, width: glyph.width * SCALE, height: glyph.height * SCALE }} />
                  </span>
                );
              }
              return font ? (
                <GameText key={index} lines={[piece.runs]} base={base} shadow={shadow} font={font} />
              ) : (
                <span key={index}>
                  <PlainRuns line={piece.runs} base={base} />
                </span>
              );
            })}
          </span>
        ))}
      </>
    );
  }
  if (font) return <GameText lines={lines} base={base} shadow={shadow} font={font} />;
  return (
    <>
      {lines.map((line, at) => (
        <span key={at} className="mc-line">
          {line.length ? <PlainRuns line={line} base={base} /> : <span>&nbsp;</span>}
        </span>
      ))}
    </>
  );
}

const OTHERS = ["Alex", "Notch"];

/** A plain string drawn in the game's font, like MiniText but without tags. */
function Mc({ text, color = "#ffffff", shadow = true }: { text: string; color?: string; shadow?: boolean }) {
  return <MiniText text={text.replace(/</g, "\\<")} base={color} shadow={shadow} />;
}

/** `art` is the server's Minecraft version when the user said yes to the game's pictures, else null. `serverId` lets
 * an item show its studio texture. */
export function StudioPreview({ kind, data, art = null, serverId }: { kind: string; data: StudioData | null; art?: string | null; serverId?: string }) {
  if (!data) return <p className="muted studio-preview-none">{tr("Fix the JSON to see a preview.")}</p>;
  return (
    <StudioServer.Provider value={serverId}>
      <PreviewBody kind={kind} data={data} art={art} />
    </StudioServer.Provider>
  );
}

function PreviewBody({ kind, data, art }: { kind: string; data: StudioData; art: string | null }) {
  const [full, setFull] = useState(false);
  const off = data.enabled === false;
  useEffect(() => {
    if (!full) return;
    const close = (event: KeyboardEvent) => {
      if (event.key === "Escape") setFull(false);
    };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [full]);
  return (
    <GameArtProvider version={art} items={itemsIn(kind, data)}>
      <div className={`studio-preview ${off ? "off" : ""}`}>
        <span className="studio-preview-label">
          {tr("Preview")}
          {off && ` · ${tr("Off")}`}
          <button type="button" className="icon-button studio-preview-full" aria-label={tr("Full screen")} data-tip={tr("Full screen")} onClick={() => setFull(true)}>
            <Icon name="maximize" />
          </button>
        </span>
        <div className="mc-scene">
          <Fit>{scene(kind, data)}</Fit>
        </div>
        <span className="studio-preview-note">{tr("Placeholders use sample values: {0}", ["Steve, 12/20, 20.0 TPS"])}</span>
      </div>
      {full &&
        createPortal(
          // The same scene, grown to fill the window in whole game pixels. Esc or a click outside closes it.
          <div className={`studio-preview-screen ${off ? "off" : ""}`} role="dialog" aria-modal="true" aria-label={tr("Preview")} onClick={() => setFull(false)}>
            <button type="button" className="icon-button studio-preview-close" aria-label={tr("Close")} data-tip={tr("Close")} onClick={() => setFull(false)}>
              <Icon name="close" />
            </button>
            <div className="mc-scene" onClick={(event) => event.stopPropagation()}>
              <Fit grow>{scene(kind, data)}</Fit>
            </div>
          </div>,
          document.body,
        )}
    </GameArtProvider>
  );
}

/** Kinds with nothing of their own to draw get the whole width for their fields instead of an empty preview. */
export function hasPreview(kind: string): boolean {
  return !["quest", "structure", "biome", "dialog"].includes(kind);
}

/** The items a preview shows, so their icons load with it. */
export function itemsIn(kind: string, data: StudioData): string[] {
  const text = (one: StudioData, key: string) => (typeof one[key] === "string" ? (one[key] as string) : "");
  switch (kind) {
    case "menu":
      return [text(data, "fill"), ...stepsIn(data.slots).map((slot) => (typeof slot.item === "string" ? slot.item : text(objectIn(slot.item), "material")))];
    case "recipe":
      return [text(objectIn(data.result), "item"), text(data, "input"), ...Object.values(objectIn(data.keys)).filter((one): one is string => typeof one === "string"), ...linesOf(data, "ingredients")];
    case "item":
      return [text(data, "material")];
    case "loot":
      return stepsIn(data.entries).map((one) => text(one, "item"));
    case "advancement":
      return [text(data, "icon") || "diamond"];
    case "npc":
      return [`${(text(data, "entity") || "villager").replace(/^minecraft:/, "")}_spawn_egg`];
    case "mob":
      return [`${(text(data, "base") || "zombie").replace(/^minecraft:/, "")}_spawn_egg`];
    case "world":
      return stepsIn(data.layers).map((one) => text(one, "block"));
    case "cosmetic":
      return [text(data, "item")];
    case "enchant":
      return ["enchanted_book"];
    case "song":
      return ["music_disc_13"];
    case "shop":
      return stepsIn(data.goods).map((one) => text(one, "item"));
    case "trade":
      return stepsIn(data.offers).flatMap((one) => [text(one, "buy"), text(one, "buy2"), text(one, "sell")]);
    case "dialog": {
      // Canvas item icons (`"item": "diamond"`) load too, so the canvas editor can make their glyphs.
      const icons = stepsIn(objectIn(data.canvas).elements).map((one) => text(one, "item"));
      return [...(Array.isArray(data.body) ? stepsIn(data.body.filter((one) => typeof one === "object")).map((one) => text(one, "item")) : []), ...icons];
    }
    default:
      return [];
  }
}

function scene(kind: string, data: StudioData) {
  switch (kind) {
    case "scoreboard":
      return (
        <div className="mc-sidebar">
          <div className="mc-sidebar-title">
            <MiniText text={textOf(data, "title")} />
          </div>
          {linesOf(data, "lines")
            .slice(0, 15)
            .map((line, at) => (
              <div key={at} className="mc-sidebar-line">
                <MiniText text={line} />
              </div>
            ))}
        </div>
      );
    case "tab":
      return <TabScene data={data} />;
    case "motd":
      return <MotdScene data={data} />;
    case "chat":
      return (
        <div className="mc-chat">
          <div className="mc-chat-line">
            <MiniText text={textOf(data, "format") || "<<player>> <message>"} />
          </div>
        </div>
      );
    case "message": {
      const rows = [
        ["firstJoin", tr("First join")],
        ["join", tr("Join")],
        ["leave", tr("Leave")],
      ].filter(([key]) => key in data);
      return (
        <div className="mc-chat">
          {rows.map(([key, label]) => (
            <div key={key} className="mc-chat-line" title={label}>
              {textOf(data, key).trim() ? <MiniText text={textOf(data, key)} /> : <span className="mc-hidden">{tr("{0}: no message", [label])}</span>}
            </div>
          ))}
        </div>
      );
    }
    case "announcement":
      return (
        <div className="mc-chat">
          {linesOf(data, "messages")
            .filter((one) => one.trim())
            .map((line, at) => (
              <div key={at} className="mc-chat-line">
                <MiniText text={line} />
              </div>
            ))}
        </div>
      );
    case "bossbar":
      return <BossBarScene data={data} />;
    case "hologram":
      return (
        <div className={`mc-hologram ${data.background === false ? "bare" : ""} ${data.shadow === true ? "shadow" : ""}`}>
          {linesOf(data, "lines").map((line, at) => (
            <div key={at}>{line.trim() ? <MiniText text={line} /> : " "}</div>
          ))}
        </div>
      );
    case "npc":
      return <NpcScene data={data} />;
    case "mob":
      return <MobScene data={data} />;
    case "dialog":
      return <DialogScene data={data} />;
    case "loot":
      return <LootScene data={data} />;
    case "advancement":
      return <AdvancementScene data={data} />;
    case "world":
      return <WorldScene data={data} />;
    case "font":
      return <FontScene data={data} />;
    case "command":
      return <CommandScene data={data} />;
    case "menu":
      return <MenuScene data={data} />;
    case "item":
      return <ItemTooltip data={data} />;
    case "recipe":
      return <RecipeScene data={data} />;
    case "rule":
      return <RuleScene data={data} />;
    case "block":
      return <LookScene data={data} block />;
    case "furniture":
      return <LookScene data={data} block={false} />;
    case "model":
      return <p className="muted studio-preview-none">{tr("The model turns in 3D in the form, with its animations.")}</p>;
    case "function":
      return <FunctionScene data={data} />;
    case "enchant":
      return <BookTooltip icon="enchanted_book" title="Enchanted Book" color="#ffff55" lines={[`${textOf(data, "name") || "Enchantment"} ${enchantRoman(typeof data.maxLevel === "number" ? data.maxLevel : 1)}`]} />;
    case "song":
      return <BookTooltip icon="music_disc_13" title="Music Disc" color="#55ffff" lines={[textOf(data, "description") || textOf(data, "name") || textOf(data, "id")]} />;
    case "shop":
      return <MenuScene data={{ title: textOf(data, "title") || "Shop", rows: typeof data.rows === "number" && data.rows > 0 ? data.rows : Math.ceil(stepsIn(data.goods).length / 9), slots: stepsIn(data.goods).map((one, at) => ({ slot: at, item: textOf(one, "item") })) }} />;
    case "trade":
      return <TradeScene data={data} />;
    case "cosmetic":
      return textOf(data, "item") ? <ItemCell name={textOf(data, "item")} /> : <span className="mc-hidden">{tr("Pick an item to see it here.")}</span>;
    case "painting":
      return <PaintingScene data={data} />;
    default:
      return null;
  }
}

/** A studio block as a cube of its pictures (crossed like a flower for a plant); furniture's picture as it hangs
 * in the world. A look made from a model asset shows in Assets instead. */
function LookScene({ data, block }: { data: StudioData; block: boolean }) {
  const look = objectIn(data.look);
  const server = useContext(StudioServer);
  const side = usePicture(textOf(look, "texture").trim(), server);
  const top = usePicture((textOf(look, "top") || textOf(look, "texture")).trim(), server);
  const bottom = usePicture((textOf(look, "bottom") || textOf(look, "top") || textOf(look, "texture")).trim(), server);
  if (textOf(look, "model").trim()) return <p className="muted studio-preview-none">{tr("This look is a model: open it in Assets to turn it in 3D.")}</p>;
  if (!side) return <p className="muted studio-preview-none">{tr("Pick a picture to see it here.")}</p>;
  if (!block) {
    return (
      <div className="mc-look">
        <img className="mc-look-flat" src={side.url} alt="" />
      </div>
    );
  }
  if (textOf(data, "shape") === "plant") {
    return (
      <div className="mc-look">
        <div className="mc-cross">
          <img src={side.url} alt="" />
          <img src={side.url} alt="" />
        </div>
      </div>
    );
  }
  return (
    <div className="mc-look">
      <div className="mc-cube">
        <img className="top" src={(top ?? side).url} alt="" />
        <img className="bottom" src={(bottom ?? top ?? side).url} alt="" />
        <img className="front" src={side.url} alt="" />
        <img className="right" src={side.url} alt="" />
      </div>
    </div>
  );
}

/** A name tag over a stand-in for the mob. */
function NpcScene({ data }: { data: StudioData }) {
  const mob = (textOf(data, "entity") || "villager").replace(/^minecraft:/, "");
  const name = textOf(data, "name");
  return (
    <div className="mc-npc">
      {name.trim() && (
        <div className="mc-npc-name">
          <MiniText text={name} />
        </div>
      )}
      <NpcBody mob={mob} baby={data.baby === true} glow={data.glow === true} />
    </div>
  );
}

/** A studio mob (M5): its name tag over its base mob, glowing when it glows. A model shows in the form instead. */
function MobScene({ data }: { data: StudioData }) {
  const base = (textOf(data, "base") || "zombie").replace(/^minecraft:/, "");
  const name = textOf(data, "name");
  return (
    <div className="mc-npc">
      {name.trim() && (
        <div className="mc-npc-name">
          <MiniText text={name} />
        </div>
      )}
      <NpcBody mob={base} baby={false} glow={data.glowing === true} />
    </div>
  );
}

/** The mob's spawn egg from the game when there is one, else a box with its name. */
function NpcBody({ mob, baby, glow }: { mob: string; baby: boolean; glow: boolean }) {
  const { icons } = useArt();
  const egg = icons[`${mob}_spawn_egg`];
  if (egg) {
    return (
      <div className={`mc-npc-egg ${baby ? "baby" : ""} ${glow ? "glow" : ""}`}>
        <img src={egg} alt="" />
        <Mc text={itemName(mob)} color="#aaaaaa" />
      </div>
    );
  }
  return <div className={`mc-npc-body ${baby ? "baby" : ""} ${glow ? "glow" : ""}`}>{itemName(mob)}</div>;
}

/** A glyph image (`font` file) in a chat line, at its height and ascent, as the game draws it. */
export function FontScene({ data }: { data: StudioData }) {
  const image = textOf(data, "image").trim().replace(/\.png$/, "");
  const picture = usePicture(image, useContext(StudioServer));
  if (!image) return <p className="muted studio-preview-none">{tr("Pick a picture to see the glyph.")}</p>;
  if (!picture) return <p className="muted studio-preview-none">{tr("Loading the picture…")}</p>;
  const size = glyphSize(data, picture);
  const glyph = (
    <span className="mc-glyph" style={{ width: size.advance * SCALE }}>
      <img src={picture.url} alt="" style={{ top: (7 - size.ascent) * SCALE, width: size.width * SCALE, height: size.height * SCALE }} />
    </span>
  );
  return (
    <div className="mc-chat mc-glyph-scene">
      <div className="mc-glyph-line">
        <MiniText text={`<player>: ${tr("Look")} `} />
        {glyph}
        <MiniText text={` ${tr("nice!")}`} />
      </div>
      <div className="mc-glyph-line">{glyph}</div>
      <div className="mc-glyph-name">:{nameOf(textOf(data, "id") || image, data)}:</div>
    </div>
  );
}

const CHECK_ON = "gui/sprites/widget/checkbox_selected.png";
const CHECK_OFF = "gui/sprites/widget/checkbox.png";
const FIELD = "gui/sprites/widget/text_field.png";

function DialogScene({ data }: { data: StudioData }) {
  const buttons = stepsIn(data.buttons);
  const { files } = useArt();
  const button = files["gui/sprites/widget/button.png"];
  const look: CSSProperties | undefined = button ? { borderImage: `url(${button}) 3 fill / ${3 * SCALE}px`, borderStyle: "solid", borderWidth: 3 * SCALE, background: "none" } : undefined;
  const columns = typeof data.columns === "number" ? Math.min(4, Math.max(1, Math.round(data.columns))) : 2;
  const exit = data.exit && typeof data.exit === "object" && !Array.isArray(data.exit) ? (data.exit as StudioData) : null;
  const canvas = textOf(data, "layout") === "canvas";
  const { glyphs } = useStudioGlyphs(useContext(StudioServer));
  return (
    <div className={`mc-dialog ${canvas ? "canvas" : ""}`}>
      <div className="mc-dialog-title">
        <MiniText text={textOf(data, "title")} />
      </div>
      {canvas && (
        <div className="mc-dialog-canvas">
          <CanvasView canvas={objectIn(data.canvas)} glyphs={glyphs} renderText={(text, shadow) => <MiniText text={text} shadow={shadow} />} />
        </div>
      )}
      {(canvas ? [] : Array.isArray(data.body) ? data.body : linesOf(data, "body")).map((line, at) =>
        typeof line === "string" ? (
          <div key={at} className="mc-dialog-text">
            <MiniText text={line} />
          </div>
        ) : (
          <div key={at} className="mc-dialog-item">
            <ItemCell name={textOf(objectIn(line), "item")} amount={typeof objectIn(line).count === "number" ? (objectIn(line).count as number) : 1} />
            <span className="mc-inline">
              <MiniText text={textOf(objectIn(line), "text")} />
            </span>
          </div>
        ),
      )}
      {stepsIn(data.inputs).map((input, at) => {
        const type = textOf(input, "type");
        const label = (
          <span className="mc-inline">
            <MiniText text={textOf(input, "label") || textOf(input, "key")} />
          </span>
        );
        if (type === "bool") {
          return (
            <div key={at} className="mc-dialog-check">
              {files[input.initial === true ? CHECK_ON : CHECK_OFF] ? (
                <img className="mc-dialog-box-art" src={files[input.initial === true ? CHECK_ON : CHECK_OFF]} alt="" />
              ) : (
                <span className={`mc-dialog-box ${input.initial === true ? "on" : ""}`} />
              )}
              {label}
            </div>
          );
        }
        if (type === "choice") {
          const options = (Array.isArray(input.options) ? input.options : []).map((one) =>
            typeof one === "string" ? { id: one, label: one } : { id: textOf(objectIn(one), "id"), label: textOf(objectIn(one), "label") || textOf(objectIn(one), "id") },
          );
          const initial = textOf(input, "initial");
          const shown = options.find((one) => one.id === initial) ?? options.find((one) => one.id.trim());
          return (
            <div key={at} className="mc-dialog-button wide" style={look}>
              {label}
              <span className="mc-inline">
                <MiniText text={`: ${shown?.label ?? ""}`} />
              </span>
            </div>
          );
        }
        if (type === "number") {
          const least = typeof input.min === "number" ? input.min : 0;
          return (
            <div key={at} className="mc-dialog-button wide" style={look}>
              {label}
              <span className="mc-inline">
                <MiniText text={`: ${least}`} />
              </span>
            </div>
          );
        }
        const rows = typeof input.lines === "number" ? Math.min(20, Math.max(1, Math.round(input.lines))) : 1;
        return (
          <div key={at} className="mc-dialog-field">
            <span>{label}</span>
            <span
              className="mc-dialog-input"
              style={{
                ...(rows > 1 ? { minHeight: 18 + (rows - 1) * 10 } : {}),
                ...(files[FIELD] ? { borderImage: `url(${files[FIELD]}) 1 fill / ${SCALE}px`, borderStyle: "solid", borderWidth: SCALE, background: "none" } : {}),
              }}
            >
              {textOf(input, "initial")}
            </span>
          </div>
        );
      })}
      {/* A canvas without buttons has none in the game (only an exit button under it); other dialogs get OK. */}
      <div className="mc-dialog-buttons" style={{ gridTemplateColumns: `repeat(${buttons.length > 1 || exit ? columns : 1}, minmax(0, 1fr))` }}>
        {(buttons.length || exit || canvas ? buttons : [{ label: "OK" }]).map((button, at) => (
          <div key={at} className="mc-dialog-button" style={look}>
            <MiniText text={textOf(button, "label") || "OK"} />
          </div>
        ))}
      </div>
      {exit && (
        <div className="mc-dialog-button mc-dialog-exit" style={look}>
          <MiniText text={textOf(exit, "label") || "OK"} />
        </div>
      )}
    </div>
  );
}

/** Each entry's share of one roll. */
function LootScene({ data }: { data: StudioData }) {
  const entries = stepsIn(data.entries);
  const weightOf = (one: StudioData) => (typeof one.weight === "number" && one.weight > 0 ? one.weight : one.weight === undefined ? 1 : 0);
  const total = entries.reduce((sum, one) => sum + weightOf(one), 0);
  const rolls = typeof data.rolls === "number" ? data.rolls : 1;
  const most = typeof data.rollsMax === "number" ? Math.max(rolls, data.rollsMax) : rolls;
  return (
    <div className="mc-loot">
      <div className="mc-loot-rolls">
        <Mc text={most > rolls ? tr("{0}–{1} rolls", [rolls, most]) : tr("{0} rolls", [rolls])} color="#aaaaaa" />
      </div>
      {entries.map((one, at) => {
        const item = textOf(one, "item");
        const share = total > 0 ? weightOf(one) / total : 0;
        const least = typeof one.min === "number" ? one.min : 1;
        const top = typeof one.max === "number" ? Math.max(least, one.max) : least;
        return (
          <div key={at} className="mc-loot-row">
            <ItemCell name={item} />
            <span className="mc-loot-name">
              <Mc text={item ? `${itemName(item)} x ${top > least ? `${least}-${top}` : least}` : tr("Nothing")} />
            </span>
            <span className="mc-loot-share">
              <Mc text={`${Math.round(share * 1000) / 10}%`} color="#aaaaaa" />
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** The pop-up a player sees when they earn it. */
function AdvancementScene({ data }: { data: StudioData }) {
  const frame = textOf(data, "frame");
  const challenge = frame === "challenge";
  const heading = challenge ? tr("Challenge Complete!") : frame === "goal" ? tr("Goal Reached!") : tr("Advancement Made!");
  const { files } = useArt();
  const toast = files["gui/sprites/toast/advancement.png"];
  if (toast) {
    return (
      <Piece url={toast} size={[160, 32]} sheet={160} className="mc-toast-art">
        <span className="mc-toast-art-icon">
          <ItemCell name={textOf(data, "icon") || "diamond"} bare />
        </span>
        <span className="mc-toast-art-text">
          <MiniText text={`<${challenge ? "#ff55ff" : "#ffff00"}>${heading}`} shadow={false} />
          <MiniText text={textOf(data, "title")} shadow={false} />
        </span>
      </Piece>
    );
  }
  return (
    <div className="mc-toast">
      <ItemCell name={textOf(data, "icon") || "diamond"} />
      <div className="mc-toast-text">
        <span className={challenge ? "mc-toast-head challenge" : "mc-toast-head"}>{heading}</span>
        <MiniText text={textOf(data, "title")} />
        {textOf(data, "description").trim() && <MiniText text={textOf(data, "description")} base="#aaaaaa" />}
      </div>
    </div>
  );
}

/** What the world will be, with flat layers drawn top first. */
function WorldScene({ data }: { data: StudioData }) {
  const type = textOf(data, "type") || "normal";
  const layers = stepsIn(data.layers);
  return (
    <div className="mc-world">
      <div className="mc-world-name">
        <Mc text={textOf(data, "name") || "world"} />
      </div>
      <div className="mc-world-kind">
        <Mc text={`${type} - ${textOf(data, "environment") || "normal"}`} color="#aaaaaa" />
      </div>
      {type === "flat" && (
        <div className="mc-world-layers">
          {[...layers].reverse().map((one, at) => (
            <div key={at} className="mc-world-layer">
              <span className="mc-world-block">
                <ItemCell name={textOf(one, "block")} bare />
                <Mc text={itemName(textOf(one, "block"))} />
              </span>
              <Mc text={String(typeof one.height === "number" ? one.height : 1)} color="#aaaaaa" />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** The game's boss bar colors. */
const BAR_COLORS: Record<string, string> = {
  pink: "#ec00b8",
  blue: "#00b7ec",
  red: "#ec3500",
  green: "#1dec00",
  yellow: "#e9ec00",
  purple: "#7b00ec",
  white: "#ececec",
};

function BossBarScene({ data }: { data: StudioData }) {
  const name = textOf(data, "color").trim().toLowerCase();
  const color = BAR_COLORS[name] ?? BAR_COLORS.purple;
  const raw = typeof data.progress === "number" && Number.isFinite(data.progress) ? data.progress : 1;
  const filled = Math.min(Math.max(raw, 0), 1);
  const notches = Number(/^notched_(6|10|12|20)$/.exec(textOf(data, "style").trim().toLowerCase())?.[1] ?? 0);
  const { files } = useArt();
  const bar = name in BAR_COLORS ? name : "purple";
  const back = files[`gui/sprites/boss_bar/${bar}_background.png`];
  const front = files[`gui/sprites/boss_bar/${bar}_progress.png`];
  if (back && front) {
    const notchBack = notches ? files[`gui/sprites/boss_bar/notched_${notches}_background.png`] : undefined;
    const notchFront = notches ? files[`gui/sprites/boss_bar/notched_${notches}_progress.png`] : undefined;
    return (
      <div className="mc-bossbar art">
        <div className="mc-bossbar-title">
          <MiniText text={textOf(data, "title")} />
        </div>
        <div className="mc-bossbar-sprite" style={{ width: 182 * MENU_SCALE, height: 5 * MENU_SCALE }}>
          <Piece url={back} size={[182, 5]} sheet={182} scale={MENU_SCALE} />
          {notchBack && <Piece url={notchBack} size={[182, 5]} sheet={182} scale={MENU_SCALE} />}
          <div className="mc-bossbar-sprite-fill" style={{ width: Math.round(182 * filled) * MENU_SCALE }}>
            <Piece url={front} size={[182, 5]} sheet={182} scale={MENU_SCALE} />
            {notchFront && <Piece url={notchFront} size={[182, 5]} sheet={182} scale={MENU_SCALE} />}
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="mc-bossbar">
      <div className="mc-bossbar-title">
        <MiniText text={textOf(data, "title")} />
      </div>
      <div className="mc-bossbar-track" style={{ "--bar": color } as CSSProperties}>
        <div className="mc-bossbar-fill" style={{ width: `${filled * 100}%` }} />
        {Array.from({ length: Math.max(notches - 1, 0) }, (_, at) => (
          <span key={at} className="mc-bossbar-notch" style={{ left: `${((at + 1) / notches) * 100}%` }} />
        ))}
      </div>
    </div>
  );
}

function objectIn(value: unknown): StudioData {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as StudioData) : {};
}

function stepsIn(value: unknown): StudioData[] {
  return Array.isArray(value) ? value.map(objectIn) : [];
}

/** "diamond_sword" -> "Diamond Sword"; a studio item keeps its "studio:" name. */
export function itemName(name: string): string {
  if (!name) return "";
  if (name.startsWith("studio:")) return name;
  return name
    .replace(/^minecraft:/, "")
    .split("_")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

/** Two letters standing in for an item's picture. */
function itemMark(name: string): string {
  const bare = name.replace(/^(minecraft|studio):/, "");
  const words = bare.split("_").filter(Boolean);
  return ((words[0]?.[0] ?? "") + (words[1]?.[0] ?? words[0]?.[1] ?? "")).toUpperCase();
}

/** One inventory slot. With the game's pictures the item's own icon shows; `bare` drops the slot frame (the GUI
 * texture under it has one). */
export function ItemCell({ name, label, amount, bare = false }: { name: string; label?: string; amount?: number; bare?: boolean }) {
  const { icons, files } = useArt();
  const slot = bare ? undefined : files["gui/sprites/container/slot.png"];
  const icon = icons[name.trim().toLowerCase().replace(/^minecraft:/, "")];
  return (
    <span className={`mc-slot ${name ? "full" : ""} ${bare ? "bare" : ""} ${icon ? "pictured" : ""} ${slot ? "art" : ""}`} style={slot ? ({ "--slot": `url(${slot})` } as CSSProperties) : undefined} title={label || itemName(name) || undefined}>
      {icon ? <img src={icon} alt="" /> : name && <span className="mc-slot-mark">{itemMark(name)}</span>}
      {amount !== undefined && amount > 1 && (
        <small>
          <Mc text={String(amount)} />
        </small>
      )}
    </span>
  );
}

/** What chat shows for the actions that write to it. */
function chatLines(steps: StudioData[]): string[] {
  return steps
    .filter((one) => one.type === "message" || one.type === "broadcast")
    .map((one) => (typeof one.text === "string" ? one.text : ""))
    .filter((one) => one.trim())
    // Variables show a sample number in the preview.
    .map((one) => one.replace(/<p?var:[a-zA-Z0-9_.-]+>/g, "3"));
}

function CommandScene({ data }: { data: StudioData }) {
  const name = textOf(data, "name") || "command";
  const lines = chatLines(stepsIn(data.actions));
  return (
    <div className="mc-chat">
      {lines.map((line, at) => (
        <div key={at} className="mc-chat-line">
          <MiniText text={line} values={{ ...SAMPLE_VALUES, args: "", arg1: "", sender: SAMPLE_VALUES.player }} />
        </div>
      ))}
      <div className="mc-chat-line mc-typed">
        <Mc text={`/${name}`} />
      </div>
    </div>
  );
}

function MenuScene({ data }: { data: StudioData }) {
  const rows = typeof data.rows === "number" ? Math.min(6, Math.max(1, Math.round(data.rows))) : 3;
  const fill = textOf(data, "fill");
  const bySlot = new Map<number, StudioData>();
  for (const slot of stepsIn(data.slots)) {
    if (typeof slot.slot === "number") bySlot.set(slot.slot, slot);
  }
  const { files } = useArt();
  const chest = files["gui/container/generic_54.png"];
  // The background glyph starts at the screen's left edge, on the title's line (top 6, ascent 7 in the game).
  const { glyphs } = useStudioGlyphs(useContext(StudioServer));
  const background = byName(glyphs)[textOf(data, "background").trim()];
  const cell = (at: number) => {
    const slot = bySlot.get(at);
    const item = slot?.item;
    const name = typeof item === "string" ? item : textOf(objectIn(item), "material");
    const label = typeof item === "string" ? item : textOf(objectIn(item), "name").replace(/<[^>]+>/g, "");
    return { name: slot ? name : fill, label: slot ? label || itemName(name) : undefined };
  };
  if (chest) {
    // The chest screen: its top with the title and rows, then its bottom edge.
    const scale = MENU_SCALE;
    return (
      <div className="mc-menu-art" style={{ width: 176 * scale }}>
        <Piece url={chest} size={[176, 17 + rows * 18]} scale={scale}>
          {background && (
            <img
              className="mc-menu-art-background"
              src={background.url}
              alt=""
              style={{ left: 0, top: (13 - background.ascent) * scale, width: background.width * scale, height: background.height * scale }}
            />
          )}
          <span className="mc-menu-art-title" style={{ left: 8 * scale, top: 6 * scale }}>
            <MiniText text={textOf(data, "title")} base="#404040" shadow={false} />
          </span>
          {Array.from({ length: rows * 9 }, (_, at) => {
            const one = cell(at);
            return (
              <span key={at} className="mc-menu-art-slot" style={{ left: (8 + (at % 9) * 18) * scale, top: (18 + Math.floor(at / 9) * 18) * scale, width: 16 * scale, height: 16 * scale }}>
                <ItemCell name={one.name} label={one.label} bare />
              </span>
            );
          })}
        </Piece>
        <Piece url={chest} at={[0, 215]} size={[176, 7]} scale={scale} />
      </div>
    );
  }
  return (
    <div className="mc-menu">
      <div className="mc-menu-title">
        <MiniText text={textOf(data, "title")} base="#404040" />
      </div>
      <div className="mc-menu-grid" style={{ gridTemplateRows: `repeat(${rows}, 1fr)` }}>
        {Array.from({ length: rows * 9 }, (_, at) => {
          const slot = bySlot.get(at);
          const item = slot?.item;
          const name = typeof item === "string" ? item : textOf(objectIn(item), "material");
          const label = typeof item === "string" ? item : textOf(objectIn(item), "name").replace(/<[^>]+>/g, "");
          return <ItemCell key={at} name={slot ? name : fill} label={slot ? label || itemName(name) : undefined} />;
        })}
      </div>
    </div>
  );
}

/** The game's tooltip box: its background and frame pictures (plain colors until they load). */
function Tip({ children }: { children: ReactNode }) {
  const { files } = useArt();
  const back = files["gui/sprites/tooltip/background.png"];
  const frame = files["gui/sprites/tooltip/frame.png"];
  return (
    <div className={`mc-tooltip ${back && frame ? "art" : ""}`} style={back && frame ? ({ "--tip-back": `url(${back})`, "--tip-frame": `url(${frame})` } as CSSProperties) : undefined}>
      {children}
    </div>
  );
}

/** The name colors the game gives each rarity. */
const RARITY_COLORS: Record<string, string> = { uncommon: "#ffff55", rare: "#55ffff", epic: "#ff55ff" };

function ItemTooltip({ data }: { data: StudioData }) {
  const enchantments = Object.entries(objectIn(data.enchantments));
  const material = textOf(data, "material");
  const picture = useItemPicture(data);
  const attack = objectIn(data.attack);
  const nameColor = RARITY_COLORS[textOf(data, "rarity")];
  const stat = (value: unknown) => (typeof value === "number" ? Math.round(value * 100) / 100 : null);
  return (
    <div className="mc-item-preview">
      <span className="mc-tooltip-slot">
        {picture ? (
          <span className="mc-slot full pictured">
            <img src={picture} alt="" />
          </span>
        ) : (
          <ItemCell name={material} />
        )}
      </span>
      {data.hideTooltip !== true && (
        <Tip>
          <span className="mc-tooltip-name">{textOf(data, "name") ? <MiniText text={textOf(data, "name")} base={nameColor} /> : <Mc text={itemName(material) || tr("Item")} color={nameColor ?? "#ffffff"} />}</span>
          {enchantments.map(([name, level]) => (
            <span key={name} className="mc-tooltip-enchant">
              <Mc text={`${itemName(name)} ${enchantLevel(name, typeof level === "number" ? level : 1)}`} color="#aaaaaa" />
            </span>
          ))}
          {linesOf(data, "lore").map((line, at) => (
            <span key={at} className="mc-tooltip-lore">
              <MiniText text={line} base="#aa00aa" />
            </span>
          ))}
          {(stat(attack.damage) !== null || stat(attack.speed) !== null) && (
            <>
              <span className="mc-tooltip-lore">&nbsp;</span>
              <span className="mc-tooltip-gray">
                <Mc text={tr("When in Main Hand:")} color="#aaaaaa" />
              </span>
              {stat(attack.damage) !== null && (
                <span className="mc-tooltip-green">
                  <Mc text={` ${tr("{0} Attack Damage", [String(stat(attack.damage))])}`} color="#00aa00" />
                </span>
              )}
              {stat(attack.speed) !== null && (
                <span className="mc-tooltip-green">
                  <Mc text={` ${tr("{0} Attack Speed", [String(stat(attack.speed))])}`} color="#00aa00" />
                </span>
              )}
            </>
          )}
          {data.unbreakable === true && (
            <span className="mc-tooltip-blue">
              <Mc text={tr("Unbreakable")} color="#5555ff" />
            </span>
          )}
          <span className="mc-tooltip-id">
            <Mc text={`minecraft:${material.replace(/^minecraft:/, "")}`} color="#555555" />
          </span>
        </Tip>
      )}
    </div>
  );
}

/** GUI textures draw at the same scale as the text, so they stay 1:1 like in the game; `Fit` shrinks the whole
 * scene together when it is wider than the preview. */
const MENU_SCALE = SCALE;

function RecipeScene({ data }: { data: StudioData }) {
  const type = textOf(data, "type") || "shaped";
  const result = objectIn(data.result);
  const made = <ItemCell name={textOf(result, "item")} amount={typeof result.amount === "number" ? result.amount : 1} />;
  const { files } = useArt();
  const scale = MENU_SCALE;
  const at = (x: number, y: number, item: string, amount?: number) => (
    <span key={`${x},${y}`} className="mc-menu-art-slot" style={{ left: x * scale, top: y * scale, width: 16 * scale, height: 16 * scale }}>
      <ItemCell name={item} amount={amount} bare />
    </span>
  );
  const amount = typeof result.amount === "number" ? result.amount : 1;
  const table = files["gui/container/crafting_table.png"];
  if (table && (type === "shaped" || type === "shapeless")) {
    const keys = objectIn(data.keys);
    const cells: string[] =
      type === "shaped"
        ? linesOf(data, "shape")
            .slice(0, 3)
            .flatMap((row) => Array.from({ length: 3 }, (_, column) => {
              const letter = row[column] ?? " ";
              return letter === " " ? "" : typeof keys[letter] === "string" ? (keys[letter] as string) : "?";
            }))
        : linesOf(data, "ingredients").filter((one) => one.trim()).slice(0, 9);
    return (
      <Piece url={table} size={[176, 80]} scale={scale} className="mc-recipe-art">
        {Array.from({ length: 9 }, (_, index) => at(30 + (index % 3) * 18, 17 + Math.floor(index / 3) * 18, cells[index] ?? ""))}
        {at(124, 35, textOf(result, "item"), amount)}
      </Piece>
    );
  }
  const furnace = files["gui/container/furnace.png"];
  if (furnace && type !== "stonecutting") {
    return (
      <Piece url={furnace} size={[176, 80]} scale={scale} className="mc-recipe-art">
        {at(56, 17, textOf(data, "input"))}
        {at(116, 35, textOf(result, "item"), amount)}
      </Piece>
    );
  }
  if (type === "shaped" || type === "shapeless") {
    const keys = objectIn(data.keys);
    const cells: string[] =
      type === "shaped"
        ? linesOf(data, "shape")
            .slice(0, 3)
            .flatMap((row) => Array.from({ length: 3 }, (_, at) => {
              const letter = row[at] ?? " ";
              return letter === " " ? "" : typeof keys[letter] === "string" ? (keys[letter] as string) : "?";
            }))
        : linesOf(data, "ingredients").filter((one) => one.trim()).slice(0, 9);
    return (
      <div className="mc-recipe">
        <div className="mc-recipe-grid">
          {Array.from({ length: 9 }, (_, at) => (
            <ItemCell key={at} name={cells[at] ?? ""} />
          ))}
        </div>
        <span className="mc-recipe-arrow" aria-hidden>
          →
        </span>
        {made}
      </div>
    );
  }
  return (
    <div className="mc-recipe">
      <ItemCell name={textOf(data, "input")} />
      <span className="mc-recipe-arrow" aria-hidden>
        →
      </span>
      {made}
    </div>
  );
}

function RuleScene({ data }: { data: StudioData }) {
  const lines = chatLines(stepsIn(data.then));
  if (!lines.length) return <span className="mc-hidden">{tr("This rule doesn't write to chat.")}</span>;
  return (
    <div className="mc-chat">
      {lines.map((line, at) => (
        <div key={at} className="mc-chat-line">
          <MiniText text={line} values={{ ...SAMPLE_VALUES, message: "hello", block: "stone", killer: "Alex", command: "spawn" }} />
        </div>
      ))}
    </div>
  );
}

/** Shrinks what's inside (text and pictures together, so their sizes stay 1:1) when it is wider than its room. */
/** The server list entry: the game's unknown-server icon and its full ping bars. */
function MotdScene({ data }: { data: StudioData }) {
  const { files } = useArt();
  const icon = files["misc/unknown_server.png"];
  const ping = files["gui/sprites/icon/ping_5.png"];
  return (
    <div className="mc-server">
      {icon ? <img className="mc-server-icon art" src={icon} alt="" /> : <i className="mc-server-icon" aria-hidden />}
      <div className="mc-server-body">
        <span className="mc-server-name">
          <Mc text={tr("My server")} />
          <small>
            <Mc text="12/20" color="#aaaaaa" />
            {ping && <img className="mc-ping" src={ping} alt="" />}
          </small>
        </span>
        <span className="mc-server-motd">
          <MiniText text={linesOf(data, "lines").slice(0, 2).join("<newline>")} base="#aaaaaa" />
        </span>
      </div>
    </div>
  );
}

/** The player list: Steve's face from the game's own skin and the ping bars on each row. */
function TabScene({ data }: { data: StudioData }) {
  const { files } = useArt();
  const skin = files["entity/player/wide/steve.png"];
  const ping = files["gui/sprites/icon/ping_5.png"];
  const name = textOf(data, "playerName");
  return (
    <div className="mc-tab">
      <div className="mc-tab-text">
        <MiniText text={linesOf(data, "header").join("<newline>")} />
      </div>
      <div className="mc-tab-players">
        {[SAMPLE_VALUES.player, ...OTHERS].map((player) => (
          <span key={player} className="mc-tab-player">
            {skin ? <Piece url={skin} at={[8, 8]} size={[8, 8]} sheet={64} className="mc-tab-face" /> : <i aria-hidden />}
            <span className="mc-tab-name">{name ? <MiniText text={name} values={{ ...SAMPLE_VALUES, player }} /> : <Mc text={player} />}</span>
            {ping && <img className="mc-ping" src={ping} alt="" />}
          </span>
        ))}
      </div>
      <div className="mc-tab-text">
        <MiniText text={linesOf(data, "footer").join("<newline>")} />
      </div>
    </div>
  );
}

/** What a function says in chat when it runs. */
function FunctionScene({ data }: { data: StudioData }) {
  const lines = chatLines(stepsIn(data.actions));
  if (!lines.length) return <span className="mc-hidden">{tr("This function doesn't write to chat.")}</span>;
  return (
    <div className="mc-chat">
      {lines.map((line, at) => (
        <div key={at} className="mc-chat-line">
          <MiniText text={line} />
        </div>
      ))}
    </div>
  );
}

const ROMAN = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
function enchantRoman(level: number) {
  const at = Math.round(level);
  return ROMAN[at] ?? String(at);
}

/** A game item's tooltip (enchanted book, music disc): the game's icon, its name, one gray line under it. */
function BookTooltip({ icon, title, color, lines }: { icon: string; title: string; color: string; lines: string[] }) {
  return (
    <div className="mc-item-preview">
      <span className="mc-tooltip-slot">
        <ItemCell name={icon} />
      </span>
      <Tip>
        <span className="mc-tooltip-name">
          <Mc text={title} color={color} />
        </span>
        {lines
          .filter((one) => one.trim())
          .map((line, at) => (
            <span key={at} className="mc-tooltip-lore">
              <MiniText text={line} base="#aaaaaa" />
            </span>
          ))}
      </Tip>
    </div>
  );
}

/** Each offer as the villager screen lists it: what they take, the game's trade arrow, what they give. */
function TradeScene({ data }: { data: StudioData }) {
  const { files } = useArt();
  const arrow = files["gui/sprites/container/villager/trade_arrow.png"];
  const offers = stepsIn(data.offers).slice(0, 7);
  if (!offers.length) return <span className="mc-hidden">{tr("No offers yet.")}</span>;
  const amount = (one: StudioData, key: string) => (typeof one[key] === "number" ? (one[key] as number) : 1);
  return (
    <div className="mc-trades">
      {offers.map((one, at) => (
        <div key={at} className="mc-trade">
          <ItemCell name={textOf(one, "buy")} amount={amount(one, "buyAmount")} />
          {textOf(one, "buy2") ? <ItemCell name={textOf(one, "buy2")} amount={amount(one, "buy2Amount")} /> : <span className="mc-trade-gap" />}
          {arrow ? <img className="mc-trade-arrow" src={arrow} alt="" /> : <span className="mc-recipe-arrow">→</span>}
          <ItemCell name={textOf(one, "sell")} amount={amount(one, "sellAmount")} />
        </div>
      ))}
    </div>
  );
}

/** The painting's picture at its size in blocks, with its title and author under it. */
function PaintingScene({ data }: { data: StudioData }) {
  const picture = usePicture(textOf(data, "texture").trim(), useContext(StudioServer));
  const wide = typeof data.width === "number" ? Math.min(16, Math.max(1, data.width)) : 1;
  const high = typeof data.height === "number" ? Math.min(16, Math.max(1, data.height)) : 1;
  if (!picture) return <p className="muted studio-preview-none">{tr("Pick a picture to see it here.")}</p>;
  const cell = Math.max(8, Math.floor(96 / Math.max(wide, high)));
  return (
    <div className="mc-painting">
      <img src={picture.url} alt="" style={{ width: wide * cell, height: high * cell }} />
      {(textOf(data, "title") || textOf(data, "author")) && (
        <Tip>
          {textOf(data, "title") && <MiniText text={`<yellow>${textOf(data, "title")}`} />}
          {textOf(data, "author") && <MiniText text={textOf(data, "author")} base="#aaaaaa" />}
        </Tip>
      )}
    </div>
  );
}

function Fit({ children, grow = false }: { children: ReactNode; grow?: boolean }) {
  const outer = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const [zoom, setZoom] = useState(1);
  const zoomNow = useRef(1);
  useLayoutEffect(() => {
    const room = outer.current;
    const body = inner.current;
    if (!room || !body) return;
    const measure = () => {
      if (grow) {
        // Full screen: the scene's own size (without the zoom already on it), grown to fill the window both ways,
        // up to 4 times.
        const shown = (body.firstElementChild ?? body).getBoundingClientRect();
        const now = zoomNow.current || 1;
        const wideNow = shown.width / now;
        const highNow = shown.height / now;
        if (wideNow <= 0 || highNow <= 0) return;
        const grown = Math.min(room.clientWidth / wideNow, (window.innerHeight - 96) / highNow, 4);
        const perPixel = SCALE * (window.devicePixelRatio || 1);
        const next = Math.max(1, Math.floor(grown * perPixel + 1e-6)) / perPixel;
        zoomNow.current = next;
        return setZoom(next);
      }
      // Measure without the zoom already on: a scene that fills its width (chat, holograms) is room / zoom wide
      // while shrunk, so measuring it zoomed kept it at half size for good.
      const kept = body.style.zoom;
      body.style.zoom = "";
      const wide = body.scrollWidth;
      body.style.zoom = kept;
      const fits = wide > 0 ? Math.min(1, room.clientWidth / wide) : 1;
      if (fits >= 1) return setZoom(1);
      // Whole steps only matter where items sit on a GUI picture (menus, recipes). A tooltip, toast or boss bar a
      // little wider than the room shrinks just enough, instead of dropping to half size with tiny text.
      if (!body.querySelector(".mc-menu-art, .mc-recipe-art")) return setZoom(Math.max(0.5, Math.floor(fits * 100) / 100));
      // Shrink only in whole steps: one game pixel stays a whole number of screen pixels, so the GUI picture and the
      // item pictures on it keep the same pixel grid (at 1.78x they rounded apart and the icons sat off the slots).
      const perPixel = SCALE * (window.devicePixelRatio || 1);
      const steps = Math.floor(fits * perPixel + 1e-6);
      setZoom(Math.max(1, steps) / perPixel);
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(room);
    watch.observe(body);
    return () => watch.disconnect();
  }, [grow]);
  return (
    <div ref={outer} className="mc-fit">
      <div ref={inner} className="mc-fit-body" style={zoom !== 1 ? { zoom } : undefined}>
        {children}
      </div>
    </div>
  );
}
