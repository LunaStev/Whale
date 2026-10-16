"""Release identity, reproducibility, archive integrity and exact-set regressions."""

import io
import json
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import release

VERSION = "0.1.0"
SOURCE = "a" * 40


def executable(target):
    kind, machine = release.TARGETS[target]
    data = bytearray(128)
    if kind == "elf":
        data[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<HH", data, 16, 2, machine)
    elif kind == "macho":
        data[:4] = b"\xcf\xfa\xed\xfe"
        struct.pack_into("<I", data, 4, machine)
        struct.pack_into("<I", data, 12, 2)
    else:
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<H", data, 68, machine)
    return bytes(data)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "binary"
        self.version = patch("release.invoke")
        invoke = self.version.start()
        self.addCleanup(self.version.stop)
        invoke.return_value.returncode = 0
        invoke.return_value.stdout = b"whale 0.1.0\n"

    def package(self, target, directory, **kwargs):
        self.binary.write_bytes(executable(target))
        return release.package(self.binary, self.root / directory, VERSION, target, SOURCE, **kwargs)

    def test_every_native_target_is_reproducible_and_readable(self):
        for target in release.TARGETS:
            with self.subTest(target=target):
                first = self.package(target, "one-" + target, socket=True)
                second = self.package(target, "two-" + target, socket=True)
                self.assertEqual(first.read_bytes(), second.read_bytes())
                manifest = release.validate(first, VERSION, target, SOURCE, execute=False)
                self.assertTrue(manifest["socket_cli"])
                self.assertEqual(manifest["source_commit"], SOURCE)
                self.assertEqual(manifest["target"], target)

    def test_identity_mismatch_and_duplicate_publication_fail(self):
        target = next(iter(release.TARGETS))
        self.package(target, "dist")
        with self.assertRaises(FileExistsError):
            self.package(target, "dist")
        with self.assertRaises(ValueError):
            release.package(self.binary, self.root / "bad", "9.9.9", target, SOURCE)
        with self.assertRaises(ValueError):
            release.validate(self.root / "dist" / release.archive_name(VERSION, target, SOURCE),
                             VERSION, target, "b" * 40, execute=False)
        with self.assertRaises(ValueError):
            release.validate_machine(executable(target), "aarch64-unknown-linux-gnu")
        with self.assertRaises(FileNotFoundError):
            release.package(self.root / "missing", self.root / "bad", VERSION, target, SOURCE)
        with patch("release.invoke") as invoke:
            invoke.return_value.returncode = 0
            invoke.return_value.stdout = b"whale 9.9.9\n"
            with self.assertRaisesRegex(ValueError, "binary version"):
                release.package(self.binary, self.root / "bad", VERSION, target, SOURCE)

    def test_checksums_require_the_exact_expected_archive_set(self):
        directory = self.root / "dist"
        targets = list(release.TARGETS)
        for target in targets:
            self.package(target, "dist")
        release.checksums(directory, VERSION, SOURCE, targets)
        release.checksums(directory, VERSION, SOURCE, targets, verify=True)
        manifest = (directory / "SHA256SUMS").read_text()
        self.assertEqual(manifest.splitlines(), sorted(manifest.splitlines(), key=lambda line: line.split("  ")[1]))
        (directory / "unexpected.zip").write_bytes(b"unexpected")
        with self.assertRaises(ValueError):
            release.checksums(directory, VERSION, SOURCE, targets, verify=True)
        (directory / "unexpected.zip").unlink()
        with self.assertRaises(ValueError):
            release.checksums(directory, VERSION, SOURCE, [targets[0], targets[0]], verify=True)
        (directory / "SHA256SUMS").write_text(manifest + manifest.splitlines()[0] + "\n")
        with self.assertRaises(ValueError):
            release.checksums(directory, VERSION, SOURCE, targets, verify=True)
        (directory / "SHA256SUMS").write_text(manifest)
        next(directory.glob("*.zip")).unlink()
        with self.assertRaises(ValueError):
            release.checksums(directory, VERSION, SOURCE, targets, verify=True)

    def test_archive_rejects_tampering_links_paths_duplicates_and_missing_files(self):
        for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]:
            original = self.package(target, "dist-" + target)
            with tempfile.TemporaryDirectory() as directory:
                release.unpack(original, Path(directory), VERSION, target, SOURCE)
                stem = release.identity(VERSION, target, SOURCE)
                entries = {p.name: p.read_bytes() for p in (Path(directory) / stem).iterdir()}
            for defect in ["tamper", "missing", "duplicate", "path", "symlink", "permission"]:
                with self.subTest(target=target, defect=defect):
                    items = list(entries.items())
                    if defect == "tamper":
                        items = [(n, d + b"changed" if n == release.binary_name(target) else d) for n, d in items]
                    if defect == "missing":
                        items.pop()
                    if defect == "duplicate":
                        items.append(items[0])
                    if defect == "path":
                        items[0] = ("../escape", items[0][1])
                    broken = self.root / ("broken.zip" if "windows" in target else "broken.tar.gz")
                    if broken.suffix == ".zip":
                        with zipfile.ZipFile(broken, "w") as writer:
                            for name, data in items:
                                info = zipfile.ZipInfo(f"{stem}/{name}")
                                mode = 0o120777 if defect == "symlink" else 0o100755
                                if defect == "permission":
                                    mode = 0o100644
                                info.external_attr = mode << 16
                                writer.writestr(info, data)
                    else:
                        with tarfile.open(broken, "w:gz") as writer:
                            for name, data in items:
                                info = tarfile.TarInfo(f"{stem}/{name}")
                                info.size = len(data)
                                info.mode = 0o644 if defect == "permission" else 0o755
                                if defect == "symlink":
                                    info.type, info.linkname = tarfile.SYMTYPE, "outside"
                                writer.addfile(info, io.BytesIO(data))
                    with self.assertRaises(ValueError):
                        release.validate(broken, VERSION, target, SOURCE, execute=False)


if __name__ == "__main__":
    unittest.main()
