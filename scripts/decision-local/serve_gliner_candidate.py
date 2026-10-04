"""Serve a verified derived LoRA checkpoint through the existing decision ABI."""
import argparse
import time
from http.server import ThreadingHTTPServer
from pathlib import Path, PurePosixPath

import gliner_decision as base


def verify_candidate(path, material_manifest):
    document=base.decode(path.read_bytes())
    base.require(isinstance(document,dict) and document.get("schema")=="aikit.decision-model-checkpoint/v1",
                 "derived checkpoint manifest required")
    stock=base.decode(material_manifest.read_bytes())
    base.require(document.get("base_revision")==stock.get("artifact_revision")
                 and document.get("license")==stock.get("license"), "exact stock base/license custody required")
    base.require(base.identifier(document.get("model_id")), "distinct checkpoint model identity required")
    base.require(document["model_id"]!=stock.get("model_id"), "checkpoint cannot impersonate the stock model")
    receipt=Path(document["training_receipt"]).resolve(strict=True)
    base.require(base.sha256(receipt)==document["training_receipt_sha256"], "training receipt changed")
    training=base.decode(receipt.read_bytes())
    base.require(training.get("schema")=="aikit.decision-model-training/v1"
                 and training.get("status")=="trained"
                 and training.get("checkpoint_digest")==document["checkpoint_digest"],
                 "successful native training custody required")
    for key,stock_key in (("base_model","artifact_repo"),("base_revision","artifact_revision"),
                          ("sdk_revision","sdk_revision"),("license","license")):
        base.require(document.get(key)==training.get(key)==stock.get(stock_key),
                     "candidate/training/stock provenance differs: "+key)
    source_digests=training.get("source_digests")
    base.require(isinstance(source_digests,dict)
                 and source_digests.get("material_manifest")==base.sha256(material_manifest),
                 "training did not use this exact stock material manifest")
    for key in ("source_digests","hyperparameters","hardware","calibration","runtime"):
        base.require(document.get(key)==training.get(key),
                     "candidate metadata differs from the successful training receipt: "+key)
    root=Path(document["adapter_root"]).resolve(strict=True)
    base.require(root.is_relative_to(receipt.parent), "checkpoint must belong to its training material")
    prefix=root.relative_to(receipt.parent).as_posix()
    artifacts=document["artifacts"]
    base.require(isinstance(artifacts,dict) and {"adapter_config.json","adapter_model.safetensors"}<=set(artifacts),
                 "native PEFT checkpoint assets required")
    base.require(len(artifacts)<=64, "bounded checkpoint closure required")
    total=0
    for name,basis in artifacts.items():
        relative=PurePosixPath(name)
        base.require(not relative.is_absolute() and ".." not in relative.parts, "checkpoint path escape refused")
        file=(root/relative).resolve(strict=True)
        base.require(training.get("artifacts",{}).get(prefix+"/"+name)==basis,
                     "checkpoint file is absent from the successful training receipt")
        base.require(file.is_relative_to(root) and file.is_file(), "checkpoint source custody required")
        base.require(type(basis["bytes"]) is int and basis["bytes"]>0
                     and file.stat().st_size==basis["bytes"] and base.sha256(file)==basis["sha256"],
                     "checkpoint asset differs from its declared Source")
        total+=basis["bytes"]
    base.require(total<=512*1024*1024, "bounded LoRA material required")
    base.require({file.relative_to(root).as_posix() for file in root.rglob("*") if file.is_file()}==set(artifacts),
                 "unattributed checkpoint material refused")
    return document,root


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest",type=Path,required=True)
    parser.add_argument("--candidate-manifest",type=Path,required=True)
    parser.add_argument("--device",choices=("cpu","mps","cuda"),required=True)
    parser.add_argument("--max-encoder-tokens",type=int,required=True)
    parser.add_argument("--port",type=int,required=True)
    parser.add_argument("--label-rendering",choices=("canonical","criteria"),default="canonical")
    parser.add_argument("--cpu-threads",type=int,default=2)
    args=parser.parse_args()
    base.require(1024<=args.port<=65535,"bounded local endpoint required")
    started=time.monotonic()
    digest=base.sha256(args.candidate_manifest)
    candidate,root=verify_candidate(args.candidate_manifest,args.manifest)
    discriminator=base.Discriminator(args.manifest,args.device,args.max_encoder_tokens,args.label_rendering,args.cpu_threads)
    from peft import PeftModel
    discriminator.model=PeftModel.from_pretrained(discriminator.model,str(root),is_trainable=False).merge_and_unload()
    discriminator.model.eval();discriminator.model.processor.change_mode(is_training=False)
    base.require(base.sha256(args.candidate_manifest)==digest,"checkpoint manifest changed during load")
    discriminator.model_id=candidate["model_id"]
    discriminator.card.update(id=candidate["model_id"],checkpoint_digest=candidate["checkpoint_digest"],
        candidate_manifest_sha256=digest,candidate_adapter_sha256=base.sha256(Path(__file__)),
        load_seconds=time.monotonic()-started,standing="research candidate; no default election")
    with ThreadingHTTPServer(("127.0.0.1",args.port),base.handler_for(discriminator)) as server:
        server.daemon_threads=True;server.serve_forever()


if __name__=="__main__":main()
