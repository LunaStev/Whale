"""Test bootstrap publication without installing Wave or LLVM."""
import contextlib
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import build_wave_elf


class ArchivePublicationTests(unittest.TestCase):
    def run_case(self, fail_archive=False, existing=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            out = root / "output"
            out.mkdir()
            archive = out / "libwhale_wave_elf.a"
            if existing:
                archive.write_bytes(b"previous good archive")

            def fake_run(command, **kwargs):
                if command[0] == "cargo":
                    return
                if "-o" in command:
                    Path(command[command.index("-o") + 1]).write_bytes(b"object")
                    return
                Path(command[2]).write_bytes(b"new archive")
                if fail_archive:
                    raise subprocess.CalledProcessError(1, command)

            with patch.object(sys, "argv", ["build_wave_elf.py", "--wave-source", str(source), "--out-dir", str(out)]), \
                    patch.object(build_wave_elf.subprocess, "check_output", side_effect=[build_wave_elf.WAVE_REVISION, ""]), \
                    patch.object(build_wave_elf.subprocess, "run", side_effect=fake_run), \
                    contextlib.redirect_stdout(io.StringIO()):
                if fail_archive:
                    with self.assertRaises(subprocess.CalledProcessError):
                        build_wave_elf.main()
                else:
                    build_wave_elf.main()
            if fail_archive and existing:
                self.assertEqual(archive.read_bytes(), b"previous good archive")
            elif fail_archive:
                self.assertFalse(archive.exists())
            else:
                self.assertEqual(archive.read_bytes(), b"new archive")
            self.assertEqual(set(p.name for p in out.iterdir()), {"elf_records.o", archive.name} if archive.exists() else {"elf_records.o"})

    def test_failed_archiver_preserves_prior_archive(self):
        self.run_case(fail_archive=True)

    def test_failed_archiver_does_not_publish_partial_archive(self):
        self.run_case(fail_archive=True, existing=False)

    def test_success_replaces_archive_and_cleans_temporary_files(self):
        self.run_case()
