"""Run: python3 -m unittest discover -s examples/agent-harness -v."""
import copy
from pathlib import Path
import tempfile
import unittest
from late import late_outcomes
from run import demonstrate, gate


class HostEnforcement(unittest.TestCase):
    def test_incident_replay_and_retry_repair(self):
        with tempfile.TemporaryDirectory() as directory:
            report = demonstrate(Path(directory))
            self.assertEqual(report["fresh"]["tool_calls"], 2)
            self.assertEqual(report["after_synthetic_feedback"]["tool_calls"], 3)
            self.assertEqual(report["guarded_after_feedback"]["tool_calls"], 2)
            self.assertEqual(report["incident_regression"], "passed")
            self.assertTrue(all(report[key]["replay_identical"] for key in ("fresh", "after_synthetic_feedback", "guarded_after_feedback")))

    def test_host_rejects_unreleased_refused_or_unknown_tool_actions(self):
        stance = {"status": "ok", "ignored": [], "habits": {"violations": 0},
                  "stance": {"action": {"level": "retry", "released": True}}}
        self.assertEqual(gate(stance, "local_probe", 0)[0], "dispatch")
        cases = [None, {}, dict(stance, status="partial"), dict(stance, status="refused"),
                 dict(stance, status="fallback"), dict(stance, ignored=["typo"]),
                 dict(stance, escalate="ask"), dict(stance, habits={"violations": 1}),
                 dict(stance, habits=None), dict(stance, stance=[]), dict(stance, stance={"action": None})]
        for key, value in (("released", False), ("level", "ask"), ("level", "arbitrary_command")):
            changed = copy.deepcopy(stance)
            changed["stance"]["action"][key] = value
            cases.append(changed)
        for result in cases:
            self.assertEqual(gate(result, "local_probe", 0)[0], "escalate", result)
        self.assertEqual(gate(stance, "shell", 0)[0], "escalate")
        self.assertEqual(gate(stance, "local_probe", 3)[0], "escalate")

    def test_late_outcomes_credit_their_own_decisions(self):
        with tempfile.TemporaryDirectory() as directory:
            report = late_outcomes(Path(directory))
            self.assertEqual(sorted(p["turn"] for p in report["paid"]), [0, 1, 2, 3])
            self.assertNotEqual([p["turn"] for p in report["paid"]], [0, 1, 2, 3], "the outcomes came back reordered")
            self.assertTrue(all(p["stance_matches"] and 0 < p["e"] <= 1 for p in report["paid"]))
            self.assertTrue(any(p["e"] < 1 for p in report["paid"]), "a late outcome moves less than a fresh one")
            self.assertEqual(report["open_after"], [])
            self.assertTrue(report["duplicate_refused_at"].endswith("inputs.for"))
            self.assertTrue(report["strand_unchanged_by_refusal"])
            self.assertEqual((report["verify_exit"], report["verify_ok"], report["actions"]), (0, True, {"checked": 4, "differs": None}))


if __name__ == "__main__":
    unittest.main()
