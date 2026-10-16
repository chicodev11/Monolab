
import { tr } from "./i18n";
export type PickedFile = { name: string; data: string };

export function pickFiles(accept: string, multiple = false): Promise<PickedFile[]> {
  return new Promise((done) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.oncancel = () => done([]);
    input.onchange = async () => {
      const files = [...(input.files ?? [])];
      done(await Promise.all(files.map(read)));
    };
    input.click();
  });
}

export function pickRawFiles(accept: string, multiple = false): Promise<File[]> {
  return new Promise((done) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.oncancel = () => done([]);
    input.onchange = () => done([...(input.files ?? [])]);
    input.click();
  });
}

export async function uploadRaw(file: File, session: string): Promise<string> {
  const answer = await fetch(`/upload?name=${encodeURIComponent(file.name)}`, {
    method: "POST",
    headers: { "X-SE-Session": session, "content-type": "application/octet-stream" },
    body: file,
  });
  const body = (await answer.json().catch(() => ({}))) as { value?: string; error?: string };
  if (!answer.ok || !body.value) throw new Error(body.error ?? tr("Couldn't upload {0}.", [file.name]));
  return body.value;
}

function read(file: File): Promise<PickedFile> {
  return new Promise((done, fail) => {
    const reader = new FileReader();
    reader.onerror = () => fail(new Error(tr("Couldn't read {0}.", [file.name])));
    reader.onload = () => {
      const text = String(reader.result);
      done({ name: file.name, data: text.slice(text.indexOf(",") + 1) });
    };
    reader.readAsDataURL(file);
  });
}

export function offerDownload(name: string, data: string, type = "application/octet-stream") {
  const raw = atob(data);
  const bytes = new Uint8Array(raw.length);
  for (let at = 0; at < raw.length; at += 1) bytes[at] = raw.charCodeAt(at);
  const url = URL.createObjectURL(new Blob([bytes], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
