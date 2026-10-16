"""Exec one Linux fuzz target with a real address-space memory ceiling."""
import argparse
import os
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--memory-mib", type=int, default=512)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or not 64 <= args.memory_mib <= 4096:
        parser.error("supply a command and memory limit in 64..4096 MiB")
    if sys.platform != "linux":
        parser.error("the fuzz address-space limit currently requires Linux")
    import resource
    maximum = args.memory_mib * 1024 * 1024
    resource.setrlimit(resource.RLIMIT_AS, (maximum, maximum))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    executable = str(Path(command[0]).resolve())
    os.execv(executable, [executable, *command[1:]])


if __name__ == "__main__":
    main()
