"""Tests for the persona functions of python/probbit.py (stdlib unittest, Python 3.9; run: python3 python/test_persona.py).
They drive `probbit persona` through the wrapper and check its documents against the goldens in examples/persona/golden/."""
import json, os, unittest
import probbit

EX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "persona")
FX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "probbit-cli", "tests", "fixtures", "persona")


def load(name):
    with open(os.path.join(EX, name), encoding="utf-8") as f:
        return json.load(f)


def golden(name, f):
    with open(os.path.join(EX, "golden", name, f), encoding="utf-8") as fh:
        return [json.loads(x) for x in fh.read().splitlines()] if f.endswith(".jsonl") else json.load(fh)


class Persona(unittest.TestCase):
    def test_init_turn_and_replay_equal_the_goldens(self):
        script = load("workday.json")["turns"]
        for name in ("ops-engineer", "tutor", "trader-assistant"):
            path = os.path.join(EX, name + ".yaml")
            st = probbit.persona_init(path)
            self.assertEqual(st, golden(name, "state0.json"))
            stances = []
            for inputs in script:
                r = probbit.persona_turn(path, st, inputs)
                stances.append(r["stance"]); st = r["state"]
            self.assertEqual(stances, golden(name, "workday.jsonl"), name)
            self.assertEqual(st, golden(name, "final-state.json"))
            self.assertEqual(probbit.persona_replay(path, {"turns": script}), stances)

    def test_a_dict_persona_is_the_same_individual(self):
        doc = load("tutor.json")
        a = probbit.persona_init(doc, seed=3); b = probbit.persona_init(os.path.join(EX, "tutor.yaml"), seed=3)
        self.assertEqual(a, b); self.assertEqual(a["seed"], 3)
        self.assertNotEqual(a["genes"], probbit.persona_init(doc, seed=4)["genes"])  # another seed, another individual
        r = probbit.persona_turn(doc, a, {"loss": True, "sentiment": "negative"}, timing=True)
        self.assertEqual(r["stance"]["stance"]["humour"]["level"], "none")  # the habit: no jokes on a loss
        self.assertEqual(r["stance"]["habits"]["violations"], 0); self.assertIn("timing", r["stance"]); self.assertLessEqual(r["stance"]["line_tokens"], 40)

    def test_goal_signals_pass_through_unchanged(self):
        # drives (docs/persona.md 2.9): the same signature; `goals` is one more input, handed to the engine as given
        path = os.path.join(FX, "drives-adversary.json")
        with open(os.path.join(FX, "drives-adversary-script.json"), encoding="utf-8") as f:
            script = json.load(f)["turns"]
        with open(os.path.join(FX, "drives-adversary-replay.jsonl"), encoding="utf-8") as f:
            gold = [json.loads(x) for x in f.read().splitlines()]
        st = probbit.persona_init(path, seed=4); self.assertIn("drives", st)
        stances = []
        for inputs in script:
            r = probbit.persona_turn(path, st, inputs)
            stances.append(r["stance"]); st = r["state"]
        self.assertEqual(stances, gold)
        self.assertEqual(probbit.persona_replay(path, {"turns": script}, seed=4), gold)
        r = probbit.persona_turn(path, st, {"goals": {"chores": {"deadline_hours": 5}}, "security": True})
        self.assertEqual(r["stance"]["pursue"]["goal"], "chores")  # the must-do habit
        self.assertEqual(r["stance"]["inputs"]["goals"], {"chores": {"deadline_hours": 5}})  # logged as given
        with self.assertRaises(probbit.ProbbitInputError) as e:
            probbit.persona_turn(path, st, {"goals": {"sleep": {"cue": True}}})
        self.assertEqual(e.exception.path, "inputs.goals.sleep")

    def test_errors_are_typed(self):
        doc = load("ops-engineer.json")
        st = probbit.persona_init(doc)
        with self.assertRaises(probbit.ProbbitInputError) as e:
            probbit.persona_turn(doc, st, {"stakes": 3})
        self.assertEqual((e.exception.code, e.exception.path), ("persona", "inputs.stakes"))
        bad = dict(doc, traits=[dict(doc["traits"][0], sprad=1)] + doc["traits"][1:])
        with self.assertRaises(probbit.ProbbitInputError) as e:
            probbit.persona_init(bad)
        self.assertEqual((e.exception.code, e.exception.path), ("persona", "traits[0].sprad"))
        with self.assertRaises(probbit.ProbbitInputError) as e:  # a state of another persona
            probbit.persona_turn(load("tutor.json"), st, {})
        self.assertEqual(e.exception.path, "state.persona.digest")
        edited = dict(st, turn=st["turn"] + 1)
        with self.assertRaises(probbit.ProbbitInputError) as e:
            probbit.persona_turn(doc, edited, {})
        self.assertEqual(e.exception.path, "state.digest")

    def test_a_refusal_is_an_answer_with_the_habits_only(self):
        doc = load("tutor.json"); doc["engine"] = dict(doc.get("engine") or {}, op="sample", sweeps=8)
        st = probbit.persona_init(doc)
        r = probbit.persona_turn(doc, st, {"loss": True})
        s = r["stance"]
        self.assertIn(s["status"], ("refused", "partial", "fallback")); self.assertTrue(s["escalate"])
        unvouched = [t for t, e in s["stance"].items() if not e["released"]]
        self.assertTrue(unvouched)
        self.assertIn("no jokes, be kind", s["line"])  # a habit in force is always safe to state

    def test_a_state_rewritten_by_python_still_reads(self):
        # Python writes 0.0 where the CLI wrote 0: the canonical number rule makes it the same state (same digest)
        doc = load("trader-assistant.json")
        st = json.loads(json.dumps(probbit.persona_init(doc)), parse_int=float)
        self.assertEqual(probbit.persona_turn(doc, st, {"error": True})["stance"]["turn"], 0)


    def test_fuzz_and_prove_on_the_0_5_0_tutor(self):
        # the tutor as it shipped in 0.5.0: one upset message can make an individual playful; its loss habit holds by construction
        tutor = os.path.join(EX, "..", "..", "probbit-cli", "tests", "fixtures", "persona", "tutor-0.5.0.yaml")
        upset = {"when": {"sentiment": "negative"}, "then": {"humour": {"at_most": "light"}}}
        f = probbit.persona_fuzz(tutor, never=upset, seeds="0-4", scripts=10, grid=[0, 1])
        self.assertEqual(f["probbit_persona_fuzz"], 1)
        self.assertTrue(f["found"])
        self.assertEqual(f["properties"][0]["shortest"]["script"], [{"sentiment": "negative"}])
        p = probbit.persona_prove(tutor, props=[dict(upset, id="not_playful"), {"id": "loss", "when": {"loss": True}, "then": {"humour": ["none"]}}], seeds=[0, 1])
        self.assertEqual(p["probbit_persona_prove"], 1)
        self.assertEqual([x["verdict"] for x in p["properties"]], ["unknown", "held_by_construction"])
        with self.assertRaises(probbit.ProbbitInputError) as e:
            probbit.persona_prove(tutor, never={"when": {"sentimentx": "negative"}, "then": {"humour": ["none"]}})
        self.assertEqual(e.exception.code, "persona")
        with self.assertRaises(TypeError):
            probbit.persona_fuzz(tutor)



