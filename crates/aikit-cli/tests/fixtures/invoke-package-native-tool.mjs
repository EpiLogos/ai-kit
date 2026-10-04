import assert from "node:assert/strict";
import { readFile, realpath, writeFile, symlink } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [packageRoot, workingDir, sdkRoot] = process.argv.slice(2);
assert(packageRoot && workingDir && sdkRoot, "package, working directory and installed Pi SDK root are required");
const sdk = await import(pathToFileURL(resolve(sdkRoot, "dist/index.js")).href);
const sdkPackage = JSON.parse(await readFile(resolve(sdkRoot, "package.json"), "utf8"));
assert.equal(sdkPackage.name, "@earendil-works/pi-coding-agent");
assert.equal(sdkPackage.version, "0.84.4", "conformance requires the evidenced native SDK revision");

const loader = new sdk.DefaultResourceLoader({
  cwd: workingDir,
  agentDir: resolve(workingDir, "agent"),
  additionalExtensionPaths: [packageRoot],
  noExtensions: true,
  noSkills: true,
  noPromptTemplates: true,
  noThemes: true,
  noContextFiles: true,
});
await loader.reload();
assert.deepEqual(loader.getExtensions().errors, []);
const { session } = await sdk.createAgentSession({
  cwd: workingDir,
  agentDir: resolve(workingDir, "agent"),
  resourceLoader: loader,
  sessionManager: sdk.SessionManager.inMemory(workingDir),
  tools: ["package_native_read"],
});
try {
  await session.bindExtensions({ mode: "headless" });
  const registration = session.getAllTools().find(t => t.name === "package_native_read");
  assert(registration, "native session must discover the contributed tool");
  const module = await realpath(resolve(packageRoot, "skills/native-source-reader/native.ts"));
  assert.equal(await realpath(registration.sourceInfo.path), module);
  const tool = session.agent.state.tools.find(t => t.name === "package_native_read");
  assert(tool, "registered tool must be active in the native agent");
  const expected = await readFile(resolve(workingDir, "source.txt"), "utf8");
  const result = await tool.execute("native-package-conformance", { path: "source.txt" }, new AbortController().signal);
  assert.deepEqual(result.content, [{ type: "text", text: expected }]);
  assert.equal(result.details.bytes, Buffer.byteLength(expected));
  await assert.rejects(tool.execute("missing-source", { path: "absent.txt" }, new AbortController().signal), /ENOENT/);
  await writeFile(resolve(workingDir, "../outside.txt"), "outside private source\n");
  await assert.rejects(tool.execute("outside-source", { path: "../outside.txt" }, new AbortController().signal), /inside the native session/);
  await symlink(resolve(workingDir, "../outside.txt"), resolve(workingDir, "outside-link.txt"));
  await assert.rejects(tool.execute("symlink-source", { path: "outside-link.txt" }, new AbortController().signal), /inside the native session/);
  process.stdout.write(JSON.stringify({ schema: "aikit.native-package-tool-conformance/v1", sdk: sdkPackage.name, version: sdkPackage.version, tool: tool.name, module, bytes: result.details.bytes, missing_source: "refused", outside_source: "refused", symlink_source: "refused", model_calls: 0 }) + "\n");
} finally {
  session.dispose();
}
