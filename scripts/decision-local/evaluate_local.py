#!/usr/bin/env python3
"""Bounded local-decision evaluation on held-out, programmatically-labelled tasks.

This is local sanity evidence for one elected endpoint, not a benchmark claim:
public numbers describe other machines, other sources and other calibrations.
Every label here is computed from the task's own construction (the same
discipline upstream's programmatically-labelled suites use), so nothing is
judged by an LLM and no answer key left the machine.

Families (8 tasks each, 40 in all; every family fixed across runs so repeats
are comparable, option-order variations are seeded):
  negation          — statements whose true/false flips with a negation word
  exceptions        — policy rules with stated exceptions and sublimits
  missing_evidence  — questions a given state cannot answer; the honest Noul
                      mass sits near the middle instead of a confident extreme
  multi_contributor — several listed contributors each change the answer
  option_order      — one Choice question re-served under seeded option orders;
                      reports argmax stability and per-option spread

Usage:
  python3 evaluate_local.py --endpoint 127.0.0.1:8019 --model kev-latest \
      [--output result.json] [--timeout 60]
"""
import argparse, itertools, json, random, sys, time, urllib.request

FAMILIES = {}

def family(name):
    def wrap(fn):
        FAMILIES[name] = fn
        return fn
    return wrap

@family("negation")
def negation():
    tasks = []
    pairs = [
        ("The backup job has finished.", "The backup job has not finished."),
        ("The deploy is reversible.", "The deploy is not reversible."),
        ("Every reviewer approved the change.", "Not every reviewer approved the change."),
        ("The port is free.", "The port is not free."),
        ("The credential was rotated.", "The credential was not rotated."),
        ("The migration is idempotent.", "The migration is not idempotent."),
        ("The licence covers commercial use.", "The licence does not cover commercial use."),
        ("The retry loop eventually terminates.", "The retry loop never terminates."),
    ]
    for i, (pos, neg) in enumerate(pairs):
        tasks.append({
            "state": {"observation": pos},
            "question": f"observation_{i}",
            "label": 1.0,
            "instructions": "Is the observation's claim TRUE?",
        })
        tasks.append({
            "state": {"observation": neg},
            "question": f"observation_{i}",
            "label": 0.0,
            "instructions": "Is the observation's claim TRUE?",
        })
    return tasks

@family("exceptions")
def exceptions():
    rules = [
        ("Deploys freeze at 17:00 on Fridays.", "It is Friday 16:59 and the change is routine.", False),
        ("Deploys freeze at 17:00 on Fridays, except hotfixes.", "It is Friday 16:59 and this is a security hotfix.", True),
        ("Refunds require a receipt.", "The customer wants a refund and has a receipt.", True),
        ("Refunds require a receipt, except under recall.", "The item is under an official recall; no receipt.", True),
        ("Only admins may rotate keys.", "An admin is rotating a key.", True),
        ("Only admins may rotate keys.", "A developer account is rotating a key.", False),
        ("Cache flushes require approval during peak hours.", "It is 03:00 (off-peak) and the flush is requested.", True),
        ("PRs need two approvals, but docs-only PRs need one.", "A docs-only PR has one approval.", True),
    ]
    return [{
        "state": {"policy": rule, "situation": situation},
        "question": "permitted",
        "label": label,
        "instructions": "Under the stated policy, is the situation PERMITTED?",
    } for rule, situation, label in rules]

@family("missing_evidence")
def missing_evidence():
    # The state deliberately lacks the fact each question turns on. A confident
    # 0/1 is a hallucination; near-0.5 (or an explicit abstain reading) is the
    # honest answer. Scored as distance from the extremes, never as accuracy.
    unknowable = [
        {"state": {"log": "service restarted at 03:14"}, "question": "Did the restart fix the memory leak?"},
        {"state": {"diff": "+ assert(x > 0)"}, "question": "Will the suite pass in CI?"},
        {"state": {"ticket": "user reports slowness"}, "question": "Is the user's disk full?"},
        {"state": {"metric": "p99 rose 4% after the deploy"}, "question": "Did the deploy cause the rise?"},
        {"state": {"email": "see attached"}, "question": "Was the attachment a contract?"},
        {"state": {"commit": "refactor auth middleware"}, "question": "Does the refactor change behaviour?"},
        {"state": {"alarm": "CPU high on host-7"}, "question": "Is host-7 out of disk?"},
        {"state": {"note": "partner API sometimes 500s"}, "question": "Will the partner API fail today?"},
    ]
    return [{
        "state": task["state"],
        "question": "knowable",
        "label": None,   # abstention-lean: scored by distance from extremes
        "instructions": task["question"] + " Answer yes only if the state establishes it.",
    } for task in unknowable]

@family("multi_contributor")
def multi_contributor():
    tasks = []
    contributors = [
        ("alice", "approved the schema change"),
        ("bob", "flagged the migration as blocking"),
        ("carol", "owns the release checklist"),
        ("dara", "requested a docs update"),
        ("eli", "reverted the caching layer"),
        ("faye", "opened the follow-up ticket"),
        ("gus", "signed off the security review"),
        ("hana", "is on holiday until Monday"),
    ]
    for i, (who, act) in enumerate(contributors):
        blocking = "blocking" in act or "reverted" in act
        tasks.append({
            "state": {"contributors": [{"name": w, "action": a} for w, a in contributors[: i + 1]]},
            "question": "release_blocked",
            "label": 1.0 if any("blocking" in a or "reverted" in a for _, a in contributors[: i + 1]) else 0.0,
            "instructions": "Does any listed contributor's action block the release?",
        })
    return tasks