class Live(unittest.TestCase):
    def test_live_event_logs_a_strand_that_verifies(self):
        import tempfile
        tutor = os.path.join(EX, "tutor.yaml")
        with tempfile.TemporaryDirectory() as d:
            strand = os.path.join(d, "pip.strand")
            r = probbit.live_event(tutor, None, {"loss": True}, seed=2, strand=strand)
            self.assertEqual(r["stance"]["stance"]["humour"]["level"], "none")
            self.assertEqual((r["strand"]["events"], r["stance"]["inputs"]["loss"]), (1, True))
            for ev in ({"praise": True, "elapsed_hours": 2.5}, {"sentiment": "negative", "elapsed_hours": 12}):
                r = probbit.live_event(tutor, r["state"], ev, strand=strand)
            self.assertEqual(r["strand"]["events"], 3)
            v = probbit.live_verify(strand)
            self.assertEqual((v["ok"], v["events"], v["last_line"], v["final_state"]), (True, 3, r["strand"]["head"], r["state"]["digest"]))
            with open(strand, encoding="utf-8", newline="") as f:  # bytes as written: a strand is verified byte for byte (no CRLF on Windows)
                text = f.read()
            with open(strand, "w", encoding="utf-8", newline="") as f:
                f.write(text.replace('"elapsed_hours":2.5', '"elapsed_hours":2.6'))
            self.assertEqual(probbit.live_verify(strand), {"ok": False, "line": 3, "diverges": "the stance differs"})
            with self.assertRaises(probbit.ProbbitInputError) as e:
                probbit.live_event(tutor, r["state"], {"elapsed_hours": -1})
            self.assertEqual(e.exception.path, "events[0].event.elapsed_hours")


