# Contributing to Whale

Thank you for contributing to Whale, a modular low-level toolchain maintained by
Wave Foundation. Code, regression tests, documentation, and reproducible reports
are welcome. This guide covers Whale itself, not the Wave compiler.

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md). Pull request descriptions,
review comments, and patch explanations should be written in English so that
contributors can follow the same discussion.

## 1. Development setup

Use Git, a stable Rust toolchain installed through rustup, and the native linker
required by that Rust toolchain. The minimum supported Rust version is **1.86.0**;
check [Cargo.toml](Cargo.toml) and [the CI guide](docs/ci.md) before changing it.
Python **3.13** is used for the CI helper tests and CLI smoke scripts.

A normal Whale build does not require the Wave compiler, an LLVM development
SDK, or an external assembler. The required CLI smoke checks inspect Whale's
output directly. Cross-host execution and optional comparison tools have their
own prerequisites, documented in [docs/ci.md](docs/ci.md).

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

The default CLI includes assembly and object commands. The experimental
`whale ir lower` command requires `--features socket-cli`; all-features checks
exercise that path as well. A host supported by CI is not necessarily a code
generation target: the RISC-V job runs Whale under QEMU but still checks AMD64
output. See the [capability table](README.md#capability-status) for current scope.

## 2. Choose the right component

| Path | Contribution area |
| --- | --- |
| `assembler/` | Assembly tokens, parsing, AMD64 encoding, symbols, and relocations |
| `object/` | Object representations, sections, symbols, and ELF serialization |
| `ir/` | Shared IR, builders, verification, and optional socket AST lowering |
| `linker/` | Linker groundwork; do not describe the current CLI stub as a working linker |
| `src/` | Whale CLI, argument handling, diagnostics, and component integration |
| `tests/` | CLI and repository-level integration regressions |
| `tools/` | CI helpers and standalone smoke checks |
| `docs/`, `.github/` | User/developer guides, contribution templates, and CI configuration |

[MAINTAINERS](MAINTAINERS) maps these paths to review contacts. It is a contact
list, not an automated assignment mechanism. Use the matching contact when
asking for a review or CC'ing an email patch.

## 3. Fork and open a focused pull request

Fork `wavefnd/Whale`, then start a topic branch from the latest upstream `master`.
In this example, replace `YOUR_USERNAME` with your GitHub login:

```sh
git clone https://github.com/YOUR_USERNAME/Whale.git
cd Whale
git remote add upstream https://github.com/wavefnd/Whale.git
git fetch upstream
git switch -c docs/my-change upstream/master
```

In an existing checkout, inspect `git remote -v` first. Add `upstream` only if it
is absent, and verify that it points to `wavefnd/Whale`. Keep uncommitted work safe
before switching branches or rebasing.

Make one logical change, stage its intended files, inspect the staged diff, and
commit with a sign-off. Push the topic branch to your fork, not upstream master:

```sh
git diff --check
git diff --cached --check
git diff --cached
git commit -s -m "docs: clarify contributor workflow"
git push -u origin HEAD
```

Open the PR against **`wavefnd/Whale:master`** and fill in the
[PR template](.github/PULL_REQUEST_TEMPLATE.md). Link an existing report or design
conversation when relevant; a standalone documentation correction does not need
a new issue just to submit a PR.

## 4. Sign-offs and email patches

All contributed commits must include a Developer Certificate of Origin (DCO)
`Signed-off-by` trailer. Read the [DCO](https://developercertificate.org/) and use
your own name and email address when signing off work you have the right to
submit:

```text
Signed-off-by: Your Name <email@example.com>
```

`git commit -s` adds this trailer. It is not a GPG/SSH commit signature and does
not transfer copyright. Preserve other authors' attribution when revising or
carrying their work.

Email patches can be sent to `patchs@wave-lang.dev`, with Whale identified in the
subject. From a signed-off topic branch, prepare a patch series against upstream:

```sh
git format-patch --subject-prefix="Whale PATCH" upstream/master
```

Send the generated patches to that address using your mail client or configured
`git send-email`, and CC the relevant contact in [MAINTAINERS](MAINTAINERS).
Include the base commit and verification results. Reviewers use the same local
checks below; this guide does not depend on a repository-specific patch script.

## 5. Local verification

Use the committed `Cargo.lock`. Run these checks from the repository root for
Rust changes; they cover both default and optional feature paths:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test --workspace --all-features --release --locked
```

For CLI or tooling changes, also run the helper tests and smoke checks against
each separately built configuration:

```sh
python3 -m unittest discover -s tools -p 'test_*.py' -v
cargo build --workspace --locked
python3 tools/ci_smoke.py --binary target/debug/whale --socket disabled
cargo build --workspace --all-features --locked
python3 tools/ci_smoke.py --binary target/debug/whale --socket enabled
```

On Windows, use `target/debug/whale.exe`. For public API documentation, set
`RUSTDOCFLAGS=-D warnings` in your shell and run:

```sh
cargo doc --workspace --all-features --no-deps --locked
```

The [CI guide](docs/ci.md) is the detailed reference for MSRV, platform matrices,
coverage, workflow validation, and retained failure evidence. Do not claim that
unavailable platforms or checks passed. Report the exact commands, feature
configuration, host, and results; identify skipped or blocked checks explicitly.

For documentation-only changes, check links, fenced examples, paths, and template
syntax. Explain why runtime tests are not applicable instead of claiming that
code was exercised. CI remains responsible for its configured gates.

## 6. Regression tests and compatibility

Keep tests beside the affected crate or in the relevant integration suite. For a
bug fix, first demonstrate the failing case on the base revision when feasible,
then verify the corrected behavior. Distinguish executed reproductions from
source-derived predictions or independently written models.

Assembler and object changes should assert emitted bytes, symbols, relocations,
or parsed object structure where applicable, not just successful assembly. IR
changes should test both invalid input with useful context and nearby valid
cases. CLI changes should check exit status, diagnostics, and output-file
behavior. Prefer fixtures that run without an external assembler or network.

Keep code generation targets, object formats, host support, public APIs, and
feature gates explicit. Do not silently expand an issue into a new instruction
set, linker implementation, or IR redesign. Update the matching page under
[docs/cli](docs/cli) when CLI behavior changes.

## 7. Formatting and review updates

Use Rust naming conventions and opening braces on the same line (K&R style).
Avoid trailing whitespace. `cargo fmt --all -- --check` is the workspace check;
it does not edit files. `cargo fmt --all` does edit the workspace, so inspect
`git diff --stat` and the actual diff before committing its results.

Keep unrelated formatting and refactoring out of a functional PR. When a
formatting failure also occurs on an unchanged upstream revision, report that
baseline problem to the maintainer instead of mixing a repository-wide cleanup
into the fix or weakening CI.

Push follow-up commits to the same topic branch and summarize which review
requests they address. Rebase onto the latest `upstream/master` when necessary,
resolving conflicts without dropping either upstream fixes or your regressions.
After rewriting an already published branch, review the final diff, rerun the
relevant checks, and use `git push --force-with-lease origin HEAD` rather than
an unconditional force push. Coordinate with anyone else using the branch.

## 8. Reports, proposals, and communication

Search existing issues and PRs before reporting a duplicate or beginning a large
change. Bug reports should include the Whale commit, host and output target,
feature flags, minimal input, exact command, expected result, and actual evidence.
For an unexecuted finding, state that clearly and identify the source path and
failure scenario. Performance reports need measured values, units, repetitions,
build profiles, and comparable revisions.

Use the templates on [Whale's tracker](https://github.com/wavefnd/Whale/issues/new/choose)
for reports and proposals. Keep implementation discussions attached to the
relevant issue or PR. The [Wave community](https://wave-lang.dev/community) is
also available for general discussion across the projects.

## 9. Licensing and repository policies

Unless a file says otherwise, contributions to Whale are covered by the
[Mozilla Public License 2.0](LICENSE). Preserve existing copyright and license
notices, including third-party attributions.

See [COPYRIGHT](COPYRIGHT) for attribution, [NOTICE](NOTICE) for project notices,
and [ai.txt](ai.txt) for the repository's AI/ML usage policy and permission contact.
A contribution or review is not permission to reuse repository material for
model training. Authors remain responsible for the correctness, provenance, and
verification of everything they submit.
