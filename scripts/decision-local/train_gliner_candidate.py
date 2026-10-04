"""Train a bounded local GLiNER LoRA candidate from declared owner data.

This explicit research command uses the verified stock material and pinned SDK.
It elects no provider and starts no body hooks. Checkpoints are derived material;
training loss does not establish held-out task performance or default standing.
"""
import argparse
import dataclasses
import importlib.metadata
import json
import os
import platform
import resource
import signal
import time
from pathlib import Path

import gliner_decision as material


def run(args):
    started=time.monotonic()
    manifest,snapshot,sdk=material.verify_material(args.material_manifest)
    basis={"training_code":material.sha256(Path(__file__)),"material_manifest":material.sha256(args.material_manifest),
           "train":material.sha256(args.train),"validation":material.sha256(args.validation),
           "dataset_manifest":material.sha256(args.dataset_manifest)}
    data_manifest=json.loads(args.dataset_manifest.read_text())
    material.require(data_manifest.get("held_out_test_included") is False, "explicit held-out exclusion required")
    for split,path in (("train",args.train),("validation",args.validation)):
        material.require(data_manifest["files"][split]["sha256"]=="sha256:"+material.sha256(path),
                         "training split digest differs from its owner's pack")
    args.output.mkdir(parents=True,exist_ok=False)
    os.environ["HF_HUB_OFFLINE"]="1";os.environ["TRANSFORMERS_OFFLINE"]="1"
    import torch
    import gliner2
    material.require(not torch.cuda.is_available(),
                     "this bounded CPU trainer does not admit automatic CUDA placement")
    material.require(Path(gliner2.__file__).resolve().is_relative_to(sdk/"gliner2"), "pinned SDK import required")
    from gliner2 import GLiNER2
    from gliner2.training import TrainingConfig,GLiNER2Trainer,TrainingDataset
    torch.set_num_threads(2)
    config=TrainingConfig(output_dir=str(args.output/"checkpoints"),experiment_name="owner-semantic-candidate",
        max_steps=args.steps,num_epochs=args.epochs,batch_size=1,eval_batch_size=1,num_workers=0,pin_memory=False,
        task_lr=args.learning_rate,fp16=False,bf16=False,use_lora=True,lora_r=args.rank,lora_alpha=2*args.rank,
        lora_dropout=0.,lora_target_modules=args.targets,save_adapter_only=True,save_total_limit=2,
        encoder_lr=args.learning_rate,
        gradient_checkpointing=args.gradient_checkpointing,
        eval_strategy="steps",eval_steps=args.steps,logging_steps=1,report_to_wandb=False,
        strict_training=True,skip_step_errors=False,ignore_nonfinite_losses=False,
        allow_invalid_samples=False,on_capacity_exceeded="raise",max_len=None,
        fused_optimizer=False,seed=42,deterministic=True)
    retained={"schema":"aikit.decision-model-training/v1","status":"running","base_model":manifest["artifact_repo"],
        "base_revision":manifest["artifact_revision"],"sdk_revision":manifest["sdk_revision"],"license":manifest["license"],
        "source_digests":basis,"hyperparameters":dataclasses.asdict(config),
        "calibration":"none; existing fixed research threshold remains uncalibrated",
        "runtime":{"python":platform.python_version(),
                   "packages":{name:importlib.metadata.version(name)
                               for name in ("torch","transformers","gliner2","peft")}},
        "hardware":{"machine":platform.machine(),"platform":platform.platform(),"device":"cpu","threads":2},
        "scope":"bounded native LoRA candidate; no default election"}
    receipt=args.output/"training.json"
    receipt.write_bytes(material.canonical(retained)+b"\n")
    try:
        train=TrainingDataset.load(args.train);validation=TrainingDataset.load(args.validation)
        train.validate();validation.validate()
        model=GLiNER2.from_pretrained(str(snapshot),local_files_only=True)
        trainer=GLiNER2Trainer(model=model,config=config)
        parameter=next(model.parameters())
        material.require(str(trainer.device)=="cpu" and parameter.device.type=="cpu",
                         "actual native trainer must honor the declared CPU boundary")
        retained["hardware"].update(device=str(parameter.device),dtype=str(parameter.dtype))
        if args.gradient_checkpointing:
            # Native HF checkpointing must retain gradients through a frozen
            # embedding. This enables input gradients without unfreezing base
            # weights; PEFT still owns the actual trainable parameter field.
            encoder=getattr(trainer.model,"encoder",None)
            enable=getattr(encoder,"enable_input_require_grads",None)
            material.require(callable(enable),"native checkpointed encoder input gradients required")
            enable()
        retained["model_load_seconds"]=time.monotonic()-started
        retained["trainable_parameters"]=sum(p.numel() for p in model.parameters() if p.requires_grad)
        material.require(0 < retained["trainable_parameters"] <= 10000000, "bounded native LoRA parameters required")
        result=trainer.train(train_data=train,eval_data=validation)
        if any(target.startswith("encoder.") for target in args.targets):
            from safetensors.torch import load_file
            weights=load_file(str(args.output/"checkpoints/best/adapter_model.safetensors"))
            updates=[tensor for name,tensor in weights.items() if "encoder" in name and "lora_B" in name]
            material.require(bool(updates) and any(bool(tensor.ne(0).any()) for tensor in updates),
                             "successful encoder candidate must contain actual native LoRA updates")
        artifacts={str(p.relative_to(args.output)):{"bytes":p.stat().st_size,"sha256":material.sha256(p)}
                   for p in sorted((args.output/"checkpoints").rglob("*")) if p.is_file()}
        material.require(any(name.endswith((".safetensors",".bin")) for name in artifacts), "native checkpoint weights required")
        retained.update(status="trained",training_summary=result,artifacts=artifacts,
                        checkpoint_digest="sha256:"+__import__("hashlib").sha256(material.canonical(artifacts)).hexdigest())
    except BaseException as error:
        retained.update(status="failed",failure=type(error).__name__+": "+str(error))
        raise
    finally:
        retained["elapsed_seconds"]=time.monotonic()-started
        retained["peak_process_rss_bytes"]=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if platform.system()!="Darwin":retained["peak_process_rss_bytes"]*=1024
        receipt.write_bytes(material.canonical(retained)+b"\n")
    print(json.dumps({key:retained[key] for key in ("status","elapsed_seconds","checkpoint_digest","trainable_parameters")}))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--material-manifest",type=Path,required=True)
    parser.add_argument("--train",type=Path,required=True)
    parser.add_argument("--validation",type=Path,required=True)
    parser.add_argument("--dataset-manifest",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--steps",type=int,default=16)
    parser.add_argument("--rank",type=int,default=4)
    parser.add_argument("--epochs",type=int,default=1)
    parser.add_argument("--targets",nargs="+",choices=("classifier","encoder.query","encoder.key","encoder.value","encoder.dense"),
                        default=["classifier"])
    parser.add_argument("--gradient-checkpointing",action="store_true")
    parser.add_argument("--learning-rate",type=float,default=0.0002)
    args=parser.parse_args()
    material.require(1 <= args.steps <= 256 and 1 <= args.rank <= 32 and 1 <= args.epochs <= 8
                     and 0 < args.learning_rate <= .01, "bounded explicit training parameters required")
    material.require(len(args.targets)==len(set(args.targets)), "duplicate native LoRA targets refused")
    def interrupted(signum, _frame):
        raise SystemExit(128+signum)
    signal.signal(signal.SIGTERM,interrupted)
    signal.signal(signal.SIGINT,interrupted)
    run(args)


if __name__ == "__main__":main()
