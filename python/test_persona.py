"""Tests for the persona functions of python/probbit.py (stdlib unittest, Python 3.9; run: python3 python/test_persona.py).
They drive `probbit persona` through the wrapper and check its documents against the goldens in examples/persona/golden/."""
import json, os, unittest
import probbit

EX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "persona")


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


if __name__ == "__main__":
    unittest.main()
