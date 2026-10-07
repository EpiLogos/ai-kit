// Tenure-scoped Central team consumer. Native AIKit/Actuation/Workcell owners
// prepare bodies, admit work, retain deliveries, cancel and release. This
// extension never spawns a harness, creates a mailbox or edits a human draft.
import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { Type } from "typebox";

export default function (pi: any) {
  const team = JSON.parse(readFileSync(new URL("./team.json", import.meta.url), "utf8"));
  const bin = process.env.AIKIT_BIN || "aikit";
  const name = `aikit_team_${team.agent_set_ref.replace(/[^a-zA-Z0-9_]/g, "_")}`;
  pi.registerTool({
    name, label: "Central team",
    description: `Delegate bounded native work through Central team ${team.agent_set_ref}. Members: ${team.members.map((member: any) => `${member.agent_ref}: ${member.description}`).join("; ")}. Use action prepare to resolve an admitted member into an existing SessionSpace/task/Workcell basis; delegate sends to its exact prepared task, read collects its attributed Return, cancel interrupts it, release retires its exact idle native generation. Native owner refusals retain recovery evidence.`,
    parameters: Type.Object({ request: Type.String({ description: "JSON of the existing gateway team operation (prepare, delegate, read, cancel, release), retaining exact member/session/task/source/authority refs. Use list for the authored membership." }) }),
    async execute(_id: string, params: { request: string }, signal: AbortSignal, _update: any, ctx: any) {
      const request = JSON.parse(params.request);
      if (request.action === "list") return {content:[{type:"text",text:JSON.stringify(team)}],details:team};
      if (!team.members.some((member: any) => member.agent_ref === request.member_ref)) throw new Error("Requested Agent is outside this authored team");
      return await new Promise((resolve, reject) => {
        const child = execFile(bin, ["--json", "gateway", "team", "--request-json", JSON.stringify(request)], {
          cwd: ctx.cwd, timeout: 120_000, maxBuffer: 1024 * 1024, encoding: "utf8",
        }, (error, stdout, stderr) => {
          signal?.removeEventListener("abort", abort);
          let result: any;
          try { result = JSON.parse(stdout); } catch { return reject(new Error(`Native team operation unreadable: ${error?.message || stderr.slice(-2048)}`)); }
          if (result.ok !== true) return reject(new Error(JSON.stringify(result.error || result)));
          resolve({content:[{type:"text",text:JSON.stringify(result.data)}],details:result.data});
        });
        const abort = () => {
          // Stop this consumer and request cancellation through the actual
          // resident owner. Its journal retains any uncertain original effect.
          child.kill("SIGTERM");
          const expectedTask=request.expected_task || request.turn?.expected_task;
          if (request.agent_session && expectedTask) {
            execFile(bin,["--json","gateway","team","--request-json",JSON.stringify({action:"cancel",member_ref:request.member_ref,agent_session:request.agent_session,expected_task:expectedTask,reason:"Parent Pi tool cancellation"})],{cwd:ctx.cwd,timeout:15_000,maxBuffer:64*1024},()=>{});
          }
        };
        signal?.addEventListener("abort",abort,{once:true});
        if (signal?.aborted) abort();
        child.stdin?.end();
      });
    },
  });
}
