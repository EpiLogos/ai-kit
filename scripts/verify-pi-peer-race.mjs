/** Exact native Gateway + installed Pi SessionManager race regression.
 * Requires the current actual Position/generation and an explicitly selected
 * pending batch belonging to this commission. No provider inference is claimed.
 * The wrapper delays only transport output; the real native command reads and
 * acknowledges its existing durable records under the actual owner basis. */
import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import {mkdtempSync,readFileSync,writeFileSync,existsSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {join,resolve,isAbsolute} from "node:path";
import {pathToFileURL} from "node:url";
import {setTimeout as pause} from "node:timers/promises";

const realBinary=process.env.AIKIT_BIN;
const piRoot=process.env.PI_AGENT_PACKAGE_ROOT;
const approved=new Set((process.env.AIKIT_RACE_COMMUNIQUE_REFS || "").split(",").filter(Boolean));
assert(realBinary && isAbsolute(realBinary),"AIKIT_BIN must be the actual absolute candidate executable");
assert(piRoot,"PI_AGENT_PACKAGE_ROOT must name the actual installed Pi package");
assert(process.env.OI_POSITION_REF && process.env.OI_OCCUPANT_GENERATION,"Use the actual current occupying parent");
assert(approved.size>0,"Select exact commission-owned pending message refs; foreign inbox material must remain untouched");
const directory=mkdtempSync(join(tmpdir(),"aikit-pi-native-race-"));
const gate=join(directory,"offered.json"),release=join(directory,"release"),wrapper=join(directory,"owner-wrapper.mjs");
let consumer;
try {
  writeFileSync(wrapper,`#!${process.execPath}\nimport {execFile} from 'node:child_process';\nimport {writeFileSync,existsSync} from 'node:fs';\nconst args=process.argv.slice(2);\nexecFile(${JSON.stringify(realBinary)},args,{encoding:'utf8',timeout:10000,maxBuffer:1024*1024},async(error,stdout,stderr)=>{\n if(error){process.stderr.write(stderr||String(error));process.exitCode=1;return;}\n if(!args.includes('--commit')&&!existsSync(${JSON.stringify(gate)})){\n   writeFileSync(${JSON.stringify(gate)},stdout,{mode:0o600});\n   const limit=Date.now()+3000;\n   while(!existsSync(${JSON.stringify(release)})){if(Date.now()>limit){process.exitCode=1;return;}await new Promise(r=>setTimeout(r,5));}\n }\n process.stdout.write(stdout);\n});\n`,{mode:0o700});
  process.env.AIKIT_BIN=wrapper;
  const {createJiti}=await import(pathToFileURL(join(piRoot,"node_modules/jiti/lib/jiti.mjs")));
  const {SessionManager}=await import(pathToFileURL(join(piRoot,"dist/core/session-manager.js")));
  const {NativePeerDelivery}=await createJiti(import.meta.url,{moduleCache:false}).import(
    resolve("registry/capsules/hook/aikit/pi-extension-carrier/payload/pi-extension-carrier.ts"));
  const manager=SessionManager.create(directory,join(directory,"sessions"));
  let duplicateTriggers=0;
  consumer=new NativePeerDelivery({sendMessage(){duplicateTriggers++;}});
  consumer.start({cwd:process.cwd(),sessionManager:manager,isIdle:()=>true,hasPendingMessages:()=>false});
  const deadline=Date.now()+10000;
  while(!existsSync(gate)){assert(Date.now()<deadline,"actual native owner did not return a bounded offer");await pause(5);}
  const response=JSON.parse(readFileSync(gate,"utf8"));
  assert.equal(response.ok,true,"native owner refused the actual parent basis");
  const delivery=response.data?.delivery;
  assert(delivery?.communique_refs?.length>0,"prepare the exact owned pending handoff before this regression");
  assert(delivery.communique_refs.every(ref=>approved.has(ref)),"refuse to acknowledge foreign or unselected inbox material");
  // The actual retained peer operation appears while the owner process is
  // paused after reading and before returning its offer to the extension.
  manager.appendCustomMessageEntry("aikit-native-peer-handoff",delivery.text,true,
    {schema:"aikit.pi-peer-handoff/v1",recipient_session_id:manager.getSessionId(),delivery});
  manager.appendMessage({role:"assistant",content:[{type:"text",text:"controlled persistence boundary"}],api:"test",provider:"controlled",model:"persistence-only",usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}},stopReason:"stop",timestamp:Date.now()});
  assert(existsSync(manager.getSessionFile()),"the actual native operation must be retained");
  writeFileSync(release,"resume",{mode:0o600});
  while(consumer.inFlight){assert(Date.now()<deadline,"native reconciliation exceeded its finite transport budget");await pause(5);}
  assert.equal(duplicateTriggers,0,"fresh retained native branch must reconcile before another peer turn");
  const readback=JSON.parse(execFileSync(realBinary,["--json","gateway","handoff"],{encoding:"utf8",timeout:10000,maxBuffer:1024*1024}));
  assert.equal(readback.ok,true);
  assert(!(readback.data?.delivery?.communique_refs || []).some(ref=>delivery.communique_refs.includes(ref)),
    "the actual owner must observe acknowledgement of the selected retained carrying");
  console.log(JSON.stringify({schema:"aikit.pi-peer-race-verification/v1",carried_records:delivery.communique_refs.length,
    duplicate_peer_turns:duplicateTriggers,native_gateway_readback:true,native_pi_retained_operation:true,provider_inference_observed:false}));
} finally {
  consumer?.stop();
  process.env.AIKIT_BIN=realBinary;
  // Release a paused read transport even when preconditions fail, so it exits
  // within its bound before its private scratch is removed.
  if(existsSync(gate)&&!existsSync(release)){writeFileSync(release,"stop",{mode:0o600});await pause(50);}
  rmSync(directory,{recursive:true,force:true});
}
