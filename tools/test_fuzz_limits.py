import subprocess
import sys
import unittest
from pathlib import Path


@unittest.skipUnless(sys.platform == "linux", "Linux resource limits")
class FuzzLimitsTests(unittest.TestCase):
    def run_child(self, code):
        return subprocess.run([sys.executable, str(Path(__file__).with_name("fuzz_limits.py")),
                               "--memory-mib", "64", "--", sys.executable, "-c", code],
                              capture_output=True, text=True, timeout=5)

    def test_small_child_runs_and_large_allocation_fails(self):
        self.assertEqual(self.run_child("print('small')").returncode, 0)
        result = self.run_child("x=bytearray(128*1024*1024)")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("MemoryError", result.stderr)

    def test_child_exit_status_survives_exec(self):
        self.assertEqual(self.run_child("raise SystemExit(19)").returncode, 19)
