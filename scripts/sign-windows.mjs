// Signs one Windows file (the app exe, the NSIS installer and its uninstaller) with signtool.
// Tauri calls it through src-tauri/tauri.sign.conf.json as `node scripts/sign-windows.mjs <file>`.
//
// The certificate comes from one of:
//   WINDOWS_CERTIFICATE (a .pfx as base64) + WINDOWS_CERTIFICATE_PASSWORD — CI secrets;
//   WINDOWS_CERTIFICATE_THUMBPRINT — a certificate already in the Windows store (a USB token, say).
// SIGNTOOL can point at signtool.exe; otherwise the newest Windows SDK one is used.
// WINDOWS_TIMESTAMP_URL overrides the timestamp server.
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const file = process.argv[2];
if (!file || !existsSync(file)) {
  console.error(`sign-windows: no file to sign (${file ?? "none given"})`);
  process.exit(1);
}

function findSigntool() {
  if (process.env.SIGNTOOL && existsSync(process.env.SIGNTOOL)) return process.env.SIGNTOOL;
  for (const base of ["C:\\Program Files (x86)\\Windows Kits\\10\\bin", "C:\\Program Files\\Windows Kits\\10\\bin"]) {
    if (!existsSync(base)) continue;
    const versions = readdirSync(base)
      .filter((name) => /^10\.\d+\.\d+\.\d+$/.test(name))
      .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
    for (const version of versions) {
      const tool = join(base, version, "x64", "signtool.exe");
      if (existsSync(tool)) return tool;
    }
  }
  return null;
}

const signtool = findSigntool();
if (!signtool) {
  console.error("sign-windows: signtool.exe not found. Install the Windows SDK or set SIGNTOOL.");
  process.exit(1);
}

const timestamp = process.env.WINDOWS_TIMESTAMP_URL || "http://timestamp.digicert.com";
const args = ["sign", "/fd", "SHA256", "/tr", timestamp, "/td", "SHA256", "/d", "Monolab"];
let scratch = null;

if (process.env.WINDOWS_CERTIFICATE) {
  scratch = mkdtempSync(join(tmpdir(), "monolab-sign-"));
  const pfx = join(scratch, "cert.pfx");
  writeFileSync(pfx, Buffer.from(process.env.WINDOWS_CERTIFICATE, "base64"));
  args.push("/f", pfx);
  if (process.env.WINDOWS_CERTIFICATE_PASSWORD) args.push("/p", process.env.WINDOWS_CERTIFICATE_PASSWORD);
} else if (process.env.WINDOWS_CERTIFICATE_THUMBPRINT) {
  args.push("/sha1", process.env.WINDOWS_CERTIFICATE_THUMBPRINT);
} else {
  console.error("sign-windows: set WINDOWS_CERTIFICATE (+ WINDOWS_CERTIFICATE_PASSWORD) or WINDOWS_CERTIFICATE_THUMBPRINT.");
  process.exit(1);
}
args.push(file);

let status = 1;
// A timestamp server sometimes refuses for a moment; try three times.
for (let attempt = 1; attempt <= 3 && status !== 0; attempt++) {
  status = spawnSync(signtool, args, { stdio: "inherit" }).status ?? 1;
  if (status !== 0 && attempt < 3) console.warn(`sign-windows: attempt ${attempt} failed, trying again`);
}
if (scratch) rmSync(scratch, { recursive: true, force: true });
process.exit(status);
