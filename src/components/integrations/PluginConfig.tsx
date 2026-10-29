import { lazy, Suspense, useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
import { commands } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { readTomlFields, readYamlFields, setTomlField, setYamlField, type YamlField } from "../../lib/yamlForm";
import { Card } from "../primitives";
import { Icon } from "../ui/Icon";

const CodeEditor = lazy(() => import("../files/CodeEditor").then((module) => ({ default: module.CodeEditor })));

/** A plugin's config.yml as a form (switches, number and text fields) or as plain text in the code editor. */
export function PluginConfig({ serverId, path, name, running, onError }: {
  serverId: string;
  path: string;
  name: string;
  running: boolean;
  onError: (message: string) => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [saved, setSaved] = useState(false);
  const [mode, setMode] = useState<"form" | "text">("form");
  const [edited, setEdited] = useState<string | null>(null);
  const load = useCallback(() => {
    setText(null);
    setEdited(null);
    commands.readEditableFile(serverId, path).then(setText).catch((cause) => onError(String(cause)));
  }, [serverId, path, onError]);
  useEffect(load, [load]);

  const toml = /\.toml$/i.test(path);
  const fields = useMemo(() => (text ? (toml ? readTomlFields(text) : readYamlFields(text)) : []), [text, toml]);
  const needle = query.trim().toLowerCase();
  const shown = needle ? fields.filter((one) => one.path.toLowerCase().includes(needle) || one.hint.toLowerCase().includes(needle) || one.value.toLowerCase().includes(needle)) : fields;
  const groups = useMemo(() => {
    const out = new Map<string, YamlField[]>();
    for (const one of shown) out.set(one.section, [...(out.get(one.section) ?? []), one]);
    return [...out.entries()];
  }, [shown]);

  async function write(next: string) {
    try {
      await commands.saveEditableFile(serverId, path, next);
      setText(next);
      setEdited(null);
      setSaved(true);
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function save(field: YamlField, value: string) {
    if (text === null || value === field.value) return;
    await write(toml ? setTomlField(text, field, value) : setYamlField(text, field, value));
  }

  const dirty = edited !== null;
  if (text === null) return <p className="muted lag-note">{tr("Loading…")}</p>;
  return (
    <div className="plugin-config">
      <div className="plugin-config-head">
        <div>
          <strong>{name}</strong>
          <small>{path}</small>
        </div>
        <span className="live-spacer" />
        {mode === "form" ? (
          <label className="search integ-config-search">
            <Icon name="search" />
            <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={tr("Search settings")} />
          </label>
        ) : (
          dirty && (
            <>
              <button type="button" className="button ghost small" onClick={() => setEdited(null)}>
                <Icon name="undo" />
                {tr("Roll back")}
              </button>
              <button type="button" className="button primary small" onClick={() => void write(edited ?? text)} data-tip="Ctrl+S">
                {tr("Save")}
              </button>
            </>
          )
        )}
        <div className="wb-seg slide" role="tablist" aria-label={tr("View")} style={{ "--at": mode === "text" ? 1 : 0 } as CSSProperties}>
          {([["form", tr("Fields")], ["text", tr("Editor")]] as const).map(([value, label]) => (
            <button key={value} type="button" role="tab" aria-selected={mode === value} className={mode === value ? "on" : ""} onClick={() => setMode(value)}>
              {label}
            </button>
          ))}
        </div>
      </div>
      {saved && running && <p className="plugin-config-note">{tr("Saved. Reload the plugin or restart the server to apply.")}</p>}
      {mode === "text" && (
        <div className="plugin-config-editor">
          <Suspense fallback={null}>
            <CodeEditor
              path={path}
              value={edited ?? text}
              readOnly={false}
              onChange={(next) => setEdited(next === text ? null : next)}
              onSave={() => {
                if (edited !== null) void write(edited);
              }}
              onCursor={() => undefined}
            />
          </Suspense>
          <div className="ide-status">
            {dirty ? (
              <span className="ide-status-dirty">
                <i className="ide-dot" aria-hidden />
                {tr("Unsaved changes")}
              </span>
            ) : (
              <span>{tr("Saved")}</span>
            )}
            <span className="ide-status-gap" />
            <span>{(edited ?? text).includes("\r\n") ? "CRLF" : "LF"}</span>
          </div>
        </div>
      )}
      {mode === "form" && fields.length === 0 && <p className="muted lag-note">{tr("No settings to show in this file.")}</p>}
      {mode === "form" && groups.map(([section, rows]) => (
        <Card key={section || "-"} className="plugin-config-group" title={section || tr("General")}>
          {rows.map((field) => {
            const draft = drafts[field.path] ?? field.value;
            const label = field.section ? field.path.slice(field.section.length + 1) : field.path;
            return (
              <div className="setting-row" key={field.path}>
                <div>
                  <strong className="plugin-config-key">{label}</strong>
                  {field.hint && <small title={field.hint}>{field.hint}</small>}
                </div>
                {field.kind === "bool" ? (
                  <label className="switch">
                    <input type="checkbox" checked={field.value === "true"} onChange={(event) => void save(field, String(event.target.checked))} />
                    <span />
                  </label>
                ) : field.kind === "list" || field.kind === "block" ? (
                  <button type="button" className="button ghost small" onClick={() => setMode("text")}>
                    {field.kind === "list" ? tr("{0, plural, one {# item} other {# items}}", [field.items]) : tr("Several lines")}
                    <Icon name="arrow" />
                  </button>
                ) : (
                  <input
                    className={`integ-value ${field.kind === "text" ? "wide" : ""}`}
                    value={draft}
                    inputMode={field.kind === "number" ? "decimal" : undefined}
                    onChange={(event) => setDrafts((now) => ({ ...now, [field.path]: event.target.value }))}
                    onKeyDown={(event) => event.key === "Enter" && (event.target as HTMLInputElement).blur()}
                    onBlur={() => {
                      setDrafts((now) => {
                        const rest = { ...now };
                        delete rest[field.path];
                        return rest;
                      });
                      if (field.kind === "number" && !/^-?\d+(\.\d+)?$/.test(draft.trim())) {
                        onError(tr("Enter a number."));
                        return;
                      }
                      void save(field, field.kind === "number" ? draft.trim() : draft);
                    }}
                  />
                )}
              </div>
            );
          })}
        </Card>
      ))}
    </div>
  );
}
