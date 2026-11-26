/** SHA-256 of a text as lowercase hex, the same as the app's `studio::hash` (UTF-8 of the string). */
export async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** The host refused a save because someone saved the file after it was opened (F2). */
export function isSavedOver(error: unknown): boolean {
  return String(error).includes("saved this file after you opened it");
}
