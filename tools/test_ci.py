"""Exercise the failure-handling contract of the CI command runner."""

import contextlib
import io
import json
import subprocess
from pathlib import Path
import sys
import tempfile
import time
import unittest

from ci import run, _WINDOWS_GATE
from ci_coverage import summarize
from ci_smoke import text_section


class RunnerTests(unittest.TestCase):
    def test_windows_gate_waits_for_child_status_and_requires_handshake(self):
        for token, expected in [(b"1", 7), (b"", 125)]:
            process = subprocess.Popen([sys.executable, "-c", _WINDOWS_GATE,
                sys.executable, "-c", "import sys,time; time.sleep(0.05); print('child finished'); sys.exit(7)"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            stdout, stderr = process.communicate(token, timeout=5)
            self.assertEqual(process.returncode, expected, stderr)
            self.assertEqual(b"child finished" in stdout, token == b"1")

    def execute(self, source, timeout=10, max_bytes=1024, console=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stdout(console if console is not None else io.StringIO()):
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

    def test_utf8_output_survives_read_boundaries(self):
        for character in ["é", "고", "🐋"]:
            for split in range(1, len(character.encode())):
                with self.subTest(character=character, split=split):
                    payload = "x" * (8192 - split) + character + "END"
                    console = io.StringIO()
                    code, log, data = self.execute(
                        f"import sys; sys.stdout.buffer.write({payload.encode()!r})",
                        max_bytes=16384, console=console,
                    )
                    self.assertEqual(code, 0)
                    self.assertEqual(log, payload.encode())
                    self.assertTrue(console.getvalue().startswith(payload))
                    self.assertFalse(data["errors"])

    def test_incomplete_or_invalid_utf8_is_replaced_only_on_console(self):
        for payload, limit, expected, truncated in [
            (b"ok\xffend", 1024, "ok\ufffdend", False),
            (b"ok\xf0\x9f", 1024, "ok\ufffd", False),
            ("고래".encode(), 4, "고\ufffd", True),
        ]:
            with self.subTest(payload=payload, limit=limit):
                console = io.StringIO()
                code, log, data = self.execute(
                    f"import sys; sys.stdout.buffer.write({payload!r})",
                    max_bytes=limit, console=console,
                )
                self.assertEqual(code, 0)
                self.assertEqual(log, payload[:limit])
                self.assertTrue(console.getvalue().startswith(expected + "{"))
                self.assertEqual(data["log_truncated"], truncated)

    def test_missing_program_fails_with_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with contextlib.redirect_stdout(io.StringIO()):
                code = run([str(root / "missing-program")], "missing", 2, root)
            self.assertNotEqual(code, 0)
            self.assertTrue(json.loads((root / "missing.json").read_text())["errors"])

    def test_parent_exit_cleans_descendant_before_post_return_side_effect(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pid_file, side_effect = root / "pid", root / "escaped"
            child = (
                "import os,time,pathlib; "
                f"pathlib.Path({str(pid_file)!r}).write_text(str(os.getpid())); "
                "time.sleep(1.5); "
                f"pathlib.Path({str(side_effect)!r}).write_text('survived')"
            )
            parent = (
                "import subprocess,sys,time,pathlib; "
                f"subprocess.Popen([sys.executable,'-c',{child!r}]); "
                f"p=pathlib.Path({str(pid_file)!r}); "
                "\nwhile not p.exists(): time.sleep(0.01)"
            )
            # The unrelated control process must survive cleanup of the owned
            # tree; its side effect also gives a positive observation window.
            control_file = root / "control"
            control = subprocess.Popen([sys.executable, "-c",
                "import time,pathlib; time.sleep(1.8); "
                f"pathlib.Path({str(control_file)!r}).write_text('alive')"])
            try:
                with contextlib.redirect_stdout(io.StringIO()):
                    started = time.monotonic()
                    code = run([sys.executable, "-c", parent], "descendant", 2, root)
                self.assertNotEqual(code, 0)
                self.assertLess(time.monotonic() - started, 3)
                self.assertTrue(pid_file.exists())
                control.wait(timeout=5)
                self.assertTrue(control_file.exists())
                self.assertFalse(side_effect.exists())
                data = json.loads((root / "descendant.json").read_text())
                self.assertIn("output pipe remained open", " ".join(data["errors"]))
            finally:
                if control.poll() is None:
                    control.kill()
                    control.wait()


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
