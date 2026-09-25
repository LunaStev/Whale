#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Build the optional Wave ELF module using a pinned source bootstrap compiler."""
import argparse
import os
from pathlib import Path
import subprocess

WAVE_REVISION = "8a465e30aeea4b817d925cdd0e8d08c1bb029c9a"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wave-source", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    args = parser.parse_args()
    source = args.wave_source.resolve()
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
    if revision != WAVE_REVISION:
        parser.error(f"expected Wave {WAVE_REVISION}, found {revision}")
    dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=source, text=True)
    if dirty:
        parser.error("Wave bootstrap source has tracked modifications")
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    # Rebuild from the verified sources rather than trusting an unrelated wavec.
    subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(source / "Cargo.toml"), "--target-dir", str(out / "bootstrap")], cwd=source, check=True)
    compiler = out / "bootstrap/debug/wavec"
    root = Path(__file__).resolve().parents[1]
    obj = out / "elf_records.o"
    subprocess.run([str(compiler), "-O0", "--target=x86_64-unknown-linux-gnu", "build", str(root / "object/wave/elf_records.wave"), "--freestanding", "--emit=obj", "-o", str(obj), "--target-dir", str(out / "wave-build")], check=True)
    archive = out / "libwhale_wave_elf.a"
    archive.unlink(missing_ok=True)
    subprocess.run([os.environ.get("AR", "ar"), "crsD", str(archive), str(obj)], check=True)
    print(f"WHALE_WAVE_ELF_DIR={out}")


if __name__ == "__main__":
    main()
