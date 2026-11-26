import { useCallback, useEffect, useRef, useState } from "react";
import type { PackInfo, Sending, StudioAssetList } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { onWeb, webSession } from "../../lib/bridge";
import { uploadRaw } from "../../lib/pickFile";
import { SegmentedControl, Switch } from "../primitives";
import { Icon } from "../ui/Icon";

const FOLDERS = [
  { id: "textures", label: () => tr("Pictures"), accept: ".png,.mcmeta", hint: () => tr("PNG, up to 2 MB. Used as monolab:studio/<name>.") },
  { id: "models", label: () => tr("Models"), accept: ".json,.bbmodel", hint: () => tr("Minecraft JSON models now; Blockbench .bbmodel is kept for animated models later.") },
  { id: "sounds", label: () => tr("Sounds"), accept: ".ogg", hint: () => tr("Ogg, up to 8 MB. Plays as monolab:studio.<name>.") },
] as const;

/** "Ruby Sword.PNG" → "ruby_sword.png", the studio's file-name rule. */
export function assetFileName(name: string): string {
  const lower = name.toLowerCase();
  const ending = [".png.mcmeta", ".png", ".json", ".bbmodel", ".ogg"].find((one) => lower.endsWith(one)) ?? "";
  const stem = lower
    .slice(0, lower.length - ending.length)
    .replace(/[^a-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 48);
  return `${stem || "asset"}${ending}`;
}

function size(bytes: number) {
  return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

async function base64Of(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let text = "";
  for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(text);
}

/**
 * Studio maker M0: the studio's own pictures, models and sounds, and the resource pack built from them that the
 * add-on hands to players.
 */
export function StudioAssetLibrary({ serverId, pumpkin, onError, onNotice, confirm }: { serverId: string; pumpkin: boolean; onError: (message: string) => void; onNotice: (message: string) => void; confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean> }) {
  const [list, setList] = useState<StudioAssetList | null>(null);
  const [folder, setFolder] = useState<(typeof FOLDERS)[number]["id"]>("textures");
  const [busy, setBusy] = useState(false);
  const [pictures, setPictures] = useState<Record<string, string>>({});
  const [draft, setDraft] = useState<Sending | null>(null);
  const picker = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    try {
      const next = await commands.studioAssetList(serverId);
      setList(next);
      setDraft(next.pack?.sending ?? { mode: "off", port: 25590, address: "", required: false });
    } catch (error) {
      onError(trBack(String(error)));
    }
  }, [serverId, onError]);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    for (const one of list?.assets ?? []) {
      if (one.folder !== "textures" || !one.file.endsWith(".png") || pictures[one.file]) continue;
      void commands
        .studioAssetImage(serverId, one.file)
        .then((url) => setPictures((now) => ({ ...now, [one.file]: url })))
        .catch(() => undefined);
    }
  }, [list, serverId, pictures]);

  const packed = (pack: PackInfo) => setList((now) => (now ? { ...now, pack } : now));
  const add = async (files: File[]) => {
    if (!files.length) return;
    setBusy(true);
    let done = 0;
    for (const file of files) {
      const name = assetFileName(file.name);
      const lower = name.toLowerCase();
      const into = lower.endsWith(".ogg") ? "sounds" : lower.endsWith(".json") || lower.endsWith(".bbmodel") ? "models" : "textures";
      try {
        // A teammate's or web window sends the file to the host in pieces first.
        packed(onWeb ? await commands.studioAssetAddUpload(serverId, into, name, await uploadRaw(file, webSession())) : await commands.studioAssetAdd(serverId, into, name, await base64Of(file)));
        setPictures((now) => {
          const next = { ...now };
          delete next[name];
          return next;
        });
        done += 1;
      } catch (error) {
        onError(trBack(String(error)));
      }
    }
    setBusy(false);
    await load();
    if (done) onNotice(tr("{0, plural, one {# file added} other {# files added}}. The pack was built again.", [done]));
  };
  const remove = async (file: string) => {
    if (!(await confirm(tr("Remove {0}?", [file]), tr("Items or sounds that use it will show as missing in the game."), tr("Remove"), true))) return;
    setBusy(true);
    try {
      packed(await commands.studioAssetRemove(serverId, folder, file));
      await load();
    } catch (error) {
      onError(trBack(String(error)));
    } finally {
      setBusy(false);
    }
  };
  const saveSending = async (next: Sending) => {
    setDraft(next);
    // Picking "From an address" shows the box first; it is saved once there is an address in it.
    if (next.mode === "url" && !next.address.trim()) return;
    setBusy(true);
    try {
      packed(await commands.studioPackSending(serverId, next));
    } catch (error) {
      onError(trBack(String(error)));
    } finally {
      setBusy(false);
    }
  };

  if (!list || !draft) return <div className="studio-assets muted">{tr("Loading…")}</div>;
  const shown = list.assets.filter((one) => one.folder === folder);
  const kind = FOLDERS.find((one) => one.id === folder)!;
  const pack = list.pack;
  return (
    <div
      className="studio-assets"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        void add([...event.dataTransfer.files]);
      }}
    >
      <div className="studio-assets-head">
        <div>
          <h2>{tr("Assets and resource pack")}</h2>
          <p className="muted">{tr("Pictures, models and sounds for your own items. Drop files here; the pack is built again after every change.")}</p>
        </div>
        <button type="button" className="button small primary" disabled={busy} onClick={() => picker.current?.click()}>
          <Icon name="upload" />
          {tr("Add files")}
        </button>
        <input ref={picker} type="file" multiple hidden accept=".png,.mcmeta,.json,.bbmodel,.ogg" onChange={(event) => void add([...(event.target.files ?? [])]).finally(() => (event.target.value = ""))} />
      </div>

      <SegmentedControl value={folder} onChange={(value) => setFolder(value as typeof folder)} options={FOLDERS.map((one) => ({ value: one.id, label: `${one.label()} ${list.assets.filter((asset) => asset.folder === one.id).length}` }))} />
      <p className="muted studio-assets-hint">{kind.hint()}</p>
      {shown.length === 0 ? (
        <p className="studio-steps-empty">{tr("Nothing here yet. Drop files onto this page or use Add files.")}</p>
      ) : (
        <div className="studio-asset-grid">
          {shown.map((one) => (
            <div key={one.file} className="studio-asset">
              <span className="studio-asset-look">{pictures[one.file] ? <img src={pictures[one.file]} alt="" /> : <Icon name={folder === "sounds" ? "bell" : folder === "models" ? "box" : "image"} />}</span>
              <span className="studio-asset-name" title={one.file}>
                <strong>{one.name}</strong>
                <code>{folder === "sounds" ? `monolab:studio.${one.name}` : `monolab:studio/${one.name}`}</code>
              </span>
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} disabled={busy} onClick={() => void remove(one.file)}>
                <Icon name="trash" />
              </button>
            </div>
          ))}
        </div>
      )}

      <section className="studio-pack">
        <div className="studio-pack-head">
          <strong>{tr("Resource pack")}</strong>
          <span className="muted">
            {pack?.sha1
              ? tr("{0} · format {1} · built {2}", [size(Number(pack.bytes)), pack.format, new Date(Number(pack.built) * 1000).toLocaleString()])
              : tr("Not built yet.")}
          </span>
          <button
            type="button"
            className="button small secondary"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void commands
                .studioPackBuild(serverId)
                .then((next) => {
                  packed(next);
                  onNotice(tr("The pack was built."));
                })
                .catch((error: unknown) => onError(trBack(String(error))))
                .finally(() => setBusy(false));
            }}
          >
            <Icon name="refresh" />
            {tr("Build again")}
          </button>
        </div>
        {/* Sending the pack to players is the host's setting. */}
        {!onWeb && (
          <>
          <div className="studio-pack-send">
            <span>{tr("Send to players")}</span>
            <SegmentedControl
              value={draft.mode}
              onChange={(mode) => void saveSending({ ...draft, mode })}
              options={[
                { value: "off", label: tr("Don't send") },
                { value: "self", label: tr("From this server") },
                { value: "url", label: tr("From an address") },
              ]}
            />
          </div>
          {draft.mode === "self" && pumpkin && (
            <div className="studio-pack-fields">
              <p className="muted">{tr("Pumpkin serves the pack on the game port itself, from the address players joined with. Behind a proxy or a port forward that changes the port, set resource_pack.java.host_base_url in its settings.")}</p>
            </div>
          )}
          {draft.mode === "self" && !pumpkin && (
            <div className="studio-pack-fields">
              <label className="field">
                <span>{tr("Port")}</span>
                <input type="number" min={1024} max={65535} value={draft.port} onChange={(event) => setDraft({ ...draft, port: Number(event.target.value) || 0 })} onBlur={() => void saveSending(draft)} />
              </label>
              <label className="field">
                <span>{tr("Address players reach")}</span>
                <input value={draft.address} placeholder={tr("The one they joined with")} spellCheck={false} onChange={(event) => setDraft({ ...draft, address: event.target.value })} onBlur={() => void saveSending(draft)} />
              </label>
              <p className="muted">{tr("The add-on serves the pack on this port (open it like the game port, TCP). Players get it next to the server's own pack.")}</p>
            </div>
          )}
          {draft.mode === "url" && (
            <div className="studio-pack-fields">
              <label className="field wide">
                <span>{tr("Pack address")}</span>
                <input value={draft.address} placeholder="https://example.com/monolab.zip" spellCheck={false} onChange={(event) => setDraft({ ...draft, address: event.target.value })} onBlur={() => void saveSending(draft)} />
              </label>
              <p className="muted">
                {tr("Upload {0} there each time it changes; the game checks it against SHA-1 {1}.", ["studio/pack/monolab.zip", pack?.sha1 ? pack.sha1.slice(0, 12) : "—"])}
              </p>
            </div>
          )}
          {draft.mode !== "off" && (
            <label className="studio-toggle">
              <span>{tr("Players must accept it")}</span>
              <Switch checked={draft.required} disabled={busy} onChange={(event) => void saveSending({ ...draft, required: event.target.checked })} />
            </label>
          )}
          </>
        )}
      </section>
    </div>
  );
}
