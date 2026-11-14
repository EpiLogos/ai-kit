/** Executable installed-Pi regression: a pi session holds its start ground from
 * its start, before any prompt. The installed Pi runs in RPC mode with this
 * carrier (-e) from AIKIT_START_GROUND_CWD, a project in a Central world (run
 * it in a scratch home whose Central root holds that project). get_messages
 * must hold the ground the actual dispatcher gives pi at SessionStart
 * (`aikit --json hook dispatch pi SessionStart`, data.injected). No model is
 * invoked. AIKIT_BIN is the actual absolute candidate executable;
 * PI_AGENT_PACKAGE_ROOT names the installed Pi package. */
import assert from "node:assert/strict";
import {execFileSync,spawn} from "node:child_process";
import {isAbsolute,join,resolve} from "node:path";

const aikit=process.env.AIKIT_BIN, piRoot=process.env.PI_AGENT_PACKAGE_ROOT, cwd=process.env.AIKIT_START_GROUND_CWD;
assert(aikit && isAbsolute(aikit),"AIKIT_BIN must be the actual absolute candidate executable");
assert(piRoot,"PI_AGENT_PACKAGE_ROOT must name the actual installed Pi package");
assert(cwd && isAbsolute(cwd),"AIKIT_START_GROUND_CWD must name a project in a Central world");
const carrier=resolve("registry/capsules/hook/aikit/pi-extension-carrier/payload/pi-extension-carrier.ts");
// What the actual dispatcher gives pi at its start, asked as the carrier asks.
// A world that grounds nothing would prove nothing, so it is refused.
const asked=JSON.parse(execFileSync(aikit,["--json","hook","dispatch","pi","SessionStart"],
  {cwd,input:JSON.stringify({type:"session_start",reason:"startup",cwd}),encoding:"utf8"}));
const ground=asked?.data?.injected??"";
const lines=text=>text.split("\n").filter(line=>/^(project|Day):/.test(line));
assert.equal(lines(ground).length,2,`the dispatcher gives pi no project and Day from ${cwd}: ${JSON.stringify(asked?.data??asked).slice(0,400)}`);
const messages=await new Promise((done,fail)=>{
  const pi=spawn(process.execPath,[join(piRoot,"dist/bundle/cli.js"),"--mode","rpc","-e",carrier],
    {cwd,env:{...process.env,AIKIT_BIN:aikit},stdio:["pipe","pipe","pipe"]});
  let buffer="",stderr="";
  const exited=code=>fail(new Error(`pi exited (${code}) before answering: ${stderr.slice(0,400)}`));
  const timer=setTimeout(()=>{pi.off("exit",exited);pi.kill("SIGTERM");fail(new Error(`pi answered no get_messages within 60 s: ${stderr.slice(0,400)}`));},60_000);
  pi.stderr.on("data",chunk=>{stderr+=chunk;});
  pi.on("exit",exited);
  pi.stdout.on("data",chunk=>{
    buffer+=chunk; let at;
    while((at=buffer.indexOf("\n"))>=0){
      const line=buffer.slice(0,at); buffer=buffer.slice(at+1);
      let event; try{event=JSON.parse(line);}catch{continue;}
      if(event.type==="response"&&event.id==="messages"){clearTimeout(timer);pi.off("exit",exited);pi.kill("SIGTERM");done(event.data?.messages??[]);}
    }
  });
  pi.stdin.write(JSON.stringify({id:"messages",type:"get_messages"})+"\n");
});
let checks=0;
const held=messages.filter(m=>m.role==="custom"&&m.customType==="aikit-hook-carrier");
assert.equal(held.length,1,`the session holds no start ground before its first prompt: ${JSON.stringify(messages).slice(0,400)}`); checks++;
assert.equal(held[0].display,false,"the start ground is context, not a shown message"); checks++;
const text=typeof held[0].content==="string"?held[0].content:held[0].content.filter(c=>c.type==="text").map(c=>c.text).join("\n");
assert.deepEqual(lines(text),lines(ground),"the session holds the project and Day the dispatcher gives pi at its start"); checks++;
console.log(JSON.stringify({schema:"aikit.pi-start-ground-verification/v1",checks,passed:checks,pi_package:piRoot,ground:lines(ground),provider_inference_observed:false}));
