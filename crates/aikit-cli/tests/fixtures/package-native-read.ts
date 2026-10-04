import { readFile, stat, realpath } from "node:fs/promises";
import { resolve, relative, isAbsolute, sep } from "node:path";
import { Type } from "typebox";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

// Real filesystem tool used by the native package conformance gate. It needs
// the actual Pi ExtensionContext supplied by the registered tool wrapper.
export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "package_native_read",
    label: "Read package conformance source",
    description: "Read a bounded UTF-8 source file inside the current working directory.",
    parameters: Type.Object({ path: Type.String() }, { additionalProperties: false }),
    async execute(_id, params: { path: string }, _signal, _update, ctx) {
      const cwd = await realpath(ctx.cwd);
      const path = await realpath(resolve(cwd, params.path));
      const rel = relative(cwd, path);
      if (rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) {
        throw new Error("source must be inside the native session working directory");
      }
      if ((await stat(path)).size > 4096) throw new Error("source exceeds conformance bound");
      const text = await readFile(path, "utf8");
      return { content: [{ type: "text", text }], details: { path, bytes: Buffer.byteLength(text) } };
    },
  });
}
