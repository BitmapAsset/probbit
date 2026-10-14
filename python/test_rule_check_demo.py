"""Smoke test for examples/rule-check/demo.py (stdlib unittest, Python 3.9; run: python3 python/test_rule_check_demo.py).
It runs the demo small (4 individuals, 20 rewarded events) against the binary the wrapper finds."""
import os, subprocess, sys, unittest
import probbit

DEMO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "rule-check", "demo.py")


class RuleCheckDemo(unittest.TestCase):
    def test_small_run(self):
        env = dict(os.environ, PROBBIT_BIN=probbit.find_binary())
        r = subprocess.run([sys.executable, DEMO, "--seeds", "0-3", "--events", "20"], stdout=subprocess.PIPE,
                           stderr=subprocess.PIPE, universal_newlines=True, env=env, timeout=120)
        self.assertEqual(r.returncode, 0, r.stderr)
        lines = r.stdout.splitlines()
        self.assertTrue(lines[0].startswith("seed  day-0 check"), lines[0])
        self.assertEqual([ln.split()[0] for ln in lines[1:5]], ["0", "1", "2", "3"])
        self.assertIn("   3  FLAGGED", r.stdout)
        self.assertIn("all strands replay byte for byte: True", r.stdout)
        self.assertIn("population, seeds 0-99: flagged 24,", r.stdout)
        self.assertIn("after one added habit: check says held_by_construction for 4/4; drifted 0/4;", lines[-1])


if __name__ == "__main__":
    unittest.main()
