import { readFileSync, writeFileSync } from "node:fs";
import type { Plugin } from "vite";

const langDir = new URL("../src/lang/", import.meta.url);
let lastWrite = 0;

// Dev only: the in-app text editor saves a changed value into src/lang/<lang>.json.
export function langEditor(): Plugin {
  return {
    name: "monolab-lang-editor",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/__lang/set", (req, res) => {
        const reply = (status: number, message: string) => {
          res.statusCode = status;
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify({ message }));
        };
        if (req.method !== "POST") return reply(405, "POST only");
        let body = "";
        req.on("data", (chunk) => (body += chunk));
        req.on("end", () => {
          try {
            const { lang, key, value } = JSON.parse(body) as { lang: unknown; key: unknown; value: unknown };
            if (typeof lang !== "string" || !/^[a-z]+(-[A-Za-z]+)?$/.test(lang)) return reply(400, "bad language");
            if (typeof key !== "string" || typeof value !== "string") return reply(400, "bad entry");
            const file = new URL(lang + ".json", langDir);
            const doc = JSON.parse(readFileSync(file, "utf8")) as Record<string, unknown>;
            const section = Object.values(doc).find(
              (one): one is Record<string, string> => typeof one === "object" && one !== null && key in one,
            );
            if (!section) return reply(404, "key not found");
            section[key] = value;
            lastWrite = Date.now();
            writeFileSync(file, JSON.stringify(doc, null, 2) + "\n");
            reply(200, "ok");
          } catch (error) {
            reply(500, String(error));
          }
        });
      });
    },
    handleHotUpdate(ctx) {
      if (ctx.file.replace(/\\/g, "/").includes("/src/lang/") && Date.now() - lastWrite < 3000) return [];
    },
  };
}
