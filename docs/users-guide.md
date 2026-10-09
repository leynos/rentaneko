# User Guide

This guide explains how to use the generated Rentaneko project after rendering
it from the template.

## Generated Tooling

Generated projects use Rust 2024, a pinned nightly toolchain, strict lint
settings, and documented starter code. Library projects render `src/lib.rs`.
Application projects render `src/main.rs`, `src/lib.rs`, release automation, and
`[package.metadata.binstall]` metadata for binary installation.

Development builds use Cranelift for debug code generation. On Linux targets,
`.cargo/config.toml` configures clang to link with `mold` so local debug builds
link quickly. Coverage generation uses `lld` instead because LLVM coverage
tools expect LLVM-compatible linker behaviour.

The `test`, `lint`, `typecheck` and `build` targets keep any exported
`RUSTFLAGS` and add `-Zthreads=8`, plus the `mold` linker flag when both the
build host and the compilation target are Linux. `make release` adds neither
flag and passes an exported `RUSTFLAGS` through. To build for another target,
set `CARGO_BUILD_TARGET` (`host-tuple` means the host's own triple); a
`--target` in `CARGO_FLAGS`, `TEST_FLAGS` or `CLIPPY_FLAGS` stops the targets
with an error, because Make cannot read it, on any host. The
[migration note](migrating-to-composed-rustflags.md) lists the changes callers
need to make.

`make coverage` assigns its own `RUSTFLAGS`: it does not keep an exported value
and adds neither standard flag, and it builds on LLVM because
`-Cinstrument-coverage` is LLVM-only.

## Makefile Targets

The generated `Makefile` exposes these public targets:

- `make all` runs formatting checks, linting, and tests.
- `make check-fmt` verifies Rust formatting.
- `make lint` runs rustdoc, Clippy, and Whitaker with warnings denied.
- `make test` runs `cargo nextest run` when cargo-nextest is installed and
  falls back to `cargo test` otherwise. All projects also run doctests.
- `make build` builds the debug target.
- `make release` builds the release target.
- `make coverage` writes `lcov.info` using `cargo llvm-cov` and `lld`.
- `make audit` derives the Rust workspace root with `cargo metadata`, checks
  that packages covered by the repository-owned default RustSec ignores are not
  reachable through normal or build dependencies, fails that preflight before
  auditing if any such package is reachable, and otherwise runs `cargo audit`
  once from the workspace root with the documented default advisory ignores. The
  [developer guide](developers-guide.md) lists the current default ignores.
- `make markdownlint` checks Markdown files.
- `make nixie` validates Mermaid diagrams.

Install `clang`, `lld`, `mold`, `python3`, and `cargo-audit` before running the
full generated workflow locally on Linux.
