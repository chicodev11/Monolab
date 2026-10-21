import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../../lib/ipc";
import { onWeb, webSession } from "../../lib/bridge";
import { pickRawFiles, uploadRaw } from "../../lib/pickFile";
import { tr, trBack } from "../../lib/i18n";
import { fileSize } from "../../lib/format";
import type { BrowseEntry, CatalogHit } from "../../lib/bindings";
import { Icon } from "../ui/Icon";
import { EmptyState } from "../primitives";

type Props = {
  serverId: string;
  query: string;
  onNotice: (message: string) => void;
};

function compact(value: number) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(value >= 10_000_000 ? 0 : 1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(value >= 10_000 ? 0 : 1)}k`;
  return String(value);
}

export function DatapackPanel({ serverId, query, onNotice }: Props) {
  const [folder, setFolder] = useState<string | null>(null);
  const [installed, setInstalled] = useState<BrowseEntry[]>([]);
  const [hits, setHits] = useState<CatalogHit[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    async (where: string) => {
      try {
        const page = await commands.browseServerFiles(serverId, where, null, null);
        setInstalled(page.entries.filter((one) => one.isDir || one.name.toLowerCase().endsWith(".zip")));
      } catch {
        setInstalled([]);
      }
    },
    [serverId],
  );

  useEffect(() => {
    let alive = true;
    commands.datapackFolder(serverId).then(
      (where) => {
        if (!alive) return;
        setFolder(where);
        void refresh(where);
      },
      (cause) => alive && setError(trBack(String(cause))),
    );
    return () => {
      alive = false;
    };
  }, [serverId, refresh]);

  useEffect(() => {
    let alive = true;
    setHits(null);
    const timer = window.setTimeout(() => {
      commands.searchDatapacks(serverId, query, 0).then(
        (page) => alive && setHits(page.hits),
        (cause) => {
          if (!alive) return;
          setHits([]);
          setError(trBack(String(cause)));
        },
      );
    }, 250);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [serverId, query]);

  const install = async (hit: CatalogHit) => {
    setBusy(hit.projectId);
    setError(null);
    try {
      const name = await commands.installDatapack(serverId, hit.projectId);
      onNotice(tr("Installed {0}. Run /reload or restart the server to load it.", [name]));
      if (folder) await refresh(folder);
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(null);
    }
  };

  const addFiles = async () => {
    if (!folder) return;
    setBusy("file");
    setError(null);
    try {
      if (onWeb) {
        const files = await pickRawFiles(".zip", true);
        if (!files.length) return;
        const tokens: string[] = [];
        for (const file of files) tokens.push(await uploadRaw(file, webSession()));
        await commands.addServerUploads(serverId, folder, tokens);
      } else {
        const picked = await open({ multiple: true, title: tr("Add a datapack"), filters: [{ name: tr("Datapack"), extensions: ["zip"] }] });
        if (!picked) return;
        await commands.addServerFiles(serverId, folder, Array.isArray(picked) ? picked : [picked], false);
      }
      onNotice(tr("Added. Run /reload or restart the server to load it."));
      await refresh(folder);
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(null);
    }
  };

  const remove = async (entry: BrowseEntry) => {
    setBusy(entry.path);
    try {
      await commands.trashServerPath(serverId, entry.path);
      setInstalled((list) => list.filter((one) => one.path !== entry.path));
      onNotice(tr("Moved {0} to the trash.", [entry.name]));
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="browse-main datapacks">
      {error && <p className="modal-error">{error}</p>}
      <section className="dp-installed">
        <header>
          <h3>{tr("In this world")}</h3>
          <button className="button secondary small" disabled={!folder || busy === "file"} onClick={() => void addFiles()}>
            <Icon name="plus" />
            {tr("Add from file")}
          </button>
        </header>
        {installed.length ? (
          <ul>
            {installed.map((one) => (
              <li key={one.path}>
                <Icon name={one.isDir ? "folder" : "archive"} />
                <span>{one.name.replace(/\.zip$/i, "")}</span>
                {!one.isDir && <small>{fileSize(one.sizeBytes)}</small>}
                <button className="icon-button" aria-label={tr("Remove")} disabled={busy === one.path} onClick={() => void remove(one)}>
                  <Icon name="trash" />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="muted dp-none">{tr("No datapacks yet. Datapacks change how the world plays without a mod or plugin.")}</p>
        )}
        <p className="muted dp-note">{tr("A datapack is read when the world loads. Run /reload or restart the server after adding or removing one.")}</p>
      </section>
      <section>
        <h3 className="dp-title">{tr("Find datapacks")}</h3>
        {hits === null ? (
          <div className="catalog-grid">
            {[1, 2, 3, 4].map((k) => (
              <div className="catalog-card skeleton" key={k} />
            ))}
          </div>
        ) : hits.length ? (
          <div className="catalog-grid">
            {hits.map((hit) => (
              <article className="catalog-card" key={hit.projectId}>
                {hit.iconUrl ? (
                  <img className="catalog-icon" src={hit.iconUrl} alt="" loading="lazy" />
                ) : (
                  <span className="catalog-icon empty">
                    <Icon name="archive" />
                  </span>
                )}
                <div className="catalog-main">
                  <div className="catalog-title">
                    <strong>{hit.title}</strong>
                    {hit.author && <span>{tr("by {0}", [hit.author])}</span>}
                  </div>
                  <p>{hit.description}</p>
                  <div className="catalog-facts">
                    {hit.downloads != null && (
                      <span>
                        <Icon name="import" />
                        {compact(hit.downloads)}
                      </span>
                    )}
                    {hit.follows != null && (
                      <span>
                        <Icon name="heart" />
                        {compact(hit.follows)}
                      </span>
                    )}
                  </div>
                </div>
                <div className="catalog-side">
                  <span />
                  <button className="button primary small" disabled={!!busy} onClick={() => void install(hit)}>
                    {busy === hit.projectId ? tr("Installing…") : tr("Install")}
                  </button>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <EmptyState icon="search" title={tr("No matching datapack found.")} />
        )}
      </section>
    </div>
  );
}
