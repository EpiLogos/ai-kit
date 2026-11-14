/** Executable installed-Pi regression: a pi session holds its start ground from
 * its start, before any prompt, once. The installed Pi runs in RPC mode with
 * only this carrier (-e; no discovered extension, no session file) from
 * AIKIT_START_GROUND_CWD, a project in a Central world (run it in a scratch
 * home whose Central root holds that project). get_messages must hold the
 * ground the actual dispatcher gives pi at SessionStart
 * (`aikit --json hook dispatch pi SessionStart`, data.injected), and a new
 * session must start holding it once, with nothing written to the RPC client
 * unasked. No model is invoked. AIKIT_BIN is the actual absolute candidate
 * executable; PI_AGENT_PACKAGE_ROOT names the installed Pi package. */
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

const pi=spawn(process.execPath,[join(piRoot,"dist/bundle/cli.js"),"--mode","rpc","--no-extensions","--no-session","-e",carrier],
  {cwd,env:{...process.env,AIKIT_BIN:aikit},stdio:["pipe","pipe","pipe"]});
let buffer="",stderr="",waiting=null;
const unasked=[];
pi.stderr.on("data",chunk=>{stderr+=chunk;});
pi.on("exit",code=>{if(waiting){const w=waiting;waiting=null;clearTimeout(w.timer);w.fail(new Error(`pi exited (${code}) before answering ${w.id}: ${stderr.slice(0,400)}`));}});
pi.stdout.on("data",chunk=>{
  buffer+=chunk; let at;
  while((at=buffer.indexOf("\n"))>=0){
    const line=buffer.slice(0,at); buffer=buffer.slice(at+1);
    let event; try{event=JSON.parse(line);}catch{continue;}
    if(event.type==="response"&&waiting&&event.id===waiting.id){
      const w=waiting; waiting=null; clearTimeout(w.timer);
      if(event.success===false) w.fail(new Error(`pi refused ${w.id}: ${event.error??JSON.stringify(event).slice(0,400)}`));
      else w.done(event.data);
    } else if(event.type!=="response") unasked.push(event.type);
  }
});
const ask=(id,command)=>new Promise((done,fail)=>{
  const timer=setTimeout(()=>{waiting=null;fail(new Error(`pi answered no ${id} within 60 s: ${stderr.slice(0,400)}`));},60_000);
  waiting={id,done,fail,timer};
  pi.stdin.write(JSON.stringify({id,...command})+"\n");
});
const grounds=data=>(data?.messages??[]).filter(m=>m.role==="custom"&&m.customType==="aikit-hook-carrier");
const textOf=m=>typeof m.content==="string"?m.content:m.content.filter(c=>c.type==="text").map(c=>c.text).join("\n");
let checks=0;
try{
  const held=grounds(await ask("messages",{type:"get_messages"}));
  assert.equal(held.length,1,`the session holds ${held.length} start grounds before its first prompt, not one`); checks++;
  assert.equal(held[0].display,false,"the start ground is context, not a shown message"); checks++;
  assert.deepEqual(lines(textOf(held[0])),lines(ground),"the session holds the project and Day the dispatcher gives pi at its start"); checks++;
  // pi rebinds twice for a new session; it starts holding its ground once, and the client hears no message it did not ask for.
  unasked.length=0;
  await ask("new",{type:"new_session"});
  const written=unasked.filter(type=>type==="message_start"||type==="message_end");
  assert.deepEqual(written,[],`pi wrote ${written.join(", ")} to the client unasked while its new session started`); checks++;
  const renewed=grounds(await ask("renewed",{type:"get_messages"}));
  assert.equal(renewed.length,1,`the new session holds ${renewed.length} start grounds, not one`); checks++;
  assert.deepEqual(lines(textOf(renewed[0])),lines(ground),"the new session holds the project and Day the dispatcher gives pi"); checks++;
}finally{
  pi.kill("SIGTERM");
  setTimeout(()=>pi.kill("SIGKILL"),2_000).unref();
}
console.log(JSON.stringify({schema:"aikit.pi-start-ground-verification/v1",checks,passed:checks,pi_package:piRoot,ground:lines(ground),provider_inference_observed:false}));
