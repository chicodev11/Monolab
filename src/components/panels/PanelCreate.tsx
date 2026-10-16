import { useEffect, useMemo, useState } from "react";
import type { PanelAllocation, PanelCreateOptions } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Dropdown } from "../ui/Dropdown";

export function PanelCreate({
  hostId,
  onDone,
  onCancel,
  onError,
  onNotice,
}: {
  hostId: string;
  onDone: () => void;
  onCancel: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const [options, setOptions] = useState<PanelCreateOptions | null>(null);
  const [name, setName] = useState("");
  const [owner, setOwner] = useState("");
  const [egg, setEgg] = useState("");
  const [image, setImage] = useState("");
  const [memory, setMemory] = useState("4096");
  const [disk, setDisk] = useState("10240");
  const [cpu, setCpu] = useState("0");
  const [node, setNode] = useState("");
  const [allocations, setAllocations] = useState<PanelAllocation[] | null>(null);
  const [allocation, setAllocation] = useState("");
  const [env, setEnv] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    commands
      .panelCreateOptions(hostId)
      .then((got) => {
        setOptions(got);
        setOwner(String(got.me || got.users[0]?.id || ""));
        const first = got.eggs.find((one) => /paper/i.test(one.name)) ?? got.eggs[0];
        if (first) setEgg(String(first.id));
        if (got.nodes[0]) setNode(String(got.nodes[0].id));
      })
      .catch((cause) => onError(String(cause)));
  }, [hostId, onError]);

  const chosenEgg = useMemo(() => options?.eggs.find((one) => String(one.id) === egg), [options, egg]);

  useEffect(() => {
    if (!chosenEgg) return;
    const javaOf = (label: string) => Number(label.replace(/\D+/g, " ").trim().split(" ").pop()) || 0;
    const newest = [...chosenEgg.images].sort((a, b) => javaOf(b.label) - javaOf(a.label))[0];
    setImage(newest?.image ?? "");
    setEnv(Object.fromEntries(chosenEgg.variables.map((one) => [one.env, one.defaultValue])));
  }, [chosenEgg]);

  useEffect(() => {
    if (!node) return;
    setAllocations(null);
    setAllocation("");
    commands
      .panelFreeAllocations(hostId, Number(node))
      .then((got) => {
        setAllocations(got);
        if (got[0]) setAllocation(String(got[0].id));
      })
      .catch((cause) => onError(String(cause)));
  }, [hostId, node, onError]);

  async function create() {
    if (!chosenEgg) return;
    setBusy(true);
    try {
      const made = await commands.panelCreateServer(hostId, {
        name,
        owner: Number(owner),
        egg: chosenEgg.id,
        dockerImage: image,
        startup: chosenEgg.startup,
        environment: env,
        memoryMib: Number(memory) || 0,
        diskMib: Number(disk) || 0,
        cpu: Number(cpu) || 0,
        allocation: Number(allocation),
      });
      onNotice(
        tr("Created {0}{1}. Installation by the panel may take a moment.", [name.trim(), made ? ` (${made})` : ""]),
      );
      onDone();
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  if (!options) {
    return <p className="panel-hint">{tr("Loading eggs and nodes from the panel…")}</p>;
  }
  if (!options.eggs.length || !options.nodes.length) {
    return (
      <p className="panel-hint">
        {tr("No eggs or nodes found. Create them in the panel's admin area first.")}
      </p>
    );
  }

  const ready = name.trim() && owner && chosenEgg && image && allocation && !busy;

  return (
    <div className="panel-form panel-create">
      <label className="field-block">
        <span>{tr("Server name")}</span>
        <input value={name} placeholder={tr("New server")} onChange={(event) => setName(event.target.value)} />
      </label>
      <div className="panel-create-row">
        <div className="field-block">
          <span>{tr("Egg")}</span>
          <Dropdown
            value={egg}
            search={options.eggs.length > 8 ? tr("Find an egg") : undefined}
            options={options.eggs.map((one) => ({ value: String(one.id), label: `${one.nest} · ${one.name}` }))}
            onChange={setEgg}
          />
        </div>
        <div className="field-block">
          <span>{tr("Owner")}</span>
          <Dropdown
            value={owner}
            search={options.users.length > 8 ? tr("Find a user") : undefined}
            options={options.users.map((one) => ({
              value: String(one.id),
              label: one.id === options.me ? tr("{0} (me)", [one.username]) : one.username,
              hint: one.email,
            }))}
            onChange={setOwner}
          />
        </div>
      </div>
      {chosenEgg && chosenEgg.images.length > 1 && (
        <div className="field-block">
          <span>{tr("Docker image")}</span>
          <Dropdown
            value={image}
            options={chosenEgg.images.map((one) => ({ value: one.image, label: one.label, hint: one.image }))}
            onChange={setImage}
          />
        </div>
      )}
      <div className="panel-create-row three">
        <label className="field-block">
          <span>{tr("Memory (MiB)")}</span>
          <input inputMode="numeric" value={memory} onChange={(event) => setMemory(event.target.value.replace(/\D/g, ""))} />
        </label>
        <label className="field-block">
          <span>{tr("Disk (MiB)")}</span>
          <input inputMode="numeric" value={disk} onChange={(event) => setDisk(event.target.value.replace(/\D/g, ""))} />
        </label>
        <label className="field-block">
          <span>{tr("CPU % (0 = unlimited)")}</span>
          <input inputMode="numeric" value={cpu} onChange={(event) => setCpu(event.target.value.replace(/\D/g, ""))} />
        </label>
      </div>
      <div className="panel-create-row">
        <div className="field-block">
          <span>{tr("Node")}</span>
          <Dropdown
            value={node}
            options={options.nodes.map((one) => ({ value: String(one.id), label: one.name }))}
            onChange={setNode}
          />
        </div>
        <div className="field-block">
          <span>{tr("Address:port")}</span>
          <Dropdown
            value={allocation}
            disabled={!allocations?.length}
            placeholder={
              allocations === null ? tr("Loading…") : tr("No free ports")
            }
            search={(allocations?.length ?? 0) > 8 ? tr("Find a port") : undefined}
            options={(allocations ?? []).map((one) => ({
              value: String(one.id),
              label: `${one.alias || one.ip}:${one.port}`,
            }))}
            onChange={setAllocation}
          />
        </div>
      </div>
      {chosenEgg && chosenEgg.variables.length > 0 && (
        <details className="panel-create-vars">
          <summary>{tr("{0, plural, one {# egg variable} other {# egg variables}}", [chosenEgg.variables.length])}</summary>
          {chosenEgg.variables.map((one) => (
            <label key={one.env} className="field-block">
              <span>
                {one.name || one.env} <code>{one.env}</code>
              </span>
              <input
                value={env[one.env] ?? ""}
                spellCheck={false}
                title={one.description}
                onChange={(event) => setEnv({ ...env, [one.env]: event.target.value })}
              />
            </label>
          ))}
        </details>
      )}
      <div className="panel-form-actions">
        <button className="button primary small" disabled={!ready} onClick={() => void create()}>
          {busy ? tr("Creating…") : tr("Create server")}
        </button>
        <button className="button ghost small" disabled={busy} onClick={onCancel}>
          {tr("Cancel")}
        </button>
      </div>
    </div>
  );
}
