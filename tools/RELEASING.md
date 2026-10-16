# Native release packages

Python 3.13+ and the standard library are sufficient for the release tools.
The six supported package hosts are the Linux, macOS and Windows x64/ARM64
hosts already exercised in native CI. RISC-V host emulation does not qualify
as native release validation. These packages generate AMD64 ELF objects,
independently of the host CPU/OS; they do not add native executable linking.

Build a native binary, then package and validate it (replace the full commit
with the actual source SHA; the version must equal Cargo.toml):

```sh
cargo build --release --workspace --all-features --locked
python tools/release.py package --binary target/release/whale --directory dist --version 0.1.0 --source FULL_40_DIGIT_COMMIT --target x86_64-unknown-linux-gnu --socket-cli
python tools/release.py validate --directory dist --version 0.1.0 --source FULL_40_DIGIT_COMMIT --target x86_64-unknown-linux-gnu
```

Use the native `.exe` on Windows. Omit `--socket-cli` for a default-feature
binary. The manifest's feature declaration is checked against the extracted
binary. Archives include exactly the binary, LICENSE, README.md and
manifest.json under a versioned root directory. The manifest records version,
host target, source SHA, feature availability and binary SHA256. Packaging
checks executable header architecture and actual CLI version; missing binaries,
version/target mismatches and existing output names fail. Identical inputs give
identical archive bytes with normalized metadata, entry order and timestamps.
The caller must provide a binary built from the stated commit; the workflow
enforces that provenance through exact-SHA native builds.

Validation checks members and metadata before extracting into a temporary
directory. It rejects paths, links, duplicates, missing files, non-executable
binaries, oversized contents and checksum mismatches. With an empty PATH and
outside the checkout, the extracted binary runs help/version, creates an object
with independently specified instruction bytes, rejects malformed assembly
without overwriting output, and confirms socket-cli availability.

After collecting the exact six archives in one directory:

```sh
python tools/release.py checksums --directory dist --version 0.1.0 --source FULL_40_DIGIT_COMMIT
python tools/release.py verify --directory dist --version 0.1.0 --source FULL_40_DIGIT_COMMIT
```

For local single-host checks, supply one `--target`; repeat it for an explicit
subset. Empty/duplicated targets and unexpected, missing or stale-version files
fail. SHA256SUMS is sorted and checked byte-for-byte, so duplicate manifest
entries and tampering fail. Files are published atomically without replacement.

The **Manual release** workflow runs only on workflow_dispatch. It requires a
full source SHA, matching Cargo version and an existing `vVERSION` tag pointing
to that exact commit. It checks source, builds and executes all six native
packages, then verifies the exact aggregate archive set and checksums. Every
producer and validation job must succeed. Only the final job has contents:write.
`publish_draft` defaults to false; when selected it creates a new draft and
refuses to overwrite an existing release. Implementing this tooling does not
dispatch the workflow, create a tag or publish a release.

Focused local regressions (synthetic packages require no credentials or external
assembler):

```sh
python -m unittest discover -s tools -p test_release.py -v
python -m unittest discover -s tools -p test_ci.py -v
```

The CI runner is non-interactive and retains process-tree ownership until output
cleanup finishes. POSIX uses a private process group; Windows uses a gated
launcher inside a kill-on-close Job before the requested command can spawn.
See [Windows Job ownership](https://learn.microsoft.com/windows/win32/procthread/job-objects).
It bounds output draining, cleans descendants even when the direct child exits
first, and joins the nonblocking reader before closing its log. Cleanup failures
remain diagnostics; captured output and command exit status are retained.
