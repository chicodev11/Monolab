import { useEffect, useState } from "react";
import { commands } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { PluginConfig } from "./PluginConfig";

type ConfigFile = { path: string; name: string };

const CONFIG = /\.(ya?ml|toml)$/i;

/** Matching key for a plugin or mod name, so "Geyser-Spigot", "geyser_spigot" and "GeyserSpigot" meet. */
export function iconKey(name: string) {
  return name.toLowerCase().replace(/\.(ya?ml|toml)$/, "").replace(/[-_](common|server|client)$/, "").replace(/[^a-z0-9]/g, "");
}

/** Finds plugin config.yml files (or mod configs in config/) and edits the chosen one as a form. */
export function PluginSettings({ serverId, mods, running, icons, onError }: {
  serverId: string;
  mods: boolean;
  running: boolean;
  /** Installed plugin or mod icons by iconKey(name). */
  icons: Record<string, string>;
  onError: (message: string) => void;
}) {
  const [files, setFiles] = useState<ConfigFile[] | null>(null);
  const [picked, setPicked] = useState("");
  const [filter, setFilter] = useState("");

  useEffect(() => {
    let gone = false;
    const list = (path: string) => commands.browseServerFiles(serverId, path, null, null).then((page) => page.entries).catch(() => []);
    const look = async () => {
      const out: ConfigFile[] = [];
      if (mods) {
        const top = await list("config");
        for (const one of top) if (!one.isDir && CONFIG.test(one.name) && !/^monolab/i.test(one.name)) out.push({ path: one.path, name: one.name });
        const folders = top.filter((one) => one.isDir && !/^(monolab|\..*)$/i.test(one.name)).slice(0, 40);
        const inner = await Promise.all(folders.map((one) => list(one.path)));
        inner.forEach((entries, at) => {
          for (const one of entries) if (!one.isDir && CONFIG.test(one.name)) out.push({ path: one.path, name: `${folders[at].name}/${one.name}` });
        });
      } else {
        const folders = (await list("plugins")).filter((one) => one.isDir && !/^(monolab|\..*)$/i.test(one.name)).slice(0, 80);
        const inner = await Promise.all(folders.map((one) => list(one.path)));
        inner.forEach((entries, at) => {
          const file = entries.find((one) => !one.isDir && one.name.toLowerCase() === "config.yml");
          if (file) out.push({ path: file.path, name: folders[at].name });
        });
      }
      out.sort((a, b) => a.name.localeCompare(b.name));
      if (!gone) setFiles(out);
    };
    void look();
    return () => {
      gone = true;
    };
  }, [serverId, mods]);

  const needle = filter.trim().toLowerCase();
  const shown = (files ?? []).filter((one) => !needle || one.name.toLowerCase().includes(needle));
  const current = files?.find((one) => one.path === picked) ?? files?.[0];
  return (
    <div className="integ-layout plugin-settings">
      <nav className="integ-nav" aria-label={mods ? tr("Mod settings") : tr("Plugin settings")}>
        {(files?.length ?? 0) > 8 && (
          <label className="search integ-nav-search">
            <Icon name="search" />
            <input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder={tr("Search")} />
          </label>
        )}
        {files === null ? (
          <p className="integ-nav-empty">{tr("Loading…")}</p>
        ) : files.length === 0 ? (
          <p className="integ-nav-empty">{mods ? tr("No mod config files found") : tr("No config.yml found")}</p>
        ) : (
          shown.map((one) => (
            <button key={one.path} type="button" className={current?.path === one.path ? "on" : ""} aria-current={current?.path === one.path ? "page" : undefined} onClick={() => setPicked(one.path)}>
              {icons[iconKey(one.name.split("/")[0])] ? <img className="integ-nav-icon" src={icons[iconKey(one.name.split("/")[0])]} alt="" /> : <Icon name="file-cog" />}
              <span>{one.name}</span>
            </button>
          ))
        )}
      </nav>
      <section className="integ-main">
        {current && (
          <PluginConfig key={current.path} serverId={serverId} path={current.path} name={current.name} running={running} onError={onError} />
        )}
      </section>
    </div>
  );
}
