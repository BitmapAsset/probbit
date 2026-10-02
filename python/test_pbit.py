"""Tests for python/pbit.py against the release binary (stdlib unittest; run: python3 python/test_pbit.py)."""
import json, os, unittest
import pbit

EX = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples")


def load(name):
    with open(os.path.join(EX, name)) as f:
        return json.load(f)


class PbitWrapper(unittest.TestCase):
    def test_run_exact_knapsack(self):
        a = pbit.run(load("knapsack-20.json"))
        self.assertEqual(a["verdict"], "exact"); self.assertEqual(len(a["plan"]), 20)
        self.assertTrue(all(0.0 <= p <= 1.0 for o in a["marginals"].values() for p in o.values()))

    def test_exact_op_and_text_input(self):
        with open(os.path.join(EX, "agent-plan-6.json")) as f:
            a = pbit.exact(f.read())
        self.assertEqual(a["verdict"], "exact")

    def test_sample_fixed_work_is_deterministic(self):
        prog = load("knapsack-20.json")
        a, b = (pbit.sample(prog, sweeps=300, seed=3, polish_ms=0) for _ in range(2))
        self.assertIn(a["verdict"], ("diagnostics_passed", "partial", "refused"))
        self.assertEqual((a["plan"], a["marginals"]), (b["plan"], b["marginals"]))

    def test_decide_router_demo(self):
        a = pbit.decide(pbit.demo(tasks=12, seed=1), budget_ms=100, collective=True)
        self.assertIn(a["verdict"], ("exact", "diagnostics_passed", "partial", "refused")); self.assertEqual(len(a["plan"]), 12)

    def test_infeasible_is_an_answer(self):
        prog = {"pbit_ir": 1, "values": ["a"], "vars": [{"id": "x"}, {"id": "y"}], "caps": [{"value": "a", "limit": 1}]}
        a = pbit.run(prog)
        self.assertEqual(a["verdict"], "infeasible")

    def test_input_error_is_typed(self):
        with self.assertRaises(pbit.PbitInputError) as e:
            pbit.run({"variables": "x"})
        self.assertEqual((e.exception.code, e.exception.path, e.exception.exit_code), ("schema", "variables", 2))

    def test_flag_error_is_typed(self):
        with self.assertRaises(pbit.PbitInputError) as e:
            pbit.run(load("knapsack-20.json"), budget_ms=-1)
        self.assertEqual(e.exception.code, "flag"); self.assertIn("--budget-ms", e.exception.message)

    def test_bad_op_and_timeout(self):
        with self.assertRaises(ValueError):
            pbit.run({}, op="nope")
        with self.assertRaises(pbit.PbitTimeout):
            pbit.decide(pbit.demo(tasks=24, seed=1), mode="sample", budget_ms=3000, timeout_s=0.2)

    def test_emoji_ids_round_trip(self):
        # json.dumps escapes them by default ("\ud83d\ude00"); they decoded to U+FFFD, came back changed and collided
        doc = pbit.demo(tasks=3, seed=1); doc["tasks"][0]["id"] = "T\U0001F600"; doc["tasks"][1]["id"] = "T\U0001F601"
        a = pbit.decide(doc, budget_ms=50)
        self.assertEqual(sorted(a["plan"]), sorted(t["id"] for t in doc["tasks"]))

    def test_deadline(self):
        a = pbit.run(load("denoise-8x12.json"), deadline_ms=500)  # `met` depends on machine load; the contract is the field
        self.assertEqual(a["deadline"]["ms"], 500); self.assertIsInstance(a["deadline"]["met"], bool)
        self.assertEqual([x["phase"] for x in a["phases"]][0], "parse")


if __name__ == "__main__":
    unittest.main()