@family("option_order")
def option_order():
    tasks = []
    cases = [
        ({"ticket": "Two charges appear for one order; the customer wants one refunded."},
         {"billing": "Charges, invoices, payment problems",
          "returns": "Exchanges, refunds, wrong items",
          "shipping": "Delivery status, delays"}, "billing"),
        ({"ticket": "Parcel marked delivered three weeks ago, never arrived."},
         {"billing": "Charges and invoices",
          "returns": "Exchanges and refunds",
          "shipping": "Delivery status, delays, lost packages"}, "shipping"),
        ({"ticket": "The wrong size arrived; the customer wants the correct size."},
         {"shipping": "Delivery status",
          "billing": "Charges, invoices",
          "returns": "Exchanges, refunds, wrong items"}, "returns"),
        ({"alert": "Disk at 97% on the database host."},
         {"capacity": "Disk, memory, CPU exhaustion",
          "network": "Packet loss, latency",
          "security": "Auth anomalies"}, "capacity"),
    ]
    for i, (state, options, expected) in enumerate(cases):
        tasks.append({
            "state": state, "question": f"route_{i}", "label": expected,
            "options": options, "orders": 3,
            "instructions": "Which team should handle this?",
        })
    return tasks

def ask(endpoint, model, state, questions, timeout):
    body = json.dumps({"model": model, "state": state, "questions": questions}).encode()
    request = urllib.request.Request(
        f"http://{endpoint}/v1/systemone", data=body,
        headers={"Content-Type": "application/json"}, method="POST")
    started = time.perf_counter()
    with urllib.request.urlopen(request, timeout=timeout) as response:
        payload = json.load(response)
    return payload, (time.perf_counter() - started) * 1000.0

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--endpoint", required=True)
    ap.add_argument("--model", required=True)
    ap.add_argument("--output")
    ap.add_argument("--timeout", type=int, default=60)
    args = ap.parse_args()

    results, latencies = {}, []
    for name, builder in FAMILIES.items():
        tasks, entries = builder(), []
        for index, task in enumerate(tasks):
            if name == "option_order":
                options = task["options"]
                orders, spread = [], {k: [] for k in options}
                rng = random.Random(20260927 + index)
                for order_index in range(task["orders"]):
                    keys = list(options)
                    if order_index:
                        rng.shuffle(keys)
                    ordered = {k: options[k] for k in keys}
                    payload, ms = ask(args.endpoint, args.model, task["state"], {
                        task["question"]: {"type": "choice", "instructions": task["instructions"],
                                           "criteria": ordered}}, args.timeout)
                    latencies.append(ms)
                    a = payload["answers"][task["question"]]
                    orders.append({"order": keys, "choice": a["choice"],
                                   "probabilities": a["probabilities"]})
                    for k in keys:
                        spread[k].append(a["probabilities"][k])
                argmax_stable = len({o["choice"] for o in orders}) == 1
                correct_any = any(o["choice"] == task["label"] for o in orders)
                entries.append({"id": index, "label": task["label"], "orders": orders,
                                "argmax_stable": argmax_stable,
                                "correct_when_stable": argmax_stable
                                and orders[0]["choice"] == task["label"],
                                "ever_correct": correct_any,
                                "spread": {k: max(v) - min(v) for k, v in spread.items()}})
            else:
                payload, ms = ask(args.endpoint, args.model, task["state"], {
                    task["question"]: {"type": "noul", "instructions": task["instructions"]}},
                    args.timeout)
                latencies.append(ms)
                noul = payload["answers"][task["question"]]["noul"]
                entry = {"id": index, "noul": noul}
                if task["label"] is None:
                    # missing evidence: reward honest middle mass, punish extremes
                    entry["abstention_lean"] = 1.0 - abs(noul - 0.5) * 2
                else:
                    entry["label"] = task["label"]
                    entry["correct"] = (noul >= 0.5) == (task["label"] == 1.0)
                entries.append(entry)
        results[name] = entries

    summary = {}
    for name, entries in results.items():
        if name == "missing_evidence":
            summary[name] = {"mean_abstention_lean":
                             sum(e["abstention_lean"] for e in entries) / len(entries)}
        elif name == "option_order":
            summary[name] = {
                "argmax_stable": sum(e["argmax_stable"] for e in entries),
                "correct_when_stable": sum(e["correct_when_stable"] for e in entries),
                "ever_correct": sum(e["ever_correct"] for e in entries),
                "total": len(entries),
                "max_spread": max(max(e["spread"].values()) for e in entries),
            }
        else:
            summary[name] = {
                "accuracy": sum(1 for e in entries if e.get("correct")) / len(entries),
                "total": len(entries),
            }
    summary["_latency_ms"] = {
        "mean": sum(latencies) / len(latencies), "max": max(latencies),
        "n": len(latencies)}
    summary["_basis"] = {
        "endpoint": args.endpoint, "model_selector": args.model,
        "labels": "computed from each task's construction; no LLM judge, no external benchmark",
        "note": "local sanity evidence only — not comparable to public benchmark numbers",
    }
    out = {"families": results, "summary": summary}
    if args.output:
        with open(args.output, "w") as f:
            json.dump(out, f, indent=2)
    print(json.dumps(summary, indent=2))

if __name__ == "__main__":
    main()
