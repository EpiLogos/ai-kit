"""Real codec/packing/refusal checks; these do not simulate model inference."""
import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import gliner_decision as g
import prepare_gliner_material as material


MODEL = "gliner2.5-decide@controlled-codec-basis"


def request():
    return {"model": MODEL, "state": {"event": "retain the source verbatim: L2′ — concern"},
            "questions": {"a [DESCRIPTION] b": {"type": "noul", "instructions": {
                "native_question": "Which candidate is supported?"}, "criteria": {
                    "true": {"description": "exact owner-supplied candidate"},
                    "false": ["insufficient evidence", "unsupported"]}}}}


class NativeProtocolChecks(unittest.TestCase):
    def test_native_state_instructions_and_structured_criteria_survive_packing(self):
        document = request()
        text, tasks, bindings = g.plan_request(document, MODEL)
        self.assertEqual(json.loads(text), document["state"])
        self.assertEqual(bindings, {"decision_0": "a [DESCRIPTION] b"})
        task = tasks["decision_0"]
        self.assertEqual(task["class_act"], "softmax")
        self.assertEqual(task["cls_threshold"], 0)
        self.assertTrue(task["multi_label"])
        for label in ("true", "false"):
            self.assertEqual(json.loads(task["labels"][label]), {
                "question": document["questions"][bindings["decision_0"]]["instructions"],
                "outcome": label,
                "criteria": document["questions"][bindings["decision_0"]]["criteria"][label]})

    def test_schema_safe_task_ids_preserve_all_actual_question_identities(self):
        document = request()
        document["questions"]["a"] = {"type": "noul"}
        _, tasks, bindings = g.plan_request(document, MODEL)
        self.assertEqual(set(tasks), {"decision_0", "decision_1"})
        self.assertEqual(set(bindings.values()), set(document["questions"]))

    def test_criteria_rendering_retains_exact_instructions_once_and_owner_criteria(self):
        document = request()
        document["questions"]["faculty"] = {"type":"choice", "instructions":"Choose a supported owner label",
            "criteria":{"#0":"Owner zero description", "#1":"Owner one description"}}
        text,tasks,bindings = g.plan_request(document,MODEL,"criteria")
        self.assertEqual(json.loads(text),{"state":document["state"],"questions":[
            {"id":key,"instructions":question["instructions"]} for key,question in document["questions"].items()]})
        for task_id,key in bindings.items():
            for label,criterion in document["questions"][key]["criteria"].items():
                expected=criterion if isinstance(criterion,str) else g.canonical(criterion).decode("utf-8")
                self.assertEqual(tasks[task_id]["labels"][label],expected)
        with self.assertRaises(g.Refused):g.plan_request(document,MODEL,"invented")

    def test_foreign_model_or_provider_fields_are_refused(self):
        for change in ({"model": "different-artifact"}, {"gold": ["verifier-answer"]}):
            document = request()
            document.update(change)
            with self.assertRaises(g.Refused):
                g.plan_request(document, MODEL)

    def test_unsupported_heads_and_unknown_question_fields_are_refused(self):
        for question in ({"type": "choice", "criteria": {}},
                         {"type": "score", "criteria": ["low", "high"]},
                         {"type": "noul", "extra": True},
                         {"type": "noul", "instructions": 1},
                         {"type": "noul", "criteria": {"maybe": "x"}},
                         {"type": "noul", "criteria": {"true": 1}}):
            document = request()
            document["questions"] = {"id": question}
            with self.assertRaises(g.Refused):
                g.plan_request(document, MODEL)

    def test_no_question_is_silently_dropped_to_fit_native_bounds(self):
        document = request()
        for count in (0, 257):
            document["questions"] = {str(i): {"type": "noul"} for i in range(count)}
            with self.assertRaises(g.Refused):
                g.plan_request(document, MODEL)
        document = request()
        document["state"] = "a" * (g.MAX_BYTES + 1)
        with self.assertRaises(g.Refused):
            g.plan_request(document, MODEL)

    def test_duplicate_keys_and_nonfinite_wire_values_are_refused(self):
        for data in (b'{"model":"a","model":"b"}', b'{"value":NaN}', b'{"value":Infinity}'):
            with self.assertRaises(g.Refused):
                g.decode(data)

    def test_controlled_scores_are_not_normalized_or_replaced_with_confidence(self):
        raw = {"decision_0": [("true", 0.125), ("false", 0.875)]}
        result = g.native_answer(raw, ["decision_0"], MODEL, 123, 8.5)
        self.assertEqual(result, {"model": MODEL, "answers": {
            "decision_0": {"type": "noul", "noul": 0.125}},
            "usage": {"input_tokens": 123, "output_tokens": 0}, "latency_ms": 8.5})
        self.assertEqual(raw["decision_0"], [("true", 0.125), ("false", 0.875)])

    def test_choice_packs_one_native_distribution_and_retains_actual_scores(self):
        document = request()
        document["questions"] = {"faculty": {"type": "choice", "instructions": "Choose from the owner field",
                                  "criteria": {"#0": "owner zero", "#1": "owner one", "abstain": "missing evidence"}}}
        text, tasks, bindings = g.plan_request(document, MODEL)
        self.assertEqual(json.loads(text), document["state"])
        self.assertEqual(bindings, {"decision_0": "faculty"})
        self.assertEqual(set(tasks["decision_0"]["labels"]), {"#0", "#1", "abstain"})
        scores = {"#0": .15, "#1": .75, "abstain": .1}
        result = g.native_answer({"decision_0": list(scores.items())}, ["decision_0"], MODEL, 81, 1.5, tasks)
        self.assertEqual(result["answers"]["decision_0"], {"type": "choice", "choice": "#1",
                         "probabilities": scores, "confidence": .75})
        with self.assertRaises(g.Refused):
            g.native_answer({"decision_0": [("#1", .75)]}, ["decision_0"], MODEL, 81, 1.5, tasks)

    def test_missing_duplicate_foreign_or_impossible_native_scores_are_refused(self):
        for raw in ({}, {"decision_0": [("true", .8)]},
                    {"decision_0": [("true", .8), ("true", .2)]},
                    {"decision_0": [("true", .8), ("foreign", .2)]},
                    {"decision_0": [("true", .8), ("false", .8)]},
                    {"decision_0": [("true", math.nan), ("false", .2)]},
                    {"decision_0": [("true", True), ("false", 0)]}):
            with self.assertRaises(g.Refused):
                g.native_answer(raw, ["decision_0"], MODEL, 123, 1)

    def test_missing_usage_or_invalid_latency_never_becomes_zero_usage(self):
        raw = {"decision_0": [("true", .8), ("false", .2)]}
        for tokens, latency in ((0, 1), (None, 1), (True, 1), (123, math.nan), (123, -1)):
            with self.assertRaises(g.Refused):
                g.native_answer(raw, ["decision_0"], MODEL, tokens, latency)

    def test_invalid_material_is_refused_before_any_model_dependency_import(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "material.json"
            path.write_bytes(g.canonical({"schema": "central.material-manifest/v1",
                "recipe": "gliner2.5-decide", "model_id": MODEL, "artifact_revision": "unpinned",
                "artifact_repo": "owner/model", "sdk_repo": "owner/sdk", "license": "Apache-2.0",
                "snapshot": directory, "sdk_directory": directory}))
            with self.assertRaisesRegex(g.Refused, "artifact_revision"):
                g.Discriminator(path, "cpu", 2048)

    def test_complete_card_provenance_and_path_types_are_required_before_load(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "material.json"
            basis = {"schema": "central.material-manifest/v1", "recipe": "gliner2.5-decide",
                     "model_id": MODEL, "artifact_revision": "a" * 40, "sdk_revision": "b" * 40,
                     "artifact_repo": "owner/model", "sdk_repo": "owner/sdk", "license": "Apache-2.0",
                     "snapshot": directory, "sdk_directory": directory}
            for key in ("artifact_repo", "sdk_repo", "license", "snapshot", "sdk_directory"):
                for value in (None, "", 1):
                    invalid = {**basis, key: value}
                    path.write_bytes(g.canonical(invalid))
                    with self.assertRaisesRegex(g.Refused, "provenance|paths"):
                        g.Discriminator(path, "cpu", 2048)

    def test_artifact_sizes_and_digests_are_typed_before_any_model_load(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "material.json"
            basis = {"schema": "central.material-manifest/v1", "recipe": "gliner2.5-decide",
                     "model_id": MODEL, "artifact_revision": "a" * 40, "sdk_revision": "b" * 40,
                     "artifact_repo": "owner/model", "sdk_repo": "owner/sdk", "license": "Apache-2.0",
                     "snapshot": directory, "sdk_directory": directory}
            for artifact in ({"bytes": True, "sha256": "a" * 64},
                             {"bytes": 0, "sha256": "a" * 64},
                             {"bytes": 10, "sha256": "A" * 64},
                             {"bytes": 10, "sha256": "short"}):
                path.write_bytes(g.canonical({**basis, "artifacts": {"model.safetensors": artifact}}))
                with self.assertRaisesRegex(g.Refused, "size/digest"):
                    g.Discriminator(path, "cpu", 2048)

    def test_unowned_material_refusal_preserves_actual_existing_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            foreign = target / "model" / "model.safetensors"
            foreign.parent.mkdir()
            foreign.write_bytes(b"foreign material must survive")
            with self.assertRaisesRegex(g.Refused, "unowned"):
                material.reserve_target(target, "a" * 64)
            self.assertEqual(foreign.read_bytes(), b"foreign material must survive")
            self.assertFalse((target / "gliner-material-preparation.json").exists())

    def test_actual_reservation_lock_blocks_second_preparation_and_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            model, lease, claim = material.reserve_target(target, "a" * 64)
            try:
                retained = (target / "gliner-material-preparation.json").read_bytes()
                self.assertTrue(model.is_dir())
                with self.assertRaises(g.Refused):
                    material.reserve_target(target, "a" * 64)
                with self.assertRaisesRegex(g.Refused, "already reserved"):
                    material.reserve_target(target, "a" * 64, claim["operation_ref"])
                self.assertEqual((target / "gliner-material-preparation.json").read_bytes(), retained)
            finally:
                lease.close()

    def test_explicit_recovery_follows_an_actual_exited_preparation_process(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            command = "from pathlib import Path;import sys,prepare_gliner_material as m;" \
                      "p,lease,claim=m.reserve_target(Path(sys.argv[1]),'a'*64);lease.close()"
            subprocess.run([sys.executable, "-c", command, str(target)],
                           cwd=Path(material.__file__).parent, check=True, timeout=5)
            prior = g.decode((target / "gliner-material-preparation.json").read_bytes())
            with self.assertRaisesRegex(g.Refused, "matching prior"):
                material.reserve_target(target, "different-recipe", prior["operation_ref"])
            model, lease, recovered = material.reserve_target(target, "a" * 64, prior["operation_ref"])
            try:
                self.assertTrue(model.is_dir())
                self.assertEqual(recovered["operation_ref"], prior["operation_ref"])
                self.assertNotEqual(recovered["pid"], prior["pid"])
            finally:
                lease.close()

    def test_incomplete_owned_publication_is_preserved_before_locked_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            _, lease, claim = material.reserve_target(target, "a" * 64)
            try:
                final = target / "decision-model-manifest.json"
                final.write_bytes(b'{"interrupted":')
                material.recover_publication(target, lease, claim)
                self.assertFalse(final.exists())
                retained = Path(claim["retained_publication"])
                self.assertEqual(retained.read_bytes(), b'{"interrupted":')
                self.assertEqual(g.decode((target / "gliner-material-preparation.json").read_bytes())[
                    "retained_publication"], str(retained))
            finally:
                lease.close()

    def test_failed_staging_validation_never_publishes_installed_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            _, lease, claim = material.reserve_target(target, "a" * 64)
            try:
                with self.assertRaises(g.Refused):
                    material.publish_manifest(target, lease, claim, {"schema": "wrong-owner"})
                self.assertFalse((target / "decision-model-manifest.json").exists())
                self.assertEqual(g.decode(Path(claim["staging_manifest"]).read_bytes()), {"schema": "wrong-owner"})
            finally:
                lease.close()


if __name__ == "__main__":
    unittest.main()
