import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { CustomProgram } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Input, SegmentedControl } from "../primitives";
import { Icon } from "../ui/Icon";

type Protocol = "tcp" | "udp" | "both";
type Draft = { program: string; args: string; ready: string; stopInput: string; stopMode: "ctrl-c" | "input"; protocol: Protocol };

const toDraft = (one: CustomProgram): Draft => ({
  program: one.program ?? "",
  args: one.args ?? "",
  ready: one.ready ?? "",
  stopInput: one.stopInput ?? "",
  stopMode: one.stopInput ? "input" : "ctrl-c",
  protocol: one.protocol === "udp" || one.protocol === "both" ? one.protocol : "tcp",
});

const toProgram = (draft: Draft): CustomProgram => ({
  program: draft.program.trim(),
  args: draft.args.trim(),
  ready: draft.ready.trim(),
  stopInput: draft.stopMode === "input" ? draft.stopInput.trim() : "",
  protocol: draft.protocol,
});

const shown = (path: string, folder: string) => {
  const base = folder.replace(/[\\/]+$/, "");
  return base && path.toLowerCase().startsWith(base.toLowerCase()) ? path.slice(base.length).replace(/^[\\/]+/, "") : path;
};

function ProgramFields({
  folder,
  draft,
  port,
  onDraft,
  onPort,
}: {
  folder: string;
  draft: Draft;
  port: string;
  onDraft: (next: Draft) => void;
  onPort: (next: string) => void;
}) {
  async function pickProgram() {
    const picked = await open({ multiple: false, defaultPath: folder || undefined, title: tr("Choose the program to run") });
    if (typeof picked === "string") onDraft({ ...draft, program: shown(picked, folder) });
  }
  return (
    <>
      <div className="create-two">
        <label className="create-field">
          <span>{tr("Program")}</span>
          <span className="program-pick">
            <Input value={draft.program} placeholder="ruby\bin\ruby.exe" spellCheck={false} onChange={(event) => onDraft({ ...draft, program: event.target.value })} />
            <button type="button" className="button secondary small" disabled={!folder} onClick={() => void pickProgram()}>
              <Icon name="folder" />
              {tr("Choose")}
            </button>
          </span>
        </label>
        <label className="create-field">
          <span>{tr("Arguments")}</span>
          <Input value={draft.args} placeholder="Server.rb" spellCheck={false} onChange={(event) => onDraft({ ...draft, args: event.target.value })} />
        </label>
      </div>
      <div className="create-two">
        <div className="create-field">
          <span>{tr("Port")}</span>
          <span className="program-stop">
            <Input value={port} inputMode="numeric" placeholder="12345" onChange={(event) => onPort(event.target.value.replace(/\D/g, "").slice(0, 5))} />
            <SegmentedControl
              value={draft.protocol}
              onChange={(protocol) => onDraft({ ...draft, protocol: protocol as Protocol })}
              options={[
                { value: "tcp", label: "TCP" },
                { value: "udp", label: "UDP" },
                { value: "both", label: "TCP+UDP" },
              ]}
            />
          </span>
        </div>
        <label className="create-field">
          <span>{tr("Ready text (optional)")}</span>
          <Input value={draft.ready} placeholder="Server started" spellCheck={false} onChange={(event) => onDraft({ ...draft, ready: event.target.value })} />
        </label>
      </div>
      <div className="create-field">
        <span>{tr("How to stop")}</span>
        <span className="program-stop">
          <SegmentedControl
            value={draft.stopMode}
            onChange={(stopMode) => onDraft({ ...draft, stopMode: stopMode as Draft["stopMode"] })}
            options={[
              { value: "ctrl-c", label: "Ctrl+C" },
              { value: "input", label: tr("Type a command") },
            ]}
          />
          {draft.stopMode === "input" && (
            <Input value={draft.stopInput} placeholder="exit" spellCheck={false} onChange={(event) => onDraft({ ...draft, stopInput: event.target.value })} />
          )}
        </span>
      </div>
    </>
  );
}

const portOf = (text: string) => Number(text) || 0;

/** Create step: pick a folder that already holds a server program, then how to run it. */
export function CustomCreate({ onCreate }: { onCreate: (folder: string, port: number, program: CustomProgram) => Promise<boolean> }) {
  const [folder, setFolder] = useState("");
  const [draft, setDraft] = useState<Draft>({ program: "", args: "", ready: "", stopInput: "", stopMode: "ctrl-c", protocol: "tcp" });
  const [port, setPort] = useState("");
  const [busy, setBusy] = useState(false);

  async function pickFolder() {
    const picked = await open({ directory: true, multiple: false, title: tr("Choose the server folder") });
    if (typeof picked === "string") setFolder(picked);
  }
  async function submit() {
    setBusy(true);
    await onCreate(folder, portOf(port), toProgram(draft));
    setBusy(false);
  }
  const ready = folder && draft.program.trim() && portOf(port) > 0 && (draft.stopMode === "ctrl-c" || draft.stopInput.trim());
  return (
    <>
      <div className="create-field">
        <span>{tr("Server folder")}</span>
        <button type="button" className="program-folder" onClick={() => void pickFolder()}>
          <Icon name="folder" />
          <span>{folder || tr("Choose the folder that has the server program")}</span>
        </button>
      </div>
      {folder && <ProgramFields folder={folder} draft={draft} port={port} onDraft={setDraft} onPort={setPort} />}
      <div className="create-actions">
        <button className="button primary" disabled={!ready || busy} onClick={() => void submit()}>
          <Icon name="plus" />
          {tr("Add")}
        </button>
      </div>
    </>
  );
}

/** Overview card: edit how a custom program is run. Applies on the next start. */
export function CustomProgramCard({
  id,
  folder,
  port,
  onError,
  onNotice,
}: {
  id: string;
  folder: string;
  port: number;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const [saved, setSaved] = useState<{ draft: Draft; port: string } | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [portText, setPortText] = useState(String(port));
  const [busy, setBusy] = useState(false);
  const errorRef = useRef(onError);
  errorRef.current = onError;

  useEffect(() => {
    setSaved(null);
    commands
      .readCustomProgram(id)
      .then((one) => {
        const next = toDraft(one);
        setSaved({ draft: next, port: String(port) });
        setDraft(next);
        setPortText(String(port));
      })
      .catch((cause) => {
        errorRef.current(String(cause));
        const empty: Draft = { program: "", args: "", ready: "", stopInput: "", stopMode: "ctrl-c", protocol: "tcp" };
        setSaved({ draft: empty, port: String(port) });
        setDraft(empty);
      });
  }, [id, port]);

  if (!saved || !draft) return <div className="setting-card"><div className="skeleton-row" /></div>;
  const changed = JSON.stringify(toProgram(draft)) !== JSON.stringify(toProgram(saved.draft)) || portText !== saved.port;

  async function save() {
    if (!draft) return;
    setBusy(true);
    try {
      await commands.saveCustomProgram(id, portOf(portText), toProgram(draft));
      setSaved({ draft, port: portText });
      onNotice(tr("Saved the program settings. They apply on the next start."));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="setting-card custom-program">
      <span className="setting-title">{tr("Launch settings")}</span>
      <ProgramFields folder={folder} draft={draft} port={portText} onDraft={setDraft} onPort={setPortText} />
      <span className="title-actions">
        <button type="button" className="button secondary small" disabled={busy || !changed} onClick={() => void save()}>
          <Icon name="check" />
          {tr("Save")}
        </button>
      </span>
    </div>
  );
}
