// Execute the installed Pi parser and resource owner against real native files.
// Controlled source material exercises discovery; this is not provider evidence.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const packageRoot = process.env.PI_AGENT_PACKAGE_ROOT;
assert(packageRoot, "PI_AGENT_PACKAGE_ROOT must select the installed Pi cut");
const { parseArgs } = await import(pathToFileURL(path.join(packageRoot, "dist/cli/args.js")));
const { DefaultResourceLoader } = await import(pathToFileURL(path.join(packageRoot, "dist/core/resource-loader.js")));
const { SettingsManager } = await import(pathToFileURL(path.join(packageRoot, "dist/core/settings-manager.js")));
const directory = await fs.mkdtemp(path.join(os.tmpdir(), "aikit-pi-repertoire-"));
try {
  const cwd = path.join(directory, "project");
  const agentDir = path.join(directory, "agent");
  const roots = {
    implementer: path.join(directory, "contexts/ctx_implementer/generations/gen_implementer/projections/pi/.pi/skills"),
    reviewer: path.join(directory, "contexts/ctx_reviewer/generations/gen_reviewer/projections/pi/.pi/skills"),
    "project-foreign": path.join(cwd, ".pi/skills"),
    "global-foreign": path.join(agentDir, "skills"),
  };
  for (const [name, root] of Object.entries(roots)) {
    const skill = path.join(root, name);
    await fs.mkdir(skill, { recursive: true });
    await fs.writeFile(path.join(skill, "SKILL.md"), `---\nname: ${name}\ndescription: Actual resource-owner ${name} test material\n---\nDistinct ${name} context.\n`);
  }
  const settings = SettingsManager.inMemory();
  settings.setProjectTrusted(true);
  const ambient = new DefaultResourceLoader({ cwd, agentDir, settingsManager: settings,
    noExtensions: true, noPromptTemplates: true, noThemes: true, noContextFiles: true });
  await ambient.reload();
  const ambientNames = ambient.getSkills().skills.map(skill => skill.name);
  assert(ambientNames.includes("global-foreign") && ambientNames.includes("project-foreign"));
  for (const name of ["implementer", "reviewer"]) {
    const args = parseArgs(["--no-skills", "--skill", roots[name]]);
    assert.equal(args.noSkills, true);
    assert.deepEqual(args.skills, [roots[name]]);
    const loader = new DefaultResourceLoader({ cwd, agentDir, settingsManager: settings,
      noSkills: args.noSkills, additionalSkillPaths: args.skills,
      noExtensions: true, noPromptTemplates: true, noThemes: true, noContextFiles: true });
    await loader.reload();
    const reading = loader.getSkills();
    assert.deepEqual(reading.skills.map(skill => skill.name), [name]);
    assert(reading.skills.every(skill => skill.filePath.startsWith(roots[name] + path.sep)));
  }
  console.log(JSON.stringify({ schema: "aikit.pi-repertoire-proof/v1", cases: 2,
    observed_native_resource_owner: true, ambient_skills_before: ambientNames.length, selected_skills_each: 1,
    ambient_project_global_or_sibling_skills: 0, provider_inference_observed: false }));
} finally {
  await fs.rm(directory, { recursive: true, force: true });
}
