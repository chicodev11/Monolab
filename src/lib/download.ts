import { call } from "./bridge";

/** Piece of a download the host sends at a time is its own; this only asks for the next until it is empty. */
type Begun = { download: string; size: number; name: string };

/**
 * Downloads a file the host offers to a teammate's window (F6: a Storage file, or a studio bundle the host makes),
 * piece by piece (`__download_begin` / `__download_part`), and saves it through the browser.
 * `onProgress` gets the fraction done.
 */
export async function downloadFromHost(begin: Record<string, unknown>, onProgress?: (done: number) => void): Promise<string> {
  const { download, size, name } = await call<Begun>("__download_begin", begin);
  const parts: Uint8Array[] = [];
  let at = 0;
  for (;;) {
    const { data } = await call<{ data: string }>("__download_part", { download, at });
    if (!data) break;
    const raw = atob(data);
    const bytes = new Uint8Array(raw.length);
    for (let index = 0; index < raw.length; index += 1) bytes[index] = raw.charCodeAt(index);
    parts.push(bytes);
    at += bytes.length;
    onProgress?.(size ? at / size : 1);
  }
  const url = URL.createObjectURL(new Blob(parts as BlobPart[]));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
  return name;
}
