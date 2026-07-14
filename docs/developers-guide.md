# Developer Guide

This guide explains the contributor workflow for the generated Rentaneko
project.

## Spelling policy

Run `make spelling` to enforce en-GB-oxendict prose spelling. The target checks
`typos.toml` for drift, runs the consumer phrase scanner, then runs the pinned
Typos release over tracked Markdown. The generated configuration combines the
shared estate dictionary with the narrow `typos.local.toml` overlay; never edit
generated entries by hand.

The configuration builder is pinned to commit
`d6da92f02240a79a945c835f69bdd08a888da1d0`. Regenerate the configuration with:

```sh
TYPOS_CONFIG_BUILDER_COMMIT=d6da92f02240a79a945c835f69bdd08a888da1d0
uvx --python 3.14 \
  --from "git+https://github.com/leynos/typos-config-builder.git@${TYPOS_CONFIG_BUILDER_COMMIT}" \
  typos-config-builder
```

Use the same command with `--check` in quality gates. The builder refreshes the
untracked `.typos-oxendict-base.toml` only when the authority is newer and can
reuse a valid cache when the authority is unavailable.

Typos splits hyphenated phrases into separate words. The consumer-owned
`scripts/typos_rollout_check.py` therefore checks phrase corrections without
duplicating the builder's validation, cache, merge or rendering behaviour.

## Local Workflow

Use `make all` as the public entrypoint for formatting, linting, and tests.
`make lint` runs rustdoc, Clippy, and Whitaker. `make test` prefers
`cargo nextest run` and falls back to `cargo test` when cargo-nextest is not
available. `make audit` derives the Rust workspace root with `cargo metadata`,
logs workspace member manifests, and runs `cargo audit` once from the workspace
root. `make coverage` uses `cargo llvm-cov` with `lld`.

GitHub Actions Act validation lives in `.github/workflows/act-validation.yml`.
The main `.github/workflows/ci.yml` workflow deliberately does not run
`make test WITH_ACT=1`; the separate Act workflow runs those slower
container-backed checks in parallel.

## Prototype API Boundaries

The prototype API is constructor-shaped. `Simulator::start` is the single
lifecycle authority for the Bun child process, temporary configuration, base
URI, and seeded installation ID. Higher-level helpers must compose that handle
rather than spawning or tearing down another simulator process.

`OctocrabFixture` is a thin convenience wrapper around `Simulator` and a real
`octocrab::Octocrab` client. It exists to keep Podbot's first integration test
small, but its long-term stability remains a design question until the Podbot
call site proves that the wrapper earns a public API surface.

Rentaneko owns simulator lifecycle and `octocrab` construction only. It must
not assert Podbot's token-file permissions, temporary-file cleanup, or
atomic-rename behaviour. Podbot owns those filesystem contracts and should test
them directly.

## Tooling

Development builds use Cranelift for debug code generation. On Linux targets,
`.cargo/config.toml` configures clang to link with `mold` so debug builds link
quickly. Coverage generation uses `lld` because LLVM coverage tooling expects
LLVM-compatible linker behaviour.

Install `clang`, `lld`, `mold`, `python3`, and `cargo-audit` before running the
full generated workflow locally on Linux.

### Security audit ignores

Security audit jobs may set `CARGO_AUDIT_IGNORES` for narrowly scoped RustSec
advisories that affect unused or tooling-only dependency paths. Keep each
ignore tied to a documented runtime impact analysis, and remove it when the
affected dependency leaves the graph or the project starts using the advised
runtime path.
