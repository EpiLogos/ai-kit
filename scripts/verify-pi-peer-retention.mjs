/** Executable installed-Pi persistence regression. This verifies actual native
 * SessionManager files; it does not claim provider inference or a live Return.
 * PI_AGENT_PACKAGE_ROOT points at the Pi installation being exercised. */
import assert from "node:assert/strict";
import {mkdtempSync,readFileSync,writeFileSync,renameSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {pathToFileURL} from "node:url";
const root=process.env.PI_AGENT_PACKAGE_ROOT;
if(!root) throw new Error("Set PI_AGENT_PACKAGE_ROOT to the actual installed @earendil-works/pi-coding-agent package");
const {createJiti}=await import(pathToFileURL(join(root,"node_modules/jiti/lib/jiti.mjs")));
const {SessionManager}=await import(pathToFileURL(join(root,"dist/core/session-manager.js")));
const jiti=createJiti(import.meta.url,{moduleCache:false});
const {NativePeerDelivery}=await jiti.import(resolve("registry/capsules/hook/aikit/pi-extension-carrier/payload/pi-extension-carrier.ts"));
const directory=mkdtempSync(join(tmpdir(),"aikit-pi-retention-"));
let checks=0;
try {
  let manager=SessionManager.create(directory,join(directory,"sessions"));
  const context={sessionManager:manager,cwd:directory};
  const consumer=new NativePeerDelivery({}); consumer.update(context);
  manager.appendCustomMessageEntry("aikit-native-peer-handoff","quoted attributed peer material",true,{recipient_session_id:manager.getSessionId()});
  const initial=manager.getBranch().at(-1);
  assert.equal(initial.type,"custom_message"); checks++;
  assert.equal(consumer.retainedOnDisk(initial),false,"native memory before first assistant is not durable carrying"); checks++;
  // A controlled native message exercises Pi's actual flush mechanism. It is
  // not provider evidence; the test never invokes a model or claims a Return.
  manager.appendMessage({role:"assistant",content:[{type:"text",text:"persistence boundary"}],api:"test",provider:"controlled",model:"persistence-only",usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}},stopReason:"stop",timestamp:Date.now()});
  assert.equal(consumer.retainedOnDisk(initial),true); checks++;
  const material="😀".repeat(30_000);
  manager.appendCustomMessageEntry("aikit-native-peer-handoff",material,true,{recipient_session_id:manager.getSessionId()});
  const unicode=manager.getBranch().at(-1);
  assert.equal(consumer.retainedOnDisk(unicode),true,"native UTF-8 records survive read chunk boundaries"); checks++;
  assert.equal(consumer.retainedOnDisk({...unicode,content:material+"changed"}),false,"same entry identity cannot attest changed carrying"); checks++;
  const file=manager.getSessionFile();
  manager=SessionManager.open(file);context.sessionManager=manager;consumer.update(context);
  assert.equal(consumer.retainedOnDisk(manager.getBranch().at(-1)),true,"native restart retains the exact operation"); checks++;
  const bytes=readFileSync(file);
  writeFileSync(file,bytes.subarray(0,bytes.length-5));
  assert.equal(consumer.retainedOnDisk(unicode),false,"interrupted native record is not confirmed"); checks++;
  writeFileSync(file,bytes);
  renameSync(file,file+".paused");
  assert.equal(consumer.retainedOnDisk(unicode),false,"unreadable native history stays pending"); checks++;
  renameSync(file+".paused",file);
  const successor=SessionManager.create(directory,join(directory,"successor"));context.sessionManager=successor;consumer.update(context);
  assert.equal(consumer.retainedOnDisk(unicode),false,"another native session cannot claim retained carrying"); checks++;
  consumer.stop();
  console.log(JSON.stringify({schema:"aikit.pi-retention-verification/v1",checks,passed:checks,pi_package:root,provider_inference_observed:false}));
} finally {rmSync(directory,{recursive:true,force:true});}
