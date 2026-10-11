"""Run: python3 -m unittest discover -s examples/agent-harness -v."""
import copy
from pathlib import Path
import tempfile
import unittest
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


if __name__ == "__main__":
    unittest.main()
