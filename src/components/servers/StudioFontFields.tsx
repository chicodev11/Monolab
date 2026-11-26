import { useEffect, useState } from "react";
import type { StudioAsset } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Dropdown } from "../ui/Dropdown";
import { glyphSize, nameOf, reloadGlyphs, usePicture, validGlyphName } from "./studioGlyphs";
import type { StudioData } from "./StudioPreview";
import "./StudioCanvas.css";

function pictureOf(data: StudioData): string {
  return (typeof data.image === "string" ? data.image : "").trim().replace(/\.png$/, "");
}

/**
 * Studio maker M2: a glyph image (`font` file). A picture from the studio's textures becomes one character of the
 * pack's font, used as `:name:` in any studio text and as a picture in canvas dialogs.
 */
export function FontFields({ id: given, data, onChange, serverId }: { id?: string; data: StudioData; onChange: (next: StudioData) => void; serverId?: string }) {
  const id = given ?? (typeof data.id === "string" ? data.id : "");
  const [assets, setAssets] = useState<StudioAsset[]>([]);
  const image = pictureOf(data);
  const picture = usePicture(image, serverId);
  useEffect(() => {
    if (!serverId) return;
    let live = true;
    commands.studioAssetList(serverId).then(
      (list) => {
        if (live) setAssets(list.assets.filter((one) => one.folder === "textures" && one.file.endsWith(".png")));
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId]);
  // Canvases and previews read the glyphs again once this form closes (the file is saved by then).
  useEffect(() => () => reloadGlyphs(serverId), [serverId]);

  const name = nameOf(id, data);
  const size = picture ? glyphSize(data, picture) : null;
  const set = (key: string, value: unknown) => {
    const next = { ...data };
    if (value === undefined || value === "") delete next[key];
    else next[key] = value;
    onChange(next);
  };
  const whole = (key: string, label: string, fallback: number, least: number, most: number, hint: string) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        max={most}
        value={typeof data[key] === "number" ? (data[key] as number) : fallback}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (Number.isFinite(value)) set(key, Math.min(most, Math.max(least, Math.round(value))));
        }}
      />
      <small>{hint}</small>
    </label>
  );

  return (
    <>
      <label className="field studio-field">
        <span>{tr("Picture")}</span>
        <Dropdown
          value={image}
          options={assets.map((one) => ({ value: one.name, label: one.file }))}
          placeholder={tr("Pick a texture")}
          search={tr("Find a texture")}
          onChange={(value) => set("image", value)}
        />
        <small>{tr("From the studio's textures (Assets). At most 255×255.")}</small>
      </label>
      {picture && (
        <div className="studio-glyph-picture">
          <img src={picture.url} alt="" />
          <small>{tr("{0}×{1} pixels", [picture.width, picture.height])}</small>
        </div>
      )}
      <label className="field studio-field">
        <span>{tr("Name")}</span>
        <input value={typeof data.name === "string" ? data.name : ""} placeholder={id.replace(/-/g, "_")} onChange={(event) => set("name", event.target.value.trim())} spellCheck={false} />
        <small>{validGlyphName(name) ? tr("Write :{0}: in any studio text: chat, scoreboard, holograms, dialogs.", [name]) : tr("Names use a-z, 0-9 and _, up to 32.")}</small>
      </label>
      <div className="studio-fields-row">
        {whole("height", tr("Height"), picture?.height ?? 8, 1, 256, tr("Drawn height in game pixels. Text is 8; a canvas background is its own size."))}
        {whole("ascent", tr("Ascent"), Math.min(7, size?.height ?? 8), -256, size?.height ?? 256, tr("How far above the text's base line its top is. 7 lines up with letters."))}
      </div>
      {picture && (
        <div className="studio-fields-row">
          <button type="button" className="button ghost small" onClick={() => onChange({ ...data, height: picture.height, ascent: Math.min(7, picture.height) })}>
            {tr("Use the picture's size")}
          </button>
          <button type="button" className="button ghost small" onClick={() => onChange({ ...data, height: 8, ascent: 7 })}>
            {tr("Letter size (8)")}
          </button>
        </div>
      )}
      {size && <p className="studio-placeholders">{tr("Drawn {0}×{1}; the text after it moves {2} pixels.", [size.width, size.height, size.advance])}</p>}
      <p className="studio-placeholders">{tr("Glyphs go into the studio pack. Players see them once the pack is built and sent (Assets).")}</p>
    </>
  );
}
