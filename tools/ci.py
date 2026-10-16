"""Run CI commands with bounded logs, deadlines, and unmodified failure status."""

import argparse
import codecs
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import threading
import time

# Windows CRT exec creates a new PID and destroys its caller. Keep the owned
# gate alive until the real child finishes so wait()/status still mean command
# completion. The child inherits its Job membership and output handles.
_WINDOWS_GATE = (
    "import subprocess,sys; token=sys.stdin.buffer.read(1); sys.stdin.close(); "
    "sys.exit(125) if token != b'1' else None; "
    "sys.exit(subprocess.call(sys.argv[1:],stdin=subprocess.DEVNULL))"
)


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
    process = None
    job = None
    reader = None
    stop_capture = threading.Event()
    with (artifacts / f"{name}.log").open("wb") as log:
        try:
            launched = command
            if os.name == "nt":
                from ci_windows import Job
                if not shutil.which(str(command[0])):
                    raise FileNotFoundError(f"program not found: {command[0]}")
                job = Job()
                # Only this gate exists before Job assignment; no command can
                # create descendants before the parent grants permission.
                launched = [sys.executable, "-c", _WINDOWS_GATE, *command]
            process = subprocess.Popen(
                launched, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                stdin=subprocess.PIPE if job else subprocess.DEVNULL,
                start_new_session=os.name != "nt",
            )
            if job:
                job.assign(process)
                process.stdin.write(b"1")
                process.stdin.close()
            os.set_blocking(process.stdout.fileno(), False)

            def capture():
                nonlocal truncated
                remaining = max_bytes
                decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
                try:
                    while True:
                        try:
                            chunk = os.read(process.stdout.fileno(), 8192)
                        except BlockingIOError:
                            if stop_capture.wait(0.01):
                                break
                            continue
                        if not chunk:
                            break
                        kept = chunk[:remaining]
                        log.write(kept)
                        # Decode for consoles that do not use UTF-8 (notably Windows).
                        sys.stdout.write(decoder.decode(kept))
                        sys.stdout.flush()
                        remaining -= len(kept)
                        truncated |= len(kept) != len(chunk)
                    sys.stdout.write(decoder.decode(b"", final=True))
                    sys.stdout.flush()
                except Exception as error:
                    errors.append(str(error))

            reader = threading.Thread(target=capture)
            reader.start()
            try:
                code = process.wait(timeout=max(0.001, timeout - (time.monotonic() - started)))
            except subprocess.TimeoutExpired:
                metadata["timed_out"] = True
                code = 124
            reader.join(timeout=min(0.25, max(0, timeout - (time.monotonic() - started))))
            if reader.is_alive():
                errors.append("output pipe remained open after command termination")
                code = code or 1
        except (OSError, subprocess.SubprocessError) as error:
            errors.append(str(error))
            code = code or 1
        finally:
            # Ownership outlives the direct child, including success followed by
            # a descendant retaining stdout. Always finish tree and reader cleanup
            # before closing their shared log. No PID-based tree discovery.
            try:
                if job:
                    job.close()
                elif process:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                if process:
                    if process.poll() is None:
                        process.kill()
                    process.wait(timeout=5)
            except (OSError, subprocess.SubprocessError) as error:
                errors.append(f"process cleanup failed: {error}")
            if reader:
                reader.join(timeout=1)
                stop_capture.set()
                reader.join()
            if process:
                process.stdout.close()
                if process.stdin and not process.stdin.closed:
                    process.stdin.close()
            if errors:
                code = code or 1
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
