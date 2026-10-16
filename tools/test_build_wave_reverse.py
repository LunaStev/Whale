"""Exercise publication and the actual std-only Cargo fingerprint guard offline."""
import contextlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import build_wave_reverse as helper

ROOT = Path(__file__).resolve().parents[1]


class ArchiveTests(unittest.TestCase):
    def publication(self, failure=None, existing=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            out = root / "output"
            out.mkdir()
            names = ["libwhale_wave_reverse.a", "provenance.json", helper.METADATA_NAME]
            if existing:
                for name in names:
                    (out / name).write_bytes(b"previous " + name.encode())
            tar = io.BytesIO()
            with tarfile.open(fileobj=tar, mode="w"):
                pass

            def fake_run(command, **kwargs):
                if str(command[0]) == "cargo":
                    compiler = out / "bootstrap/debug/wavec"
                    compiler.parent.mkdir(parents=True, exist_ok=True)
                    compiler.write_bytes(b"pinned compiler")
                    if failure == "bootstrap":
                        raise subprocess.CalledProcessError(1, command)
                elif "-o" in command:
                    Path(command[command.index("-o") + 1]).write_bytes(b"object")
                    if failure == "compile":
                        raise subprocess.CalledProcessError(1, command)
                else:
                    Path(command[2]).write_bytes(b"new archive")
                    if failure == "archive":
                        raise subprocess.CalledProcessError(1, command)

            arguments = ["build_wave_reverse.py", "--wave-source", str(source), "--out-dir", str(out)]
            with patch.object(sys, "argv", arguments), \
                    patch.object(helper.subprocess, "check_output", side_effect=[helper.WAVE_REVISION, "rustc 1.89.0 (test)", tar.getvalue()]), \
                    patch.object(helper, "run", side_effect=fake_run), \
                    contextlib.redirect_stdout(io.StringIO()):
                if failure:
                    with self.assertRaises(subprocess.CalledProcessError):
                        helper.main()
                else:
                    helper.main()
            for name in names:
                path = out / name
                if failure and existing:
                    self.assertEqual(path.read_bytes(), b"previous " + name.encode())
                elif failure:
                    self.assertFalse(path.exists())
                else:
                    self.assertTrue(path.is_file())
            self.assertFalse(any(p.name.startswith(".wave-reverse-") for p in out.iterdir()))
            if not failure:
                source_bytes = b"\n".join((ROOT / p).read_bytes() for p in ["reverse/wave/amd64.wave", "reverse/wave/cfg.wave"])
                self.assertEqual((out / helper.METADATA_NAME).read_text(), helper.fingerprint_metadata(source_bytes, b"new archive"))
                import json
                provenance = json.loads((out / "provenance.json").read_text())
                self.assertEqual(provenance["archive_sha256"], helper.digest(out / names[0]))
                self.assertEqual(provenance["wave_revision"], helper.WAVE_REVISION)

    def test_success_publishes_matching_fingerprints_and_provenance(self):
        self.publication()

    def test_failures_preserve_complete_last_good_set(self):
        for failure in ["bootstrap", "compile", "archive"]:
            with self.subTest(failure=failure):
                self.publication(failure)

    def test_failures_never_publish_partial_first_build(self):
        for failure in ["bootstrap", "compile", "archive"]:
            with self.subTest(failure=failure):
                self.publication(failure, existing=False)


class GuardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.guard = Path(cls.temporary.name) / "guard"
        subprocess.run([shutil.which("rustc") or "rustc", str(ROOT / "reverse/build.rs"),
                        "--edition=2021", "-o", str(cls.guard)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_fnv_known_vectors(self):
        self.assertEqual(helper.fnv64(b""), "cbf29ce484222325")
        self.assertEqual(helper.fnv64(b"hello"), "a430d84680aabd0b")

    def run_guard(self, corruption=None, enabled=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            kernels = root / "wave"
            kernels.mkdir()
            (kernels / "amd64.wave").write_bytes(b"decode\n")
            (kernels / "cfg.wave").write_bytes(b"cfg\n")
            archive = root / "libwhale_wave_reverse.a"
            archive.write_bytes(b"archive")
            metadata = root / helper.METADATA_NAME
            metadata.write_text(helper.fingerprint_metadata(b"decode\n\ncfg\n", b"archive"))
            if corruption == "source":
                (kernels / "cfg.wave").write_bytes(b"changed")
            elif corruption == "archive":
                archive.write_bytes(b"partial archive")
            elif corruption == "abi":
                metadata.write_text(metadata.read_text().replace("abi_version=1", "abi_version=2"))
            elif corruption == "missing":
                metadata.unlink()
            elif corruption == "duplicate":
                metadata.write_text(metadata.read_text() + "schema=1\n")
            elif corruption == "oversized":
                metadata.write_text("x" * 4097)
            env = os.environ.copy()
            env.update(CARGO_MANIFEST_DIR=str(root), HOST="x86_64-unknown-linux-gnu",
                       TARGET="x86_64-unknown-linux-gnu", WHALE_WAVE_REVERSE_DIR=str(root))
            env.pop("CARGO_FEATURE_WAVE", None)
            if enabled:
                env["CARGO_FEATURE_WAVE"] = "1"
            return subprocess.run([str(self.guard)], env=env, capture_output=True, text=True)

    def test_matching_guard_passes_and_tracks_every_input(self):
        result = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stderr)
        for path in ["amd64.wave", "cfg.wave", "reverse-core.meta", "libwhale_wave_reverse.a"]:
            self.assertIn("cargo:rerun-if-changed=", result.stdout)
            self.assertIn(path, result.stdout)
        self.assertIn("cargo:rustc-cfg=whale_wave_reverse", result.stdout)

    def test_stale_corrupt_and_wrong_abi_archives_are_rejected_before_link(self):
        for corruption, reason in [("source", "stale"), ("archive", "corrupt"),
                                   ("abi", "ABI revision"), ("missing", "missing"),
                                   ("duplicate", "duplicate"), ("oversized", "oversized")]:
            with self.subTest(corruption=corruption):
                result = self.run_guard(corruption)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(reason, result.stderr)
                self.assertNotIn("cargo:rustc-link-lib", result.stdout)

    @unittest.skipUnless(sys.platform == "linux" and os.uname().machine == "x86_64", "native bootstrap host")
    def test_cargo_rechecks_warm_source_and_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            (project / "src").mkdir()
            (project / "src/lib.rs").write_text("pub fn marker() {}\n")
            (project / "Cargo.toml").write_text('[package]\nname="guard-probe"\nversion="0.0.0"\nedition="2021"\n[features]\nwave=[]\n')
            shutil.copyfile(ROOT / "reverse/build.rs", project / "build.rs")
            (project / "wave").mkdir()
            for name in ["amd64.wave", "cfg.wave"]:
                shutil.copyfile(ROOT / "reverse/wave" / name, project / "wave" / name)
            out = project / "archive"
            out.mkdir()
            archive = out / "libwhale_wave_reverse.a"
            archive.write_bytes(b"archive")
            source = b"\n".join((project / "wave" / name).read_bytes() for name in ["amd64.wave", "cfg.wave"])
            (out / helper.METADATA_NAME).write_text(helper.fingerprint_metadata(source, b"archive"))
            environment = os.environ.copy()
            environment.update(WHALE_WAVE_REVERSE_DIR=str(out), CARGO_TARGET_DIR=str(project / "target"))
            command = ["cargo", "check", "--offline", "--features", "wave", "--manifest-path", str(project / "Cargo.toml")]

            def check(success, reason=None):
                result = subprocess.run(command, env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, success, result.stderr)
                if reason:
                    self.assertIn(reason, result.stderr)

            check(True)
            cfg = project / "wave/cfg.wave"
            original = cfg.read_bytes()
            cfg.write_bytes(original + b"\n")
            check(False, "stale Wave reverse archive")
            cfg.write_bytes(original)
            check(True)
            archive.write_bytes(b"damaged archive")
            check(False, "corrupt or mismatched")
            archive.write_bytes(b"archive")
            check(True)

    def test_feature_disabled_ignores_optional_archive(self):
        result = self.run_guard("missing", enabled=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo:rustc-link-lib", result.stdout)


if __name__ == "__main__":
    unittest.main()