class BoundaryErrors(unittest.TestCase):
    def test_explicit_missing_binary_does_not_fall_back(self):
        with self.assertRaises(probbit.ProbbitError):
            probbit.persona_init(load("tutor.json"), binary="/not/a/probbit/binary")

    def test_falsey_malformed_events_are_not_replaced_with_empty_objects(self):
        doc = load("tutor.json")
        state = probbit.persona_init(doc)
        for event in (False, [], "", 0):
            with self.subTest(event=event):
                with self.assertRaises(probbit.ProbbitInputError):
                    probbit.persona_turn(doc, state, event)
                with self.assertRaises(probbit.ProbbitInputError):
                    probbit.live_event(doc, state, event)

    def test_replay_error_after_a_good_turn_retains_its_type(self):
        with self.assertRaises(probbit.ProbbitInputError) as err:
            probbit.persona_replay(load("tutor.json"), [{}, {"sentiment": "not-a-level"}])
        self.assertEqual(err.exception.code, "persona")
        self.assertIn("sentiment", err.exception.path)

    def test_checkpoint_event_receipt_names_the_committed_event_and_checkpoint(self):
        import pathlib, subprocess, tempfile
        doc = {"probbit_persona": 1, "identity": {"name": "Receipt", "version": "1"},
               "traits": [{"id": "action", "levels": ["retry", "ask"], "logw": [1, 0]}]}
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            policy, state, strand, events = [root / name for name in ("policy.json", "state.json", "receipt.strand", "events.jsonl")]
            policy.write_text(json.dumps(doc), encoding="utf-8")
            state.write_text(json.dumps(probbit.persona_init(doc)), encoding="utf-8")
            events.write_text("{}\n" * 999, encoding="utf-8")
            subprocess.run([probbit.find_binary(), "live", str(policy), "--state", str(state), "--clock", "fixed", "--events", str(events), "--strand", str(strand)], check=True, capture_output=True)
            receipt = probbit.live_event(doc, json.loads(state.read_text()), {}, strand=strand)
            verified = probbit.live_verify(strand)
            self.assertEqual(receipt["strand"]["events"], 1000)
            self.assertEqual(receipt["strand"]["head"], verified["last_line"])
            self.assertEqual(verified["checkpoints"], 1)

    def test_later_control_or_partial_append_cannot_replace_our_event_receipt(self):
        import hashlib, pathlib, subprocess, tempfile
        from unittest.mock import patch
        original = probbit._live_call
        with tempfile.TemporaryDirectory() as directory:
            strand = pathlib.Path(directory) / "receipt.strand"
            def after_event(*args, **kwargs):
                result = original(*args, **kwargs)
                subprocess.run([probbit.find_binary(), "live", "control", str(strand), "pause", "--by", "human:owner", "--reason", "test"], check=True, capture_output=True)
                with strand.open("a", encoding="utf-8") as stream:
                    stream.write('{"incomplete":')  # a subsequent writer has not finished yet
                return result
            with patch.object(probbit, "_live_call", side_effect=after_event):
                receipt = probbit.live_event(load("tutor.json"), event={}, strand=strand)
            event_line = strand.read_text().splitlines()[1]
            expected_head = "sha256:" + hashlib.sha256(event_line.encode()).hexdigest()
            self.assertEqual(receipt["strand"], {"path": str(strand), "events": 1, "head": expected_head})

    def test_pause_is_typed_and_never_appends(self):
        import pathlib, subprocess, tempfile
        with tempfile.TemporaryDirectory() as directory:
            strand = pathlib.Path(directory) / "test.strand"
            doc = load("tutor.json")
            first = probbit.live_event(doc, event={}, strand=strand)
            subprocess.run([probbit.find_binary(), "live", "control", str(strand), "pause", "--by", "human:owner", "--reason", "test"], check=True, capture_output=True)
            before = strand.read_bytes()
            with self.assertRaises(probbit.ProbbitControlError) as err:
                probbit.live_event(doc, first["state"], {}, strand=strand)
            self.assertEqual(err.exception.code, "paused")
            self.assertEqual(strand.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
