"""Exercise the failure-handling contract of the CI command runner."""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest

from ci import run
from ci_coverage import summarize
from ci_smoke import text_section


class RunnerTests(unittest.TestCase):
    def execute(self, source, timeout=10, max_bytes=1024):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stdout(io.StringIO()):
                code = run([sys.executable, "-c", source], "check", timeout, root, max_bytes)
            return code, (root / "check.log").read_bytes(), json.loads(
                (root / "check.json").read_text(encoding="utf-8")
            )

    def test_failure_keeps_status_and_output(self):
        code, log, data = self.execute("import sys; print('diagnostic'); sys.exit(7)")
        self.assertEqual(code, 7)
        self.assertIn(b"diagnostic", log)
        self.assertEqual(data["exit_code"], 7)

    def test_timeout_fails_and_retains_evidence(self):
        code, log, data = self.execute(
            "import time; print('started', flush=True); time.sleep(30)", timeout=1,
        )
        self.assertEqual(code, 124)
        self.assertTrue(data["timed_out"])
        self.assertIn(b"started", log)

    def test_noisy_output_is_bounded(self):
        code, log, data = self.execute("print('x' * 100000)", max_bytes=256)
        self.assertEqual(code, 0)
        self.assertEqual(len(log), 256)
        self.assertTrue(data["log_truncated"])

    def test_missing_program_fails_with_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stdout(io.StringIO()):
                code = run([str(root / "missing-program")], "missing", 2, root)
            self.assertNotEqual(code, 0)
            self.assertTrue(json.loads((root / "missing.json").read_text())["errors"])


class EvidenceTests(unittest.TestCase):
    def test_coverage_keeps_untested_member_and_rejects_missing_member(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[package]\nname="cli"\n[workspace]\nmembers=["linker"]\n')
            (root / "linker").mkdir()
            (root / "linker/Cargo.toml").write_text('[package]\nname="linker"\n')
            lcov = f"SF:{root}/src/main.rs\nDA:1,3\nend_of_record\n"
            with self.assertRaisesRegex(ValueError, "linker"):
                summarize(lcov, root)
            lcov += f"SF:{root}/linker/src/lib.rs\nDA:1,0\nend_of_record\n"
            report = summarize(lcov, root)
            self.assertIn("| cli | 1 | 1 | 100.00% |", report)
            self.assertIn("| linker | 0 | 1 | 0.00% |", report)

    def test_smoke_rejects_missing_or_truncated_elf_header(self):
        for data in (b"", b"not ELF", b"\x7fELF\x02\x01\x01" + bytes(57)):
            with self.assertRaises(AssertionError):
                text_section(data)


if __name__ == "__main__":
    unittest.main()
