# Developer Guide

This guide explains the contributor workflow for the generated Rentaneko
project.

## Spelling policy

Run the spelling gate with:

```bash
make spelling
```

The tracked `typos.toml` is regenerated on every run from the live shared
dictionary and the repository-specific `typos.local.toml` overlay. Never edit
generated entries by hand; add only narrow repository terminology to the
overlay. Because the dictionary is live, `typos.toml` must never be drift
checked in continuous integration.

The pinned `typos-config-builder` release refreshes the estate dictionary into
an untracked local cache only when the authoritative copy is newer. A valid
cache remains usable when the network is unavailable. The gate also enforces
the shared phrase corrections that Typos cannot match as whole phrases, because
Typos splits hyphenated phrases into separate words.

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

### Compatibility checkpoint

The 1.1.1 compatibility checkpoint requires Bun `1.3.11` or newer and the
checked-in `simulacat-core` dependency. Run the ignored proof with
`cargo nextest run --run-ignored all -E 'test(octocrab_compatibility)'` to
recheck the opt-in checkpoint.

The Rust test uses the development-only `uselesskey` crate to generate a fresh
RSA-2048+ RS256 signing key in memory at runtime. It converts the transient
PKCS#8 representation directly into `jsonwebtoken::EncodingKey` for real
Octocrab App JWT signing. Private-key material must never be committed,
persisted, printed, or logged; production and public library APIs must not
depend on `uselesskey`.

The Bun runner installs `SIGINT` and `SIGTERM` handlers before listening. The
Rust-side teardown path is separate: it sends process-group `SIGTERM`, waits
for a bounded interval, then force-kills and reaps the child if it is still
alive. `Drop` remains synchronous and best-effort last-resort cleanup.
Deterministic checkpoint tests cover readiness and stdout/stderr capture
cancellation, shutdown, and Unix process-group cleanup. Only the managed
`Simulator` lifecycle, stdin ownership, and related managed cancellation remain
deferred to roadmap task 1.3.2.

Delete this subsection once roadmap tasks 1.3.1 and 1.3.2 replace the throwaway
runner with the owned Bun process and Rust process handle.

## Tooling

Development builds use Cranelift for debug code generation. On Linux targets,
`.cargo/config.toml` configures clang to link with `mold` so debug builds link
quickly. Coverage generation uses `lld` because LLVM coverage tooling expects
LLVM-compatible linker behaviour.

Install `clang`, `lld`, `mold`, `python3`, and `cargo-audit` before running the
full generated workflow locally on Linux.

### Security audit ignores

`make audit` includes the repo-owned default ignores `RUSTSEC-2023-0071` and
`RUSTSEC-2024-0370`. `RUSTSEC-2023-0071` currently comes from `rsa` through the
test-only `octocrab` / `jsonwebtoken` App JWT path; the advisory has no fixed
upgrade and the AWS-LC JWT backend was evaluated but did not link in this
environment. `RUSTSEC-2024-0370` comes from `proc-macro-error` through the
`rstest-bdd-macros` test harness.

The `make audit` preflight should reject those repo-owned ignores when a
package is reachable through normal or build dependencies. That keeps the
default ignores confined to test-only or tooling-only paths, rather than
masking runtime code that ships in the normal dependency graph.

Security audit jobs may add `CARGO_AUDIT_IGNORES` for further narrowly scoped
RustSec advisories that affect unused or tooling-only dependency paths. Keep
each ignore tied to a documented runtime impact analysis, and remove it when
the affected dependency leaves the graph or the project starts using the
advised runtime path.
