"""Run CI commands with bounded logs, deadlines, and unmodified failure status."""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import threading
import time


def run(command, name, timeout, artifacts, max_bytes=8 * 1024 * 1024):
    artifacts.mkdir(parents=True, exist_ok=True)
    metadata = {
        "command": command,
        "platform": platform.platform(),
        "cwd": str(Path.cwd()),
        "timeout_seconds": timeout,
    }
    started = time.monotonic()
    truncated = False
    errors = []
    code = 1
    with (artifacts / f"{name}.log").open("wb") as log:
        try:
            process = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                start_new_session=os.name != "nt",
            )

            def capture():
                nonlocal truncated
                remaining = max_bytes
                try:
                    while chunk := process.stdout.read(8192):
                        kept = chunk[:remaining]
                        log.write(kept)
                        # Decode for consoles that do not use UTF-8 (notably Windows).
                        sys.stdout.write(kept.decode("utf-8", errors="replace"))
                        sys.stdout.flush()
                        remaining -= len(kept)
                        truncated |= len(kept) != len(chunk)
                except Exception as error:
                    errors.append(str(error))

            reader = threading.Thread(target=capture, daemon=True)
            reader.start()
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                metadata["timed_out"] = True
                if os.name == "nt":
                    subprocess.run(
                        [str(Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"),
                         "/PID", str(process.pid), "/T", "/F"],
                        check=False, timeout=10, capture_output=True,
                    )
                else:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                process.kill()
                process.wait(timeout=10)
                code = 124
            reader.join(timeout=10)
            if reader.is_alive():
                errors.append("output pipe remained open after command termination")
                code = code or 1
            else:
                process.stdout.close()
            if errors:
                code = code or 1
        except (OSError, subprocess.SubprocessError) as error:
            errors.append(str(error))
    metadata.update(
        exit_code=code, elapsed_seconds=round(time.monotonic() - started, 3),
        log_truncated=truncated, errors=errors,
    )
    (artifacts / f"{name}.json").write_text(
        json.dumps(metadata, indent=2) + "\n", encoding="utf-8",
    )
    print(json.dumps(metadata), flush=True)
    return code if code >= 0 else 128 - code


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="operation", required=True)
    host = commands.add_parser("host")
    host.add_argument("--expected", required=True)
    runner = commands.add_parser("run")
    runner.add_argument("--name", required=True)
    runner.add_argument("--timeout", type=int, default=900)
    runner.add_argument("--artifacts", type=Path, default=Path(".ci-artifacts"))
    runner.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.operation == "host":
        result = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True)
        print(result.stdout)
        if f"host: {args.expected}" not in result.stdout.splitlines():
            parser.error(f"Rust host is not the expected native host: {args.expected}")
        return 0
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or args.timeout <= 0 or not re.fullmatch(r"[a-zA-Z0-9_-]+", args.name):
        parser.error("supply a command, positive timeout, and a simple log name")
    return run(command, args.name, args.timeout, args.artifacts)


if __name__ == "__main__":
    sys.exit(main())
