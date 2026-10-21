#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Optional system-tool object interoperability, separate from ordinary Cargo tests."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


def run(arguments, **kwargs):
    return subprocess.run(list(map(str, arguments)), check=True, **kwargs)


def windows_environment(arch):
    environment = os.environ.copy()
    candidates = [Path(environment.get(k.upper(), 'C:/Program Files (x86)')) /
                  'Microsoft Visual Studio/Installer/vswhere.exe'
                  for k in ['ProgramFiles(x86)', 'ProgramFiles']]
    vswhere = next((p for p in candidates if p.is_file()), None)
    if vswhere is None:
        raise RuntimeError('native COFF fixture requires Visual Studio and its Windows SDK')
    installation = subprocess.check_output(
        [str(vswhere), '-latest', '-products', '*', '-property', 'installationPath'], text=True).strip()
    setup = Path(installation) / 'VC/Auxiliary/Build/vcvarsall.bat'
    # Only the installed VS path and our fixed architecture enter this command.
    # cmd parses /c itself rather than using the C runtime argv convention.
    # A list would pass nested quotes through list2cmdline as backslash-quotes,
    # which cmd does not unescape. Supply the exact outer/inner quote pairs.
    command = f'cmd.exe /d /s /c "call "{setup}" {"arm64" if arch == "aarch64" else "amd64"} >nul && set"'
    result = subprocess.run(command, check=False, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f'Visual Studio setup failed ({result.returncode}): {result.stderr.strip()}')
    for line in result.stdout.splitlines():
        key, separator, value = line.partition('=')
        if separator and key:
            environment[key.upper()] = value
    return environment


def sources(arch):
    if arch == 'amd64':
        return ('''section .text
global entry
symbol entry, function, 14, default
extern helper
symbol helper, function, 0, default
extern payload
entry: sub rsp,40
call helper
add rsp,40
ret
section .data
global payload_ref
payload_ref: dq payload+4
''', '''section .text
global helper
symbol helper, function, 6, default
helper: mov eax,42
ret
section .data
global payload
payload: dd 7,42
''')
    return ('''.text
.global entry
.extern helper
.extern payload
entry: sub sp,sp,#16
str x30,[sp,#8]
bl helper
ldr x30,[sp,#8]
add sp,sp,#16
ret
.data
.global payload_ref
payload_ref: .quad payload+4
''', '''.text
.global helper
helper: mov w0,#42
ret
.data
.global payload
payload: .word 7,42
''')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--format', choices=['coff'], required=True)
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--readobj', type=Path)
    parser.add_argument('--linker', type=Path, help='optional cross-host lld-link')
    args = parser.parse_args()
    binary = args.binary.resolve()
    arches = ['amd64', 'aarch64']
    if args.native:
        if os.name != 'nt':
            parser.error('COFF native execution requires Windows')
        arches = ['aarch64' if platform.machine().lower() in ['arm64', 'aarch64'] else 'amd64']
    if not args.native and not args.readobj:
        parser.error('non-native validation requires --readobj')
    with tempfile.TemporaryDirectory(prefix='whale-object-oracle-') as directory:
        root = Path(directory)
        for arch in arches:
            objects = []
            for i, source in enumerate(sources(arch)):
                source_path = root / f'{arch}-{i}.s'
                source_path.write_text(source)
                obj = root / f'{arch}-{i}.obj'
                run([binary, 'asm', '--arch', arch, '--format', args.format, source_path, '-o', obj])
                objects.append(obj)
                if args.readobj:
                    run([args.readobj, '--file-headers', '--sections', '--symbols', '--relocations', obj])
            executable = root / f'{arch}.exe'
            if args.native:
                environment = windows_environment(arch)
                driver = root / 'driver.c'
                driver.write_text('extern int entry(void); extern int *payload_ref;\n'
                                  'int main(void) { return entry()==42 && *payload_ref==42 ? 42 : 7; }\n')
                compiler = shutil.which('cl.exe', path=environment.get('PATH'))
                if compiler is None:
                    raise RuntimeError('Visual Studio environment contains no cl.exe')
                run([compiler, '/nologo', driver, *objects, f'/Fe:{executable}'],
                    cwd=root, env=environment)
                result = subprocess.run([str(executable)], timeout=30, check=False)
                if result.returncode != 42:
                    raise AssertionError(f'{arch} COFF linked program returned {result.returncode}')
            elif args.linker:
                run([args.linker, '/entry:entry', '/subsystem:console', '/nodefaultlib',
                     '/machine:' + ('ARM64' if arch == 'aarch64' else 'X64'),
                     '/out:' + str(executable), *objects])
                if args.readobj:
                    run([args.readobj, '--file-headers', executable])
            print(f'{arch}: COFF inspection/link' + ('/native exit 42' if args.native else '') + ' passed')


if __name__ == '__main__':
    main()
