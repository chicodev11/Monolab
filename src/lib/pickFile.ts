
import { call, onMonolink } from "./bridge";
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

/** Piece size for sending a file through Monolink (the host accepts up to 4 MB). */
const MONOLINK_PART = 1024 * 1024;

function base64Of(bytes: Uint8Array) {
  let text = "";
  for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(text);
}

/** Sends a file to the host in pieces over Monolink; gives the same token as the web panel's upload. */
async function uploadViaMonolink(file: File): Promise<string> {
  const { upload } = await call<{ upload: string }>("__upload_begin", { name: file.name, size: file.size });
  for (let at = 0; at < file.size; at += MONOLINK_PART) {
    const piece = new Uint8Array(await file.slice(at, at + MONOLINK_PART).arrayBuffer());
    await call("__upload_part", { upload, at, data: base64Of(piece) });
  }
  return call<string>("__upload_end", { upload });
}

export async function uploadRaw(file: File, session: string): Promise<string> {
  if (onMonolink) return uploadViaMonolink(file);
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
