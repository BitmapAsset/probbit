"""Keep the public host-enforcement example in the Python/CI regression suite."""
import os
from pathlib import Path
import subprocess
import sys
import unittest
import probbit


class AgentHarness(unittest.TestCase):
    def test_runnable_host_example(self):
        root = Path(__file__).resolve().parent.parent
        environment = dict(os.environ, PROBBIT_BIN=probbit.find_binary())
        result = subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", "examples/agent-harness", "-v"],
                                cwd=root, env=environment, capture_output=True, encoding="utf-8", timeout=30)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
