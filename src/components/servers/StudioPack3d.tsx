import { createContext, lazy, type ReactNode, Suspense, useContext, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { assetsOf, pictureOf } from "./StudioItemFields";
import { Icon } from "../ui/Icon";
import { armorModel, cubeModel, jsonModel, plantModel, spriteModel, textureName } from "./studioShapes";
import "./StudioModels.css";

const ModelView = lazy(() => import("../files/ModelView").then((module) => ({ default: module.ModelView })));

/** The right-hand preview column of the studio page; 3D views move there when it's shown (null = stay in the form). */
export const StudioSideSlot = createContext<HTMLElement | null>(null);

/** Puts a 3D view in the studio's right column when there is one, else leaves it where it is in the form. */
export function InSide({ title, children }: { title?: string; children: ReactNode }) {
  const slot = useContext(StudioSideSlot);
  if (!slot) return <>{children}</>;
  return createPortal(
    <div className="studio-side-card">
      {title && <span className="studio-side-title">{title}</span>}
      {children}
    </div>,
    slot,
  );
}

/** What the pack draws for a block, a furniture piece or a piece of armor. */
export type PackShape =
  | { kind: "cube"; side: string; top?: string; bottom?: string }
  | { kind: "plant"; texture: string }
  | { kind: "model"; model: string }
  | { kind: "armor"; slot: string; texture: string }
  | { kind: "sprite"; texture: string };

/** Which pixels of a picture are solid (alpha over about 10%), read from the picture itself. */
function solidPixels(url: string): Promise<{ width: number; height: number; solid: (x: number, y: number) => boolean } | null> {
  return new Promise((resolve) => {
    const image = new Image();
    image.onload = () => {
      const width = image.naturalWidth;
      const height = image.naturalHeight;
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const context = canvas.getContext("2d");
      if (!context || !width || !height) return resolve(null);
      context.drawImage(image, 0, 0);
      const pixels = context.getImageData(0, 0, width, height).data;
      resolve({ width, height, solid: (x, y) => pixels[(y * width + x) * 4 + 3] > 25 });
    };
    image.onerror = () => resolve(null);
    image.src = url;
  });
}

async function sourceOf(serverId: string, shape: PackShape): Promise<string> {
  const assets = await assetsOf(serverId);
  const picture = async (name: string | undefined) => {
    const asset = name ? assets.find((one) => one.folder === "textures" && one.name === name && one.file.endsWith(".png")) : undefined;
    return asset ? pictureOf(serverId, asset).catch(() => null) : null;
  };
  switch (shape.kind) {
    case "cube":
      return cubeModel(await picture(shape.side), await picture(shape.top || shape.side), await picture(shape.bottom || shape.top || shape.side));
    case "plant":
      return plantModel(await picture(shape.texture));
    case "armor":
      return armorModel(shape.slot, await picture(shape.texture));
    case "sprite": {
      const url = await picture(shape.texture);
      const read = url ? await solidPixels(url) : null;
      return read ? spriteModel(url, read.width, read.height, read.solid) : spriteModel(url);
    }
    case "model": {
      const overview = await commands.studioOverview(serverId);
      const text = await commands.readEditableFile(serverId, `${overview.root}/assets/models/${shape.model}.json`);
      const named = Object.values((JSON.parse(text) as { textures?: Record<string, string> }).textures ?? {});
      const pictures: Record<string, string> = {};
      for (const name of new Set(named.map(textureName).filter((one): one is string => !!one))) {
        const url = await picture(name);
        if (url) pictures[name] = url;
      }
      return jsonModel(text, pictures);
    }
  }
}

/** The pack's look turned in 3D (drag to turn, scroll to zoom), with the studio's own pictures. */
export function StudioPack3d({ serverId, shape }: { serverId?: string; shape: PackShape | null }) {
  const key = shape ? JSON.stringify(shape) : "";
  const [source, setSource] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    setSource(null);
    setProblem(null);
    if (!serverId || !key) return;
    let live = true;
    sourceOf(serverId, JSON.parse(key) as PackShape).then(
      (next) => live && setSource(next),
      (cause) => live && setProblem(String(cause)),
    );
    return () => {
      live = false;
    };
  }, [serverId, key]);
  if (!serverId || !shape) return null;
  return (
    <InSide>
    <div className="field studio-field studio-pack3d">
      <span>{tr("3D preview")}</span>
      {problem ? (
        <p className="studio-model-note">{tr("The preview couldn't be drawn: {0}", [problem])}</p>
      ) : (
        <Studio3dStage source={source} className="studio-pack3d-stage" />
      )}
      <small>{tr("Drag to turn, scroll to zoom. Pictures from the assets; the game's own pictures show grey.")}</small>
    </div>
    </InSide>
  );
}

/**
 * The 3D stage the studio forms share: ModelView in a framed box with a reset button (it builds the view again,
 * which puts the camera back). Nothing turns by itself, so reduced motion has nothing to stop here.
 */
export function Studio3dStage({ source, animation, time, className = "" }: { source: string | null; animation?: string | null; time?: number | null; className?: string }) {
  const [round, setRound] = useState(0);
  return (
    <div className={`studio-model-stage ${className}`}>
      <Suspense fallback={null}>{source && <ModelView key={round} source={source} animation={animation} time={time} />}</Suspense>
      {source && (
        <button type="button" className="icon-button small studio-stage-reset" aria-label={tr("Reset view")} data-tip={tr("Reset view")} onClick={() => setRound(round + 1)}>
          <Icon name="refresh" />
        </button>
      )}
    </div>
  );
}
