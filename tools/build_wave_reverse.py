#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Explicitly build the opt-in Wave reverse archive from an immutable source tree.

Ordinary Cargo builds never run this bootstrap or download a compiler. The input
repository is read only: git archive reads the pinned committed tree, regardless
of unrelated working-tree changes. The temporary source archive contains no Git
checkout or private/untracked files, and supplies its immutable std revision.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

WAVE_REVISION = "e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b"
ABI_VERSION = 1
METADATA_NAME = "reverse-core.meta"


def fnv64(data):
    """Compatibility fingerprint, not a cryptographic trust check."""
    value = 0xcbf29ce484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return f"{value:016x}"


def fingerprint_metadata(source, archive):
    return (f"schema=1\nabi_version={ABI_VERSION}\n"
            f"source_fnv64={fnv64(source)}\narchive_fnv64={fnv64(archive)}\n")


def run(arguments, **kwargs):
    subprocess.run(list(map(str, arguments)), check=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wave-source", required=True, type=Path,
                        help="Wave Git repository containing the official pinned commit; never modified")
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--rust-toolchain", default="1.89.0")
    parser.add_argument("--opt-level", choices=["0", "2"], default="2")
    parser.add_argument("--allow-network", action="store_true",
                        help="explicitly permit Cargo dependency downloads during this bootstrap")
    args = parser.parse_args()
    repository = args.wave_source.resolve()
    revision = subprocess.check_output(
        ["git", "-C", str(repository), "rev-parse", WAVE_REVISION + "^{commit}"], text=True).strip()
    if revision != WAVE_REVISION:
        parser.error("Wave repository does not contain the pinned immutable commit")
    rustc = subprocess.check_output(["rustc", "+" + args.rust_toolchain, "--version"], text=True).strip()
    if not rustc.startswith("rustc 1.89.0 "):
        parser.error("the pinned Wave compiler requires Rust 1.89.0")
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[1]
    kernels = [root / "reverse/wave/amd64.wave", root / "reverse/wave/cfg.wave"]
    kernel_contents = [path.read_bytes() for path in kernels]
    kernel_bytes = b"\n".join(kernel_contents)
    environment = os.environ.copy()
    environment["WAVE_STD_REVISION"] = WAVE_REVISION
    environment["CARGO_INCREMENTAL"] = "0"
    # Neither the mutable checkout nor an installed wavec supplies compiler/std.
    archive = subprocess.check_output(["git", "-C", str(repository), "archive", "--format=tar", WAVE_REVISION])
    with tempfile.TemporaryDirectory(prefix=".wave-reverse-", dir=output) as temporary:
        temporary = Path(temporary)
        source = temporary / "source"
        source.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as tree:
            tree.extractall(source, filter="data")
        command = ["cargo", "+" + args.rust_toolchain, "build", "--locked", "--jobs", "2",
                   "--manifest-path", source / "Cargo.toml", "--target-dir", output / "bootstrap", "--bin", "wavec"]
        if not args.allow_network:
            command.append("--offline")
        run(command, cwd=source, env=environment)
        compiler = output / "bootstrap/debug/wavec"
        # A single compilation unit lets the CFG call the same Wave decoder.
        kernel = temporary / "reverse.wave"
        kernel.write_bytes(kernel_bytes)
        obj = temporary / "reverse.o"
        run([compiler, "-O" + args.opt_level, "-C", "relocation-model=pic", "--std-root", source / "std",
             "--target=x86_64-unknown-linux-gnu", "build", kernel, "--freestanding",
             "--emit=obj", "-o", obj, "--target-dir", temporary / "wave-build"], cwd=temporary)
        candidate = temporary / "libwhale_wave_reverse.a"
        run([environment.get("AR", "ar"), "crsD", candidate, obj])
        provenance = {
            "abi_version": ABI_VERSION, "wave_revision": WAVE_REVISION,
            "backend": "wave-llvm-bootstrap", "host": "x86_64-unknown-linux-gnu",
            "rustc": rustc, "opt_level": args.opt_level,
            "compiler_sha256": digest(compiler), "archive_sha256": digest(candidate),
            "kernels": {str(path.relative_to(root)): hashlib.sha256(content).hexdigest()
                        for path, content in zip(kernels, kernel_contents)},
        }
        # Prepare every publication file before touching the last-good set.
        # Each rename is atomic. An interruption between renames is rejected by
        # the fingerprint guard rather than silently linking mixed generations.
        metadata = temporary / "provenance.json"
        metadata.write_text(json.dumps(provenance, indent=2) + "\n")
        fingerprint = temporary / METADATA_NAME
        fingerprint.write_text(fingerprint_metadata(kernel_bytes, candidate.read_bytes()))
        os.replace(candidate, output / candidate.name)
        os.replace(metadata, output / metadata.name)
        os.replace(fingerprint, output / fingerprint.name)
    print(f"WHALE_WAVE_REVERSE_DIR={output}")


if __name__ == "__main__":
    main()
