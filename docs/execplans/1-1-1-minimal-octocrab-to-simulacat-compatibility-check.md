# Add the minimal Octocrab-to-Simulacat compatibility checkpoint (1.1.1)

This ExecPlan (execution plan) is a living document. The sections `Constraints`,
`Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`, `Decision Log`,
and `Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: COMPLETED

## Purpose / big picture

Rentaneko's entire walking skeleton rests on one unproven assumption: that a
real `octocrab` 0.51.0 GitHub App client can call Simulacat Core's existing
installation-token route and read back the fixed token `FAKE_GITHUB_TOKEN`
without Rentaneko patching the response. Roadmap task 1.1.1 is the *fail-fast
compatibility checkpoint* that proves or disproves this before any process
lifecycle machinery is built. See [rentaneko-design.md](../rentaneko-design.md)
§5 and [ADR 001](../adr-001-use-simulacat-core-for-octocrab-spike.md).

The single irreducible fact only real Rust can prove (beyond what a one-line
`curl` already shows) is this: `octocrab` 0.51.0 constructs an RS256 JSON Web
Token that Simulacat Core's permissive authentication *accepts* and, with
`Content-Type: application/json`, receives `FAKE_GITHUB_TOKEN` through the real
`InstallationToken` path. The Rust harness also proves that installation `9999`
reaches Octocrab's typed `404 Not Found` GitHub error. This is exactly the
boundary Podbot depends on (ADR 001 decision drivers).

After this change, a developer can run the single, opt-in checkpoint test that:

1. starts a throwaway Simulacat Core process seeded with installation `2000`
   for App `1`, bound to `127.0.0.1` on an operating-system-assigned port;
2. builds a real App-authenticated `octocrab::Octocrab` whose base URI points at
   that process;
3. calls `installation_token_with_buffer` for installation `2000`; and
4. observes a typed `404 Not Found` Octocrab error for the unseeded installation
   `9999`, while the seeded installation `2000` returns `FAKE_GITHUB_TOKEN`.

The observable result is:
`cargo nextest run --run-ignored all -E 'test(octocrab_compatibility)'` passes
both scenarios against the pinned simulator dependency. The deliverable is
deliberately a *throwaway* harness, not the managed `Simulator` handle; that
handle is later roadmap work (1.3.2), and the throwaway artefacts carry an
explicit supersede-and-delete clause (see Constraints).

This plan closes out task 1.1.2's upstream outcome: no Simulacat Core response
change is required. The earlier deserialization symptom came from a `400`
request-schema rejection caused by the missing JSON content-type header, not
from the installation-token payload. The harness records that client
configuration requirement without rewriting the simulator response.

## Constraints

Hard invariants that must hold throughout implementation. Violation requires
escalation, not a workaround.

- Do not modify the public library surface. Task 1.1.1 is test-only.
  `src/lib.rs` keeps its current `greet` stub; no new public items, no new
  runtime dependencies in the `[dependencies]` table. All new crates go in
  `[dev-dependencies]`.
- Do not build the managed runner, the `Simulator` handle, the versioned
  configuration schema, the full v1 NDJSON event parser, or the
  `RentanekoError` enum. Those are roadmap tasks 1.2.1, 1.2.2, 1.3.1, 1.3.2,
  and 1.4.x. This checkpoint may read only the readiness line it needs and must
  clearly mark its harness as throwaway.
- Supersede-and-delete clause: every artefact added here is disposable. When
  roadmap 1.3.1 lands the real config-reading runner and 1.3.2 lands the
  `Simulator` handle, `tests/checkpoint_support/checkpoint_runner.ts`, the
  throwaway guard, and the checkpoint's bespoke process handling must be
  deleted or folded into the managed runner — not left to coexist as a second
  Bun entrypoint. Record this trigger in `docs/developers-guide.md` so it is
  not forgotten.
- Do not patch, rewrite, or post-process the simulator's token response in Rust.
  The token must arrive unmodified through real `octocrab`. If it does not,
  stop and record an upstream Simulacat Core task (this is the §5 / §12
  contract).
- Keep `octocrab` pinned to the 0.51.x line to match Podbot's incubator
  dependency, and ensure the lockfile resolves to exactly `0.51.0` (Podbot's
  pin; rentaneko-design.md §10 requires the same minor). The checkpoint uses
  App ID `1` and installation ID `2000` exactly (rentaneko-design.md §6).
- Keep `make check-fmt`, `make lint`, and `make test` green for a contributor
  who does **not** have Bun or Simulacat Core installed. The Bun-dependent
  checkpoint must therefore be `#[ignore]`-gated, because `make test` runs with
  `--all-features` (Makefile `TEST_FLAGS = --all-targets --all-features`) and a
  Cargo feature gate would not exclude it. (Note: AGENTS.md prose describing
  `make test` as plain `cargo test --workspace` is stale; the Makefile is the
  real gate.)
- Honour the lint policy in `Cargo.toml` and `clippy.toml`. Denied lints include
  `unwrap_used`, `expect_used`, `print_stdout`, `print_stderr`,
  `indexing_slicing`, `cast_possible_truncation`. `clippy.toml` also enforces
  `cognitive-complexity-threshold = 9`, `too-many-lines-threshold = 70`,
  `too-many-arguments-threshold = 4`, and `excessive-nesting-threshold = 4`,
  all denied via `make lint`. Therefore every helper (spawn, readiness read,
  parse, octocrab call) must be a separate sub-70-line, sub-complexity-9
  function from the outset; do not wait for a 400-line file before splitting.
  Test bodies may use `.expect(...)` (`allow-expect-in-tests = true`); shared
  helpers outside a `#[test]`/`#[rstest]` function must return `Result` and use
  `?`.
- Documentation must follow
  [documentation-style-guide.md](../documentation-style-guide.md):
  en-GB-oxendict spelling, prose wrapped at 80 columns, code blocks at 120,
  attributed fenced code blocks.

## Tolerances (exception triggers)

Stop and escalate when any threshold is breached:

- Scope: if delivering the checkpoint requires changing more than 12 files or
  more than ~400 net lines of Rust, stop and escalate.
- Lint: if any single function exceeds the `clippy.toml` thresholds
  (70 lines, cognitive complexity 9, 4 arguments, nesting 4) and cannot be
  decomposed cleanly, stop and escalate rather than adding a scoped allow.
- Interface: if any change to `src/lib.rs`'s public API appears necessary, stop
  and escalate — 1.1.1 is test-only.
- Dependencies: the expected new `[dev-dependencies]` are `octocrab`, `tokio`,
  `jsonwebtoken`, `nix`, `chrono`, `secrecy`, `serde_json`, `rstest`,
  `rstest-bdd`, `rstest-bdd-macros`, `googletest`, and `pretty_assertions`, plus
  `wiremock` only if the diagnostic triage path (Stage A.4) is needed. If any
  *additional* direct crate is required, or if any cannot resolve at the
  versions named in `Interfaces and dependencies`, stop and escalate.
- Readiness timeout: the harness must bound the readiness wait. If a bounded
  wait (default 30 s) elapses with no readiness line, stop the child, surface
  captured stderr, and fail with a clear message. A harness that can block
  indefinitely is a defect, not an acceptable throwaway.
- Compatibility: if `installation_token_with_buffer` returns anything other than
  `FAKE_GITHUB_TOKEN`, or the route returns a non-2xx status, or `octocrab`
  fails to deserialize the payload, stop. Do not patch the response. Capture
  the exact failure and proceed to the 1.1.2 upstream-task recommendation.
- Dependency availability: if `simulacat-core` cannot be made importable from a
  Bun entrypoint after trying the fallbacks in `Plan of work` Stage A, stop and
  escalate — the checkpoint is blocked on an upstream packaging question.
- Iterations: if the checkpoint still fails after 3 focused debugging attempts
  against a confirmed-running simulator, stop and escalate. Use the ADR's
  in-process `wiremock` stub (Stage A.4) to localize whether the fault is
  `octocrab` parsing or the Simulacat Core route before escalating.
- Ambiguity: if `rstest-bdd` will not forward `#[ignore]` to its generated test
  and the documented fallback also fails, stop and present options.

## Risks

- Risk: `simulacat-core` is not published to a public npm registry under that
  name (research found the npm name unpublished; the module is mirrored at
  `github.com/leynos/simulacat-core`, version `0.6.4`). Severity: high.
  Likelihood: high. Mitigation: Stage A resolves the dependency before any Rust
  work, trying, in order, (1) `bun add simulacat-core` against whatever
  registry the environment provides, (2) a git dependency pinned to a **commit
  SHA** (`"simulacat-core": "github:leynos/simulacat-core#<sha>"`), with a
  build step only if the package ships sources rather than `dist/`, and (3)
  escalation if neither yields an importable `simulation` export. Prefer (1)
  over (2): a git dependency that builds on install is slower and less
  reproducible, and a floating branch ref would drift silently before the 1.5.1
  tripwire runs — so a SHA pin is mandatory if (2) is used. This is a go/no-go
  gate.
- Risk: the `@simulacrum/foundation-simulator` `listen` contract differs from
  the verified 0.6.1 shape if the lockfile resolves to 0.7.x/0.8.x. Severity:
  medium. Likelihood: low. Mitigation: the Bun entrypoint reads the bound port
  defensively from `handle.server.address().port` with `handle.port` as a
  fallback, and Stage A records the resolved `foundation-simulator` version,
  install wall-time, and `node_modules` size.
- Risk: `rstest-bdd` 0.5.0 does not forward `#[ignore]` to the test it
  generates, so the behavioural scenario would run (and fail) in Bun-less CI.
  Severity: medium. Likelihood: medium. Mitigation: Stage B verifies attribute
  forwarding with a trivial scenario. Fallback: keep the end-to-end assertions
  as plain `#[rstest] #[ignore]` async tests and treat the `.feature` file as
  living documentation whose steps delegate to the same shared harness. The
  two-scenario feature (happy path plus the unknown-installation negative path)
  keeps the BDD layer earning its place rather than wrapping a single assertion.
- Risk: process and resource leaks at 03:00. `tokio::process::Child::kill` only
  reaps the direct child; if Bun spawns helpers they may survive. A panic
  between spawn and guard construction orphans the child. A hung import or bind
  with no timeout blocks forever. Severity: medium. Likelihood: medium.
  Mitigation: construct the RAII guard atomically with the spawn (before any
  further `?`); bound the readiness wait with `tokio::time::timeout`; terminate
  the read loop on readiness, child-stdout EOF, or timeout; on Linux, kill the
  child's process group rather than the bare PID. ADR 001 already accepts a
  later stray-runner reaper as the backstop; this plan restates that as a known
  limitation rather than inheriting it silently.
- Risk: Bun is absent in CI or on a contributor machine, so the checkpoint
  cannot run. Severity: low (by design). Likelihood: high. Mitigation: the
  checkpoint is opt-in (`#[ignore]`). The CI safety net for the new code is
  `make lint` (`--all-targets --all-features`), which compiles and Clippy-lints
  the checkpoint even while it is ignored; the Bun-free port-extractor `rstest`
  test is executed by CI's coverage action. The live checkpoint is not executed
  in CI until the drift tripwire, roadmap 1.5.1; the rot window between 1.1.1
  and 1.5.1 is bounded by the SHA-pinned dependency and a tracking note (Stage
  D).
- Risk: the `octocrab` default client must be constructed inside an active Tokio
  runtime; building it outside one panics or errors. Severity: low. Likelihood:
  medium. Mitigation: all `octocrab` construction and calls run inside the
  scenario's current-thread Tokio runtime
  (`#[tokio::test(flavor = "current_thread")]`).
- Risk: persisted or logged RSA private-key material, though test material,
  trips secret scanners or the `coderabbit review --agent` gate. Severity: low.
  Likelihood: medium. Mitigation: generate an RSA-2048+ RS256 key at runtime
  with `uselesskey`, feed it directly into `jsonwebtoken::EncodingKey`, and
  keep it out of stdout, stderr, and on-disk artefacts.

## Progress

- [x] 2026-07-19: review pass cleaned the stale lifecycle wording, removed the
  committed-key recovery branch, and clarified that readiness-schema means the
  Bun parser contract (`version == 1` and `port` in `1..=65535`), with tests
  enforcing it.
- [x] 2026-06-24: implementation approved and started on branch
  `1-1-1-minimal-octocrab-to-simulacat-compatibility-check`.
- [x] 2026-06-24: Stage A go/no-go passed. `simulacat-core` is installed from
  GitHub at SHA `79b51f314238d7d602b73fede7bd27b10f206b6e`; a fresh
  `bun install` plus the throwaway source-import runner served installation
  `2000` with `FAKE_GITHUB_TOKEN` and rejected installation `9999`.
- [x] Stage A: resolve `simulacat-core` Bun dependency (SHA-pinned if git),
  decide the test-key strategy, and confirm a throwaway server serves the token
  route (go/no-go).
- [x] 2026-06-24: Stage B parser Red-Green cycle completed. The focused
  `cargo nextest run -E 'test(parse_listening_port)'` red run failed because
  `parse_listening_port` did not exist; after adding the pure helper, the six
  parameterized parser cases passed.
- [x] 2026-06-24: Stage B BDD red state captured. The default
  `cargo nextest run --all-targets --all-features` run skipped the two ignored
  BDD scenarios, proving `#[ignore]` forwarding works; the opt-in
  `cargo nextest run --run-ignored all -E 'test(octocrab_compatibility)'` run
  failed because the checkpoint was not wired yet.
- [x] Stage B: add red tests — the `.feature` scenarios (happy and negative) and
  the port-extractor `rstest` cases — and observe them fail for the expected
  reasons; verify `#[ignore]` forwarding.
- [x] 2026-06-24: Stage C reached the compatibility stop condition. The
  throwaway Rust harness starts Simulacat Core and the negative scenario gets
  an Octocrab error, but the required happy-path
  `installation_token_with_buffer` call fails by treating the token payload as
  a GitHub error response.
- [x] Stage C: implement the Bun entrypoint, test key, harness (with timeout,
  stderr capture, atomic guard, EOF/error handling), and the `octocrab` calls
  until the planned compatibility boundary is exercised.
- [x] 2026-06-24: deterministic default gates are green after the compatibility
  stop. Evidence: `/tmp/fmt-rentaneko-1-1-1-no-expect.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-no-expect.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-no-expect.out`,
  `/tmp/lint-rentaneko-1-1-1-no-expect.out`, and
  `/tmp/test-rentaneko-1-1-1-default-after-blocker.out`.
- [x] 2026-06-24: Stage C CodeRabbit findings addressed. The harness now
  preserves captured stderr for all startup failures, owns the port parser, sets
  `kill_on_drop(true)` on the Bun child, stores the BDD Octocrab client in
  scenario state, and preserves typed Octocrab errors in `token_result`.
  Evidence: `/tmp/coderabbit-rentaneko-1-1-1-stage-c-blocked.out`.
- [x] 2026-06-24: deterministic default gates are green after the Stage C
  CodeRabbit fixes. Evidence: `/tmp/fmt-rentaneko-1-1-1-coderabbit-fixes-2.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-coderabbit-fixes-2.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-coderabbit-fixes-2.out`,
  `/tmp/lint-rentaneko-1-1-1-coderabbit-fixes-2.out`, and
  `/tmp/test-rentaneko-1-1-1-coderabbit-fixes-2.out`.
- [x] 2026-06-24: Stage C follow-up CodeRabbit findings addressed. The
  throwaway guard is now constructed before waiting for readiness so process
  group cleanup covers early startup failures, the public harness function
  documents its errors, the BDD feature uses a shared `Background`, the
  installation-token step is parameterized, and the token placeholder is typed
  for quote stripping. Evidence:
  `/tmp/coderabbit-rentaneko-1-1-1-stage-c-followup.out`.
- [x] 2026-06-24: deterministic default gates are green after the Stage C
  follow-up CodeRabbit fixes. Evidence:
  `/tmp/fmt-rentaneko-1-1-1-coderabbit-followup-fixes.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-coderabbit-followup-fixes.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-coderabbit-followup-fixes.out`,
  `/tmp/lint-rentaneko-1-1-1-coderabbit-followup-fixes.out`, and
  `/tmp/test-rentaneko-1-1-1-coderabbit-followup-fixes.out`.
- [x] 2026-06-24: Stage C second follow-up CodeRabbit findings triaged. Valid
  findings were applied by removing the redundant parser wrapper, flattening
  Octocrab error propagation, and documenting the runner host contract. The
  `expect()` suggestions were skipped because `make lint` denies `expect_used`;
  the `Default::default()` fixture suggestion was skipped because the
  `rstest-bdd` fixture macro previously produced `unused_braces` under
  `RUSTFLAGS="-D warnings"`; the underscore-parameter suggestion was reverted
  because the live checkpoint proved `rstest-bdd` requires the parameter name
  `checkpoint_state` for fixture lookup. Evidence:
  `/tmp/coderabbit-rentaneko-1-1-1-stage-c-followup-2.out`,
  `/tmp/test-live-stage-c-after-bdd-fixes-rentaneko-1-1-1.out`, and
  `/tmp/test-live-stage-c-after-rstest-bdd-fixture-restore-rentaneko-1-1-1.out`.
- [x] 2026-06-24: deterministic default gates are green after the second
  follow-up triage. Evidence:
  `/tmp/fmt-rentaneko-1-1-1-after-fixture-restore.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-after-fixture-restore.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-after-fixture-restore.out`,
  `/tmp/lint-rentaneko-1-1-1-after-fixture-restore.out`, and
  `/tmp/test-rentaneko-1-1-1-after-fixture-restore.out`.
- [x] 2026-06-24: Stage C third CodeRabbit findings addressed after one
  randomized 52-minute rate-limit backoff. BDD setup/assertion steps now return
  `Result<(), BoxError>` where they can propagate harness failures, token
  request execution returns a setup `Result` while still storing Octocrab token
  errors for the negative scenario, and process-group cleanup uses `nix` signal
  APIs instead of invoking the external `kill` binary. Evidence:
  `/tmp/coderabbit-rentaneko-1-1-1-stage-c-followup-3.out`,
  `/tmp/coderabbit-rentaneko-1-1-1-stage-c-followup-3-retry.out`.
- [x] 2026-06-24: deterministic default gates are green after the Result/nix
  changes. Evidence: `/tmp/fmt-rentaneko-1-1-1-result-steps-nix-4.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-result-steps-nix-4.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-result-steps-nix-4.out`,
  `/tmp/lint-rentaneko-1-1-1-result-steps-nix-4.out`, and
  `/tmp/test-rentaneko-1-1-1-result-steps-nix-4.out`.
- [x] 2026-06-24: the ignored live checkpoint reached an apparent compatibility
  stop after the Result/nix changes. This historical result was superseded on
  2026-07-18: the missing JSON content-type header caused the schema `400`, not
  an installation-token payload incompatibility. Evidence:
  `/tmp/test-live-stage-c-after-result-steps-nix-rentaneko-1-1-1.out`.
- [x] 2026-06-24: Stage C fourth CodeRabbit findings addressed or adjudicated.
  The harness now documents `initialized` using the project spelling, spawn
  errors identify the Bun runner path, and `nix::kill` results are consumed
  explicitly. The suggested `#[expect(unused_variables)]` replacement for
  `drop(checkpoint_state)` was attempted and reverted because `make lint`
  reports an unfulfilled lint expectation while the live checkpoint still needs
  the exact `checkpoint_state` parameter name. Evidence:
  `/tmp/coderabbit-rentaneko-1-1-1-stage-c-followup-4.out`,
  `/tmp/lint-rentaneko-1-1-1-coderabbit-followup-4-fixes.out`.
- [x] 2026-06-24: deterministic default gates are green after the fourth
  CodeRabbit pass restore. Evidence:
  `/tmp/fmt-rentaneko-1-1-1-coderabbit-followup-4-restore.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-coderabbit-followup-4-restore.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-coderabbit-followup-4-restore.out`,
  `/tmp/lint-rentaneko-1-1-1-coderabbit-followup-4-restore.out`, and
  `/tmp/test-rentaneko-1-1-1-coderabbit-followup-4-restore.out`.
- [x] 2026-06-24: the ignored live checkpoint still showed the apparent
  compatibility stop after the fourth CodeRabbit restore. The 2026-07-18 trace
  supersedes this result with the missing content-type root cause. Evidence:
  `/tmp/test-live-stage-c-after-followup-4-restore-rentaneko-1-1-1.out`.
- [x] 2026-06-24: Stage C CodeRabbit concerns cleared. The final review pass
  completed with zero findings after deterministic gates and the live
  compatibility stop were rechecked. Evidence:
  `/tmp/coderabbit-rentaneko-1-1-1-stage-c-followup-5.out`.
- [x] Stage D: update the roadmap and execplan for the current implementation
  status. Task 1.1.2 records that the existing upstream route is compatible
  when the client supplies `Content-Type: application/json`.
- [x] Quality gates green: `make fmt`, `make check-fmt`, `make markdownlint`,
  `make lint`, `make test`.
- [x] `coderabbit review --agent` concerns cleared.
- [x] Roadmap 1.1.1 marked done.
- [x] 2026-06-26: `make audit` failure triaged. `rsa` 0.9.10 enters the
  lockfile through the test-only `octocrab` / `jsonwebtoken` App JWT path and
  triggers `RUSTSEC-2023-0071`; `proc-macro-error` remains the existing
  `rstest-bdd-macros` unmaintained warning (`RUSTSEC-2024-0370`). An AWS-LC JWT
  backend attempt removed `rsa` from the graph, but
  `cargo test --no-run --test octocrab_compatibility_checkpoint` failed to link
  unresolved `aws_lc_0_41_0_*` symbols even after trying
  `aws-lc-rs/prebuilt-nasm`. Outcome: keep the buildable Octocrab default JWT
  backend and make both audit ignores repo-owned defaults in `make audit`, with
  runtime impact documented in `docs/developers-guide.md`.
- [x] 2026-06-26: final deterministic gates and CodeRabbit review passed for
  the audit-ignore change. Evidence:
  `/tmp/fmt-rentaneko-1-1-1-audit-ignore-final.out`,
  `/tmp/check-fmt-rentaneko-1-1-1-audit-ignore-final.out`,
  `/tmp/markdownlint-rentaneko-1-1-1-audit-ignore-final.out`,
  `/tmp/lint-rentaneko-1-1-1-audit-ignore-final.out`,
  `/tmp/test-rentaneko-1-1-1-audit-ignore-final.out`,
  `/tmp/audit-rentaneko-1-1-1-audit-ignore-final.out`, and
  `/tmp/coderabbit-rentaneko-1-1-1-audit-ignore.out` (`findings: 0`).
- [x] 2026-07-15: rebased this branch onto `origin/main`. The sole zdiff3
  conflict was `.gitignore`; retained `main`'s Python and typos-tool exclusions
  alongside this checkpoint's `node_modules/` exclusion because they serve
  independent generated artefacts. Rebuilt `Cargo.lock` from the merged
  manifests as required. `make check-fmt`, `make test`, and `make typecheck`
  passed. The first `make lint` run hit a transient `clang` bus error while
  building Whitaker's isolated Dylint cache; after removing only the incomplete
  generated `target/dylint` cache, the clean `make lint` rebuild passed.
  `coderabbit review --agent` completed with zero findings.
- [x] 2026-07-17: review follow-up applied the concrete unknown-installation
  HTTP assertion, bounded stderr capture, early runner signals, and production-
  path audit preflight.
- [x] 2026-07-17: review follow-up is green. `make check-fmt`,
  `make markdownlint`, `make typecheck`, `make lint`, `make test`, and
  `make audit` all passed; `coderabbit review --agent` reported zero findings.
- [x] 2026-07-18: removed the committed checkpoint key, generated the RSA-2048+
  RS256 key at runtime with `uselesskey`, and changed both scenarios to await
  explicit checkpoint shutdown. Added `Content-Type: application/json` to the
  App client after a traced live request showed that Simulacat Core otherwise
  rejects the request schema with `400`. The ignored checkpoint now passes for
  both installation `2000` (`FAKE_GITHUB_TOKEN`) and `9999` (typed `404`). A
  separate shutdown unit test would need to mock a real child, process group,
  and reaping boundary; the ignored checkpoint covers that process behaviour,
  while cancellation-interleaving coverage remains task 1.3.2. Evidence: current
  `/tmp/check-fmt-...review-followup.out`,
  `/tmp/markdownlint-...review-followup.out`,
  `/tmp/typecheck-...review-followup.out`, `/tmp/lint-...review-followup.out`,
  `/tmp/test-...review-followup.out`, `/tmp/audit-...review-followup.out`, and
  `/tmp/coderabbit-...review-followup.out` (`findings: 0`).

## Surprises & discoveries

- Observation: `leta workspace add` succeeded, but `leta grep` failed because
  `rust-analyzer` closed during startup. This does not block Stage A because
  the current work is package resolution and a throwaway TypeScript entrypoint,
  but Rust symbol navigation should be retried before the Rust implementation
  stage. Evidence:
  `leta grep '.*' 'src/|tests/' -k function,method,struct,enum` returned
  `Language server 'rust-analyzer' for rust failed to start`. Impact: use
  repository-local inspection for non-symbol material and retry `leta` after
  checking the installed toolchain before editing Rust helpers.
- Observation: the lifecycle note that said graceful shutdown and
  forced-termination were omitted is now stale; the later 2026-07-18 update
  owns those behaviours, while managed lifecycle/cancellation and artefact
  supersession still belong to roadmap task 1.3.2.
- Observation: the Bun readiness schema is enforced in the parser and covered
  by parser tests for `version == 1` plus the port range; that contract is
  distinct from the Octocrab `Content-Type` compatibility issue.
- Observation: installing the pinned toolchain's `rust-analyzer` component and
  restarting the `leta` daemon restored Rust symbol discovery. Evidence:
  `rustup component add rust-analyzer`, `leta daemon restart`, and a follow-up
  `leta grep` over `src/|tests/` listed `src/lib.rs:6 [Function] greet`.
  Impact: Rust navigation can be used for the implementation stage.
- Observation: `rstest-bdd` 0.5.0 does not re-export its procedural macros; its
  own tests and the local user guide import
  `rstest_bdd_macros::{given, scenario, then, when}`. Evidence:
  `cargo info rstest-bdd-macros@0.5.0` and `docs/rstest-bdd-users-guide.md`
  §"Using `#[scenario]` with async". Impact: the BDD skeleton requires
  `rstest-bdd-macros = "0.5"` as a dev-dependency alongside
  `rstest-bdd = "0.5"`.
- Observation: Octocrab 0.51.0 can avoid the RustSec-advised `rsa` crate by
  switching from its default `jwt-rust-crypto` path to `jwt-aws-lc-rs` and a
  matching `jsonwebtoken/aws_lc_rs` direct dev-dependency, but the resulting
  AWS-LC graph failed to link in this worktree with unresolved
  `aws_lc_0_41_0_*` symbols. Enabling `aws-lc-rs/prebuilt-nasm` did not change
  the failure. Impact: do not switch this checkpoint to AWS-LC until the native
  link issue is solved separately; use the documented audit ignore for the
  no-fixed-upgrade `rsa` advisory.
- Observation: `bun add simulacat-core` against the public npm registry failed
  with `GET https://registry.npmjs.org/simulacat-core - 404`, so the plan's git
  fallback was required. Evidence:
  `/tmp/bun-add-rentaneko-1-1-1-minimal-octocrab-to-simulacat-compatibility-check.out`.
  Impact: `package.json` pins
  `github:leynos/simulacat-core#79b51f314238d7d602b73fede7bd27b10f206b6e`.
- Observation: the SHA-pinned package installs with `simulacat-core` 0.6.4 and
  `@simulacrum/foundation-simulator` 0.6.1. A fresh root `bun install` installs
  143 packages in roughly 83 ms on this machine and leaves `node_modules/` at
  66 MiB. Evidence: `bun pm ls`, `bun.lock`, and
  `/tmp/bun-install-fresh-after-coderabbit-rentaneko-1-1-1.out`. Impact: the
  committed lockfile is sufficient for the checkpoint runner's package import
  when Bun is launched with `--conditions development`; no generated `dist/`
  files under `node_modules/` are needed.
- Observation: importing `simulacat-core` by package name from the git
  dependency fails after a fresh install under Bun's default export conditions
  because the package's `exports` point at absent `dist/` artefacts. Building
  those artefacts requires package-local dev tooling and, under this Node
  version, `tsdown --config-loader unrun`. Evidence: the first
  `bun -e 'import { simulation } from "simulacat-core"'` failed with
  `Cannot find package 'simulacat-core'`;
  `bun run --cwd node_modules/simulacat-core build` failed with a
  `tsdown.config.ts` loader error;
  `bun run --conditions development tests/checkpoint_support/checkpoint_runner.ts`
  resolved the package's documented development export to `src/index.ts` after
  deleting and reinstalling `node_modules/`. Impact: the throwaway runner uses
  a normal package import, and the Rust harness must launch Bun with
  `--conditions development`.
- Observation: the hand-started throwaway runner printed
  `{"version":1,"event":"listening","host":"127.0.0.1","port":43423}` on a
  fresh install; `POST /app/installations/2000/access_tokens` returned `201`
  with `token:"FAKE_GITHUB_TOKEN"` and `permissions`, while installation `9999`
  returned `404`. Evidence: `/tmp/stage-a-runner-fresh-rentaneko-1-1-1.out` and
  the curl transcript from 2026-06-24. Impact: Stage A's go/no-go condition
  passed; Stage B/C may proceed.
- Observation: after the Stage A CodeRabbit review, the runner still printed a
  valid readiness line and served the same `201`/`404` route outcomes after the
  signal-handler, shutdown, port-validation, and error-diagnostic fixes.
  Evidence: `/tmp/stage-a-runner-after-coderabbit-rentaneko-1-1-1.out`. Impact:
  CodeRabbit's Stage A concerns were addressed without changing the simulator
  compatibility result.
- Observation: after the second Stage A CodeRabbit review, the runner used the
  standard `simulacat-core` package import, declared the Bun engine constraint,
  bounded `listen`, flattened port extraction, and still served installation
  `2000` with `FAKE_GITHUB_TOKEN` and installation `9999` with `404`. Evidence:
  `/tmp/stage-a-runner-package-import-rentaneko-1-1-1.out`. Impact: the
  remaining Stage A CodeRabbit concerns were addressed.
- Observation: after the final Stage A CodeRabbit lifecycle review, the runner
  guarded shutdown against repeated signals, clears the startup timeout timer,
  and still served installation `2000` with `FAKE_GITHUB_TOKEN` and installation
  `9999` with `404`. Evidence:
  `/tmp/stage-a-runner-final-lifecycle-rentaneko-1-1-1.out`. Impact: the Stage
  A runner lifecycle concerns were addressed.
- Observation: after the retry CodeRabbit review, shutdown waits are bounded to
  3 seconds, signal handlers are registered only after `listen` returns a
  handle, `packageManager` declares `bun@1.3.11`, and the runner still served
  installation `2000` with `FAKE_GITHUB_TOKEN` and installation `9999` with
  `404`. Evidence: `/tmp/stage-a-runner-shutdown-timeout-rentaneko-1-1-1.out`.
  Impact: the remaining Stage A CodeRabbit concerns were addressed.
- Observation: after the final metadata and validation review, `package.json`
  is private, port validation rejects non-finite and out-of-range values, the
  shutdown handle guard is explicit, and the runner still served installation
  `2000` with `FAKE_GITHUB_TOKEN` and installation `9999` with `404`. Evidence:
  `/tmp/stage-a-runner-private-port-guard-rentaneko-1-1-1.out`. Impact: the
  final Stage A CodeRabbit concerns were addressed.
- Observation: early live checkpoint runs appeared to fail on the happy-path
  `installation_token_with_buffer` call with a missing `message` field. The
  2026-07-18 HTTP trace supersedes that diagnosis: Octocrab omitted
  `Content-Type: application/json`, so Simulacat Core returned a request-schema
  `400` with a non-GitHub error body. Configuring the header makes the
  unchanged installation `2000` route return `FAKE_GITHUB_TOKEN` and makes
  `9999` a typed `404`. Evidence: `/tmp/octocrab-unknown.strace`. Impact: no
  upstream response change or token-response rewrite is required.
- Observation: Simulacat Core or its transitive dependencies can emit a
  `FORCE_COLOR`/`NO_COLOR` warning to stdout before the readiness JSON in this
  environment. Evidence: the first hand run captured the warning as the first
  stdout line, before the JSON readiness line. Impact: the Rust readiness loop
  must ignore non-JSON stdout lines and keep scanning until it sees a valid
  `listening` event, an `error` event, EOF, or timeout.
- Observation: `simulacat-core` does ship a CLI (`bin/start.cjs`), but it calls
  `simulation()` with no `initialState` and binds a fixed port with
  human-readable output, so it cannot serve the seeded token route the
  checkpoint needs. Evidence: `bin/start.cjs` and `package.json` `"bin"` field
  on the `main` branch. Impact: the checkpoint must use the programmatic
  `simulation({initialState})` path via a small custom Bun entrypoint, not the
  packaged CLI.
- Observation: the design's seed shape is correct and directly supported.
  Evidence: `simulacat-core` `tests/installations.test.ts` seeds
  `installations: [{id: 2000, account: 'lovely-org'}]` plus a matching
  `organizations` entry and asserts
  `POST /app/installations/2000/access_tokens` returns `201` with
  `token === 'FAKE_GITHUB_TOKEN'`. The zod `githubAppInstallationSchema` accepts
  `id`, `account`, and `app_id`, preserves caller-supplied ids verbatim, and
  the route matches strictly on numeric `id` (auth and `app_id` are ignored for
  the match). Impact: seeding
  `installations: [{id: 2000, account: "rentaneko", app_id: 1}]` is sufficient
  for the token. The upstream test pairs a matching
  `organizations: [{login: "rentaneko"}]` entry, which design §6 lists as
  conditional — Stage A confirmed the seeded route itself is reachable. The
  final checkpoint confirms the real Octocrab request is also compatible when
  it supplies the route's required JSON content type.
- Observation: CI does not run `make test`. `.github/workflows/ci.yml` runs
  `check-fmt`, `markdownlint`, `audit`, `lint`, then a `generate-coverage`
  action. `make lint` (`--all-targets --all-features`) is therefore the CI gate
  that compiles and Clippy-lints the checkpoint code; the coverage action runs
  the non-ignored unit test. Evidence: `.github/workflows/ci.yml` steps.
  Impact: the plan's "runs in CI" claims are scoped accordingly — see Risks.

## Decision log

- Decision: deliver the checkpoint as an opt-in (`#[ignore]`) behavioural test
  (rstest-bdd, two scenarios) plus a Bun-free `rstest` unit test for the port
  extractor. Rationale: `make test` runs `--all-features`, so a Cargo feature
  gate cannot keep a Bun-dependent test out of the default run; `#[ignore]` is
  the only mechanism that keeps the gates green without Bun while still
  compiling and linting the new code. Two scenarios (happy + negative) justify
  the BDD layer over a bare assertion. Date/Author: 2026-06-21, planning agent.
- Decision: keep the live Simulacat Core process as the *primary* proof and use
  the ADR's in-process `wiremock` stub only as a *diagnostic* triage step.
  Rationale: ADR 001 explicitly rejects the in-process stub as the backend for
  the Podbot proof (it would create a second source of truth) but reserves it
  to isolate octocrab-parsing faults from route faults. Making wiremock primary
  would contradict the design of record; using it for triage honours it.
  Date/Author: 2026-06-21, planning agent.
- Decision: emit the full v1 wire shape
  `{"version":1,"event":"listening","host":"127.0.0.1","port":N}` from the
  throwaway entrypoint even though the extractor only needs the port.
  Rationale: design §8 makes `version` a mandatory discriminant; emitting the
  real shape now keeps Stage A's hand-verification aligned with the contract
  and avoids shipping a structurally invalid example the 1.2.2 parser would
  reject. Date/Author: 2026-06-21, planning agent.
- Decision: put all new crates in `[dev-dependencies]`, not `[dependencies]`.
  Rationale: 1.1.1 adds no public API; keeping `octocrab` and friends as
  dev-dependencies preserves the narrow public surface (design §2, §9) until
  the fixture API lands in roadmap 1.4. Date/Author: 2026-06-21, planning agent.
- Decision: drive everything through the scenario's current-thread Tokio runtime
  using `tokio::process` for the child, rather than `std::process` plus a
  separate runtime. Rationale: avoids building nested runtimes (which
  `octocrab` construction dislikes), keeps fixture borrows valid across
  `.await`, and matches the rstest-bdd async guidance in
  [rstest-bdd-users-guide.md](../rstest-bdd-users-guide.md) §"Async scenario
  execution". Date/Author: 2026-06-21, planning agent.
- Decision (superseded): the checkpoint's process handling was originally a
  minimal throwaway RAII guard (named to be un-promotable, e.g.
  `ThrowawayServerGuard`, with a marker comment pointing at 1.3.2) that
  bounded startup, captured stderr, and killed its owned child (process group
  on Linux) on drop. That note is superseded by the 2026-07-18 lifecycle
  update that added bounded graceful shutdown and forced-termination fallback;
  managed `Simulator` lifecycle/cancellation and artefact supersession remain
  roadmap task 1.3.2. Date/Author: 2026-06-21, planning agent.
- Decision: one throwaway server per scenario is acceptable here *only* because
  there is a single ignored scenario family. Rationale: design §9 recommends
  sharing one read-only simulator at module/package scope; 1.3.2 and 1.4.3 must
  adopt that shared scope so the spawn-per-test pattern is not cargo-culted
  into the managed fixture and multiplied across CI wall-clock. Date/Author:
  2026-06-21, planning agent.
- Decision: generate the RSA-2048+ RS256 test key at runtime with `uselesskey`.
  Rationale: a runtime-only key preserves real App-JWT signing without storing
  private-key material in the repository, on disk, or in diagnostics.
  Date/Author: 2026-07-18, implementation agent.
- Decision: accept CodeRabbit's Stage A lifecycle hardening findings for the
  throwaway runner. Rationale: installing signal handlers before `listen`,
  catching `ensureClose` failures, validating the reported port, and emitting
  stack diagnostics reduce checkpoint flakiness while staying within the
  throwaway scope. Date/Author: 2026-06-24, implementation agent.
- Decision: use `bun run --conditions development` when launching the throwaway
  runner. Rationale: this keeps the runner on the standard `simulacat-core`
  package import while selecting the package's source export, so the checkpoint
  does not rely on generated `dist/` output from the git dependency.
  Date/Author: 2026-06-24, implementation agent.
- Decision: keep the throwaway runner's startup timeout and signal cleanup
  explicit even though the Rust harness also has its own timeout and drop
  guard. Rationale: the TypeScript process should fail fast when run by hand or
  by the Rust harness, and duplicated timeout boundaries are acceptable at this
  disposable process edge. Date/Author: 2026-06-24, implementation agent.
- Decision: stop Stage C at the compatibility failure rather than making the
  checkpoint pass via Rust-side payload rewriting or a raw `_post` bypass.
  Rationale: the plan's core invariant is that a real installation-scoped
  `octocrab` client must call `installation_token_with_buffer` and read
  `FAKE_GITHUB_TOKEN` unmodified. The raw route is reachable, but the required
  Octocrab boundary fails. Date/Author: 2026-06-24, implementation agent.
- Decision: keep the default Octocrab / jsonwebtoken RustCrypto JWT backend for
  this test-only checkpoint and make `RUSTSEC-2023-0071` a repo-owned
  `make audit` ignore. Rationale: the advisory has no fixed upgrade, the
  affected path is not shipped as a runtime library dependency, and the AWS-LC
  replacement backend was tested but failed deterministic compilation gates in
  this environment. The ignore must be removed when Octocrab/jsonwebtoken ship
  a buildable fixed backend or when the advised dependency leaves the graph.
  Date/Author: 2026-06-26, implementation agent.
- Decision: extend the checkpoint guard with bounded graceful shutdown and
  forced-termination fallback. Rationale: this corrects ordinary checkpoint
  teardown without claiming ownership of the managed `Simulator` lifecycle;
  cancellation coverage and artefact supersession remain roadmap task 1.3.2.
  Date/Author: 2026-07-18, implementation agent.
- Decision: configure the checkpoint App client with
  `Content-Type: application/json`. Rationale: a traced real Octocrab request
  otherwise receives Simulacat Core's request-schema `400`, whose non-GitHub
  error body previously appeared as Octocrab deserialization failure. The
  existing simulator response is unchanged; with the header, installation
  `2000` returns `FAKE_GITHUB_TOKEN` and `9999` becomes a typed `404`.
  Date/Author: 2026-07-18, implementation agent.
- Decision: keep the Bun readiness schema parser strict on the v1 listening
  event shape (`version == 1`, `port` in `1..=65535`) and cover that contract
  with parser tests. Rationale: this is the readiness follow-up from the
  review pass; the Octocrab `Content-Type` fix remains a separate HTTP
  compatibility decision. Date/Author: 2026-07-19, review pass.

## Outcomes & retrospective

The checkpoint was delivered and reviewed. The throwaway Bun runner seeds
Simulacat Core with installation `2000`; the real installation-scoped Octocrab
client receives `FAKE_GITHUB_TOKEN`; and installation `9999` produces a typed
`404 Not Found` GitHub error. The required client configuration is
`Content-Type: application/json`. Without it, Simulacat Core returns a `400`
request-schema error whose body does not match Octocrab's GitHub-error model.

The implementation therefore proves roadmap task 1.1.1 compatibility and closes
task 1.1.2 with no upstream payload or route change. Rentaneko did not fork or
rewrite the token response. The checkpoint guard's explicit graceful teardown
is limited to this disposable harness; managed lifecycle cancellation coverage
and artefact supersession remain task 1.3.2.
This review pass removed the obsolete committed-key recovery wording and kept
the readiness follow-up separate from the Octocrab header fix: the Bun parser
enforces `version == 1` and the port range, while
`Content-Type: application/json` addresses the HTTP compatibility finding.

## Context and orientation

Rentaneko is a Rust library crate (edition 2024, pinned nightly toolchain). Its
job is to be the Rust counterpart of Simulacat: start a GitHub-shaped simulator
and hand back a real `octocrab` client. Three projects share the boundary:
Simulacat Core owns GitHub state and HTTP behaviour (a Bun/TypeScript package);
Rentaneko owns Rust fixture lifecycle and `octocrab` construction; Podbot owns
token-file semantics.

Relevant existing files, by full repository-relative path:

- `src/lib.rs` — the crate root; currently only a `greet` stub. Unchanged here.
- `tests/stub.rs` — a generated placeholder test. Leave it until real tests
  exist elsewhere; this plan does not delete it.
- `Cargo.toml` — package metadata and the strict lint policy. This plan adds a
  `[dev-dependencies]` table only.
- `clippy.toml` — function-level ceilings (70 lines, complexity 9, 4 arguments,
  nesting 4) enforced by `make lint`.
- `Makefile` — public gates: `make check-fmt`, `make lint`, `make test`
  (`TEST_FLAGS = --all-targets --all-features`; prefers `cargo nextest run`,
  falls back to `cargo test`, always runs doctests).
- `.github/workflows/ci.yml` — runs `check-fmt`, `markdownlint`, `audit`,
  `lint`, then a coverage action; it does **not** run `make test`.
- `docs/rentaneko-design.md` — the design of record. §5 (fail-fast checkpoint),
  §6 (simulator state contract), §8 (event contract / `version` discriminant),
  §10 (octocrab construction), §12 (upstream dependency), §13 (failure modes),
  §14 (verification strategy).
- `docs/adr-001-use-simulacat-core-for-octocrab-spike.md` — the backend
  decision; the in-process stub is a diagnostic option only.
- `docs/roadmap.md` — tasks 1.1.1 and 1.1.2.
- `docs/repository-layout.md`, `docs/contents.md` — update when files or
  top-level entries are added.
- `docs/rstest-bdd-users-guide.md`, `docs/rust-testing-with-rstest-fixtures.md`,
  `docs/reliable-testing-in-rust-via-dependency-injection.md` — testing
  references to follow.

Terms of art, defined on first use:

- *Throwaway Simulacat Core process*: a Bun process the checkpoint starts and
  kills itself, with no persistent lifecycle handle. Sanctioned by design §5
  and subject to the supersede-and-delete clause.
- *Readiness line*: a single line of JSON the Bun entrypoint prints to standard
  output once the server is listening, of the v1 shape
  `{"version":1,"event":"listening","host":"127.0.0.1","port":N}`. The
  checkpoint reads only the fields it needs (`event`, `host`, `port`); the full
  v1 NDJSON contract (noise tolerance, error events, additive fields) is
  roadmap 1.2.2 and is deliberately *not* validated here.
- *App-authenticated `octocrab`*: a client built with
  `Octocrab::builder().app(AppId, EncodingKey)` that signs requests with an
  RS256 JSON Web Token derived from an RSA private key.

Verified external facts that this plan depends on (from source inspection of
`octocrab` 0.51.0 and `simulacat-core` `main` /
`@simulacrum/foundation-simulator` 0.6.1):

- `octocrab::Octocrab::builder().base_uri(uri)?` returns `Result<Self>` and does
  **not** append `/api/v3`; it trims trailing slashes and appends the request
  path, so base URI `http://127.0.0.1:PORT` yields
  `POST http://127.0.0.1:PORT/app/installations/2000/access_tokens`.
- `.app(app_id: AppId, key: jsonwebtoken::EncodingKey) -> Self` is infallible;
  `.build()?` returns `Result<Octocrab>`. `AppId` and `InstallationId` live at
  `octocrab::models::{AppId, InstallationId}`, are `pub struct _(pub u64)` with
  `From<u64>`.
- The token method is the `async fn`
  `installation_token_with_buffer(&self, buffer: chrono::Duration)` returning
  `Result<secrecy::SecretString>`. It takes **no** id argument; the client must
  first be scoped with `octo.installation(InstallationId(2000))?`. Read the
  value with `secrecy::ExposeSecret::expose_secret()`.
- Simulacat Core's route requires `Content-Type: application/json`. With that
  header, the real installation-scoped `installation_token_with_buffer` call
  returns `FAKE_GITHUB_TOKEN` from its `token` and `permissions` payload; an
  unseeded installation becomes Octocrab's typed `404 Not Found` GitHub error.
- `octocrab` 0.51.0 depends on `jsonwebtoken` major `10` (not re-exported, so a
  direct dev-dependency is required) and `secrecy` `0.10`. The checkpoint keeps
  Octocrab's default RustCrypto JWT backend because the AWS-LC backend removed
  `rsa` but failed to link in this environment. The `rsa` advisory is therefore
  handled by the documented `make audit` ignore while this code remains
  test-only. A Tokio runtime is required.
- The runtime-only `uselesskey` RSA `rs256` fixture supplies an RSA-2048+ key
  for App JWT signing without persisting or logging private-key material.
- `simulation({initialState})` returns a `FoundationSimulator`;
  `await handle.listen(0, "127.0.0.1")` resolves to
  `{server, port, ensureClose}`, binding an OS-assigned port readable from
  `handle.server.address().port`. `handle.ensureClose()` shuts it down.
- Simulacat Core requires Bun (pinned `bun@1.3.11`). It is **not** confirmed on
  a public npm registry under the name `simulacat-core`; dependency resolution
  is a go/no-go gate (Stage A).

## Plan of work

### Stage A: resolve the Bun dependency and prove the server (go/no-go)

No Rust changes yet. Confirm a throwaway Simulacat Core server can be started
from a Bun entrypoint and serves the seeded token route.

1. Add a root `package.json` declaring `simulacat-core` and resolve it. Try, in
   order: (a) `bun add simulacat-core`; (b) on failure, a git dependency pinned
   to a commit SHA `"simulacat-core": "github:leynos/simulacat-core#<sha>"` plus
   `bun install`, adding a build step only if the package ships sources rather
   than `dist/`; (c) if neither yields an importable `simulation` export, stop
   and escalate (the checkpoint is blocked on upstream packaging — this becomes
   the 1.1.2 finding). Prefer (a); never use a floating branch ref.
2. Record the resolved `simulacat-core` and `@simulacrum/foundation-simulator`
   versions (`bun pm ls`), the install wall-time, and `node_modules` size in
   `Surprises & Discoveries`.
3. Write the throwaway Bun entrypoint and start it by hand (capturing its PID);
   confirm it prints the v1 readiness line and that
   `curl -s -X POST http://127.0.0.1:PORT/app/installations/2000/access_tokens`
   returns `201` with `FAKE_GITHUB_TOKEN`, and that an unknown id returns
   `404`. This hand-started variant (design §5) proves the route before Rust is
   involved. Stop the server with the captured PID.
4. Generate the test signing key at runtime with `uselesskey` and decide the
   `wiremock` triage posture. No `wiremock` code is written unless Stage C
   debugging needs it.

Go/no-go: if step 1 cannot produce an importable module, or step 3 does not
return `FAKE_GITHUB_TOKEN`, do not proceed — record the outcome for 1.1.2.

### Stage B: red tests

1. Add the `rstest` unit test for the readiness-line port extractor first,
   asserting a pure function `parse_listening_port(line: &str) -> Option<u16>`
   exists and behaves. Run it; it fails to compile (function absent) — the red
   state. This test needs no Bun and is executed by CI's coverage action.
2. Add the Gherkin feature `tests/features/octocrab_compatibility.feature` with
   two scenarios (acquire token for installation `2000`; unknown installation
   is rejected) and `#[scenario]`-bound async tests in
   `tests/octocrab_compatibility_checkpoint.rs`. With the harness not yet wired
   the scenarios fail. Capture the failures.
3. Verify `#[ignore]` is forwarded to the generated scenario tests (run the full
   suite and confirm they are reported as ignored, not failed). If not
   forwarded, switch to plain `#[rstest] #[ignore]` async tests per the Risks
   fallback and keep the feature file as documentation.

### Stage C: implementation

1. Add `[dev-dependencies]` to `Cargo.toml` (see `Interfaces and dependencies`).
2. Generate the RSA-2048+ RS256 test key at runtime with `uselesskey`.
3. Implement `tests/checkpoint_support/checkpoint_runner.ts` (the Bun
   entrypoint) emitting the v1 readiness line and wrapping its body in
   try/catch that prints a structured `error` line and exits non-zero on
   failure.
4. Implement the throwaway harness in `tests/checkpoint_support/mod.rs`
   (a subdirectory module, not compiled as a separate test binary), composed of
   small sub-70-line helpers: locate Bun; spawn the runner with
   `tokio::process::Command` (stdout **and** stderr piped, stdin kept open) and
   construct the `ThrowawayServerGuard` *immediately* so the child is owned
   before any further `?`; read stdout lines inside a `tokio::time::timeout`,
   terminating on readiness, EOF (child died → surface captured stderr), or
   timeout; build the base URI. Ordinary teardown sends `SIGTERM`, waits for a
   bounded graceful exit, then force-kills and reaps only if needed; `Drop`
   remains last-resort cleanup. Helpers return `Result`; only `#[test]`/
   `#[rstest]` bodies use `.expect`.
5. Implement the `octocrab` interaction inside the async steps: build the App
   client against the base URI, scope to installation `2000`, call
   `installation_token_with_buffer(chrono::Duration::seconds(60))`, expose the
   secret, and assert it equals `FAKE_GITHUB_TOKEN`; in the negative scenario,
   assert an unseeded id yields an `octocrab` error.
6. Make the port-extractor unit test pass and run the opt-in checkpoint against
   the real compatibility boundary. The implemented checkpoint passes the
   installation `2000` happy path and typed `9999` rejection without changing
   Simulacat Core's response.

### Stage D: document the checkpoint outcome

1. Keep every function within the `clippy.toml` ceilings and the test file under
   400 lines; the harness already lives in `tests/checkpoint_support/mod.rs`,
   referenced via `mod checkpoint_support;`.
2. Update `docs/repository-layout.md` (new `tests/features/`,
   `tests/checkpoint_support/`, `package.json`, Bun lockfile),
   `docs/contents.md` if a new doc is added, `docs/developers-guide.md` (a
   "Compatibility checkpoint" subsection: how to run it, the throwaway-harness
   convention, and the supersede-and-delete trigger at 1.3.1/1.3.2),
   `docs/users-guide.md` (the new opt-in test command and the Bun
   prerequisite), and `.gitignore` (append `node_modules/`; commit the Bun
   lockfile).
3. Record the checkpoint outcome for task 1.1.2: the existing route is
   compatible when the App client sends `Content-Type: application/json`; mark
   1.1.2 complete without changing the simulator response.
4. Run all gates, then `coderabbit review --agent`, clear concerns, and mark
   roadmap 1.1.1 done.

## Concrete steps

Run from the repository root unless stated otherwise.

Stage A — resolve and hand-verify (example transcript):

```bash
bun add simulacat-core || true   # else add a SHA-pinned git dependency
bun pm ls | grep -iE 'simulacat|foundation-simulator'   # record versions
PID=""
bun run tests/checkpoint_support/checkpoint_runner.ts & PID=$!
# {"version":1,"event":"listening","host":"127.0.0.1","port":49213}
curl -s -X POST -H 'content-type: application/json' \
  http://127.0.0.1:49213/app/installations/2000/access_tokens -d '{}'
# {"token":"FAKE_GITHUB_TOKEN","expires_at":"2030-07-11T22:14:10Z", ...}
curl -s -o /dev/null -w '%{http_code}\n' -X POST \
  http://127.0.0.1:49213/app/installations/9999/access_tokens -d '{}'   # 404
kill "$PID"   # never leave the hand-started server orphaned
```

The runtime test harness generates its RSA-2048+ RS256 signing key with
`uselesskey`; it never writes or logs private-key material.

Run the Bun-free unit test (executed by CI's coverage action):

```bash
cargo nextest run -E 'test(parse_listening_port)'
```

Run the opt-in checkpoint (requires Bun + Simulacat Core):

```bash
cargo nextest run --run-ignored all -E 'test(octocrab_compatibility)'
```

Full gates before each CodeRabbit review:

```bash
make check-fmt 2>&1 | tee /tmp/check-fmt-rentaneko-1-1-1.out
make lint      2>&1 | tee /tmp/lint-rentaneko-1-1-1.out
make test      2>&1 | tee /tmp/test-rentaneko-1-1-1.out
```

## Validation and acceptance

Red-Green-Refactor evidence to capture:

- Port extractor — Red: `cargo nextest run -E 'test(parse_listening_port)'`
  fails to compile because `parse_listening_port` does not exist. Green: after
  adding the function, the parameterized cases pass (valid v1 `listening` line →
  `Some(port)`; `error` event, non-JSON noise, wrong host, missing/oversized
  port → `None`). Refactor: rerun after tidying; still green.
- Checkpoint — Red: omit the JSON content-type header and the simulator returns
  a schema-validation `400`; this is surfaced as a non-GitHub Octocrab error.
  Green: with the header configured on the real App client,
  `cargo nextest run --run-ignored all -E 'test(octocrab_compatibility)'`
  returns `FAKE_GITHUB_TOKEN` for `2000` and a typed `404 Not Found` error for
  `9999`. The checkpoint does not patch or bypass the token response.

Behaviour-driven specification (embedded; keep synchronized with the test):

```gherkin
Feature: Octocrab consumes the Simulacat Core installation-token route

  Background:
    Given a throwaway Simulacat Core seeded with installation 2000 for app 1
    And an App-authenticated octocrab client pointed at the simulator

  Scenario: Acquire an installation token from a throwaway Simulacat Core
    When the client requests an installation token for installation 2000
    Then the token equals "FAKE_GITHUB_TOKEN"

  Scenario: An unknown installation is rejected
    When the client requests an installation token for installation 9999
    Then octocrab returns an error
```

Quality criteria (what "done" means):

- Tests: the port-extractor `rstest` cases pass (and are exercised by CI's
  coverage action); both `#[ignore]`d checkpoint scenarios compile and are
  skipped (not failed) by default; when run locally with Bun present, both
  scenarios pass with the token and typed-error assertions.
- Lint/typecheck: `make lint` passes with warnings denied (rustdoc, Clippy with
  the `clippy.toml` ceilings, Whitaker). No lint suppressions added.
- Format: `make check-fmt` and `make markdownlint` pass; `make fmt` applied to
  Markdown changes.
- Compatibility: the token route still returns `FAKE_GITHUB_TOKEN` outside
  Rentaneko's Rust path, the token is never modified in Rust, the
  unknown-installation path is a typed `404`, and the real Octocrab happy path
  returns the expected token with `Content-Type: application/json` configured.

Quality method: run the three gate commands above with `tee`, then
`coderabbit review --agent` only after they are green, and clear all concerns
before marking the task done.

## Idempotence and recovery

- `bun add` / `bun install` are re-runnable; the committed lockfile makes them
  deterministic. Delete `node_modules/` and rerun to recover from a corrupt
  install.
- The checkpoint owns and kills only its own child (process group on Linux); a
  failed or panicked run leaves no managed state because the guard is
  constructed atomically with the spawn. The Stage A hand-started server is
  stopped via the captured `$PID`; the test-managed harness never relies on it.
- If generating at runtime, no artefact persists.
- All Rust changes are confined to `tests/`, `Cargo.toml`'s
  `[dev-dependencies]`, and docs, so reverting the commit fully restores the
  prior state. The supersede-and-delete clause governs removal when 1.3.1/1.3.2
  land.

## Artefacts and notes

The throwaway Bun entrypoint (final form to verify against the resolved
`foundation-simulator` version; note the v1 readiness shape and the error path):

```typescript
/** @file Throwaway Simulacat Core server for the 1.1.1 compatibility checkpoint.
 *  Superseded by the managed runner in roadmap 1.3.1 — delete then. */
import {simulation, type InitialState} from "simulacat-core";

const initialState: InitialState = {
  users: [],
  installations: [{id: 2000, account: "rentaneko", app_id: 1}],
  organizations: [{login: "rentaneko"}],
  repositories: [],
  branches: [],
  blobs: [],
};

try {
  const app = simulation({initialState});
  const handle = await app.listen(0, "127.0.0.1");
  const address = handle.server.address();
  const port = typeof address === "object" && address ? address.port : handle.port;
  process.stdout.write(
    `${JSON.stringify({version: 1, event: "listening", host: "127.0.0.1", port})}\n`,
  );
  const shutdown = async () => {
    await handle.ensureClose();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
} catch (error) {
  process.stdout.write(
    `${JSON.stringify({version: 1, event: "error", message: String(error)})}\n`,
  );
  process.exit(1);
}
```

The committed `.ts` is not covered by the repository's Rust lint regime; keep
it minimal (≤30 lines). A Biome gate for TypeScript can be added later (see the
`biomejs` skill) but is out of scope for this checkpoint.

Minimal `octocrab` call shape used inside the async step (illustrative):

```rust,no_run
use chrono::Duration;
use octocrab::Octocrab;
use octocrab::models::{AppId, InstallationId};
use secrecy::ExposeSecret;

let key = runtime_signing_key()?;
let client = Octocrab::builder()
    .base_uri(base_uri)? // e.g. "http://127.0.0.1:49213"
    .app(AppId(1), key)
    .build()?;
let token = client
    .installation(InstallationId(2000))?
    .installation_token_with_buffer(Duration::seconds(60))
    .await?;
assert_eq!(token.expose_secret(), "FAKE_GITHUB_TOKEN");
```

## Interfaces and dependencies

Add only a `[dev-dependencies]` table to `Cargo.toml`. Use caret requirements
(repository policy). The lockfile must resolve `octocrab` to exactly `0.51.0`
(Podbot's pin); if it cannot, or any other crate cannot resolve, escalate.

```toml
[dev-dependencies]
octocrab = "0.51.0"                                           # match Podbot's incubator line
tokio = { version = "1", features = ["macros", "rt-multi-thread", "process", "io-util", "time"] }
jsonwebtoken = "10"                                          # Octocrab does not re-export EncodingKey
nix = { version = "0.30", features = ["process", "signal"] }   # process-group cleanup in the throwaway harness
chrono = "0.4"                                                 # installation_token_with_buffer takes chrono::Duration
secrecy = "0.10"                                               # ExposeSecret to read the returned SecretString
serde_json = "1"                                              # parse the readiness line
rstest = "0.26"                                               # align with Podbot's rstest 0.26.1
rstest-bdd = "0.5"                                            # behavioural scenarios; see local users guide
rstest-bdd-macros = "0.5"                                     # procedural macros used by the checkpoint
googletest = "0.13"                                           # expressive assertions
pretty_assertions = "1"                                       # readable equality diffs
# wiremock = "0.6"                                            # uncomment only for ADR-sanctioned triage (Stage A.4)
```

Test-only Rust items to create (no public crate API changes):

- `tests/octocrab_compatibility_checkpoint.rs`:
  - `mod checkpoint_support;` — the harness module below, which owns the pure
    `parse_listening_port(line: &str) -> Option<u16>` helper. The integration
    test unit-tests that helper with `#[rstest]` cases. It parses one JSON
    line, requires `event == "listening"` and `host == "127.0.0.1"`, returns
    the port via `u16::try_from` (no truncating casts), and ignores `version`
    plus any additive fields. Full v1 parsing is roadmap 1.2.2.
  - the `#[scenario]`-bound async tests driving both scenarios; each annotated
    `#[tokio::test(flavor = "current_thread")]` and
    `#[ignore = "requires Bun and Simulacat Core; run with --run-ignored"]`.
- `tests/checkpoint_support/mod.rs`:
  - `ThrowawayServerGuard` — owns the child; explicit shutdown sends its process
    group `SIGTERM`, waits within a bound, then force-kills and reaps only when
    needed. `Drop` is last-resort cleanup. It exposes `base_uri()` and remains
    distinct from the roadmap 1.3.2 `Simulator`.
  - `async fn start_throwaway_server() -> Result<ThrowawayServerGuard, Box<dyn std::error::Error>>`
    — locates Bun, spawns the runner (stdout+stderr piped), constructs the guard
    atomically, awaits readiness within `tokio::time::timeout`, surfaces captured
    stderr on EOF/timeout. Decomposed into sub-70-line helpers.
- `tests/checkpoint_support/checkpoint_runner.ts` — the Bun entrypoint above.
- `tests/features/octocrab_compatibility.feature` — the two Gherkin scenarios.
- `package.json` (+ committed Bun lockfile) — declares `simulacat-core`.

Testing-rigour judgement (recorded per the task's testing brief):

- `rstest` unit test: applicable — the port extractor is the only pure logic.
- `rstest-bdd` behavioural test: applicable — the checkpoint is an externally
  observable, end-to-end workflow; two scenarios (happy + negative) justify the
  layer.
- `insta` snapshots: not applicable — output is a single fixed token with no
  multivariant format to pin.
- `proptest`/`kani`: not applicable — no invariant over a meaningful input range
  beyond the trivial port extractor.
- `verus`: not applicable — no contractual lemma is introduced.

Hexagonal note: the boundary exercised here is the GitHub-client port
(`octocrab`) against the simulator process (the adapter). The checkpoint keeps
that boundary observable but intentionally does **not** introduce
ports/adapters abstractions; the real domain/port separation arrives with the
`Simulator` handle and `OctocrabFixture` in roadmap 1.3–1.4. See the
`hexagonal-architecture` skill — protect the boundary, do not transplant a
pattern into a throwaway checkpoint.

## Documentation and skills signposts

Reference while implementing:

- [rentaneko-design.md](../rentaneko-design.md) §§5, 6, 8, 10, 12, 13, 14.
- [adr-001-use-simulacat-core-for-octocrab-spike.md](../adr-001-use-simulacat-core-for-octocrab-spike.md).
- [roadmap.md](../roadmap.md) tasks 1.1.1 and 1.1.2.
- [rstest-bdd-users-guide.md](../rstest-bdd-users-guide.md) — feature files,
  step macros, and §"Async scenario execution".
- [rust-testing-with-rstest-fixtures.md](../rust-testing-with-rstest-fixtures.md)
  — fixtures, parameterization, async tests.
- [reliable-testing-in-rust-via-dependency-injection.md](../reliable-testing-in-rust-via-dependency-injection.md)
  — deterministic injection of external dependencies.
- [rust-doctest-dry-guide.md](../rust-doctest-dry-guide.md),
  [complexity-antipatterns-and-refactoring-strategies.md](../complexity-antipatterns-and-refactoring-strategies.md),
  and [documentation-style-guide.md](../documentation-style-guide.md).

Skills to load: `rust-router` (then `rust-unit-testing`, `rust-errors`,
`rust-async-and-concurrency`, and `domain-cli-and-daemons` for the
child-process lifecycle), `leta` (navigation), `hexagonal-architecture`
(boundary framing), `nextest` (running and filtering tests, `--run-ignored`),
`biomejs` (only if a TypeScript lint gate is added later), and `proptest`/
`kani` /`verus`/`insta` only if the rigour judgement above changes.

## Revision note

Initial draft (2026-06-21): authored from the roadmap, design, and ADR, with
external API facts verified against `octocrab` 0.51.0 source and
`simulacat-core` / `@simulacrum/foundation-simulator` 0.6.1 source. The
load-bearing question — whether installation `2000` can be seeded so the route
returns `FAKE_GITHUB_TOKEN` — was confirmed answerable "yes" by a bundled
Simulacat Core test. A later live trace showed that the apparent Octocrab
deserialization failure was Simulacat Core's `400` request-schema rejection
when the client omitted `Content-Type: application/json`; the unchanged route
is compatible once that header is configured.

Revision 2 (2026-06-21): incorporated a Logisphere design-review panel
(Pandalump, Telefono, Doggylump, Buzzy Bee, Wafflecat, Dinolump). Changes: emit
the v1 `version:1` readiness shape (was a bare line that §8 would reject); add
a bounded readiness timeout, stderr capture, EOF/error-event handling, and
atomic guard-on-spawn to make the fail-fast path diagnosable and leak-free;
cite the real `clippy.toml` function ceilings (70 lines / complexity 9) rather
than only the 400-line file rule; correct the CI claims (CI runs `make lint`
plus a coverage action, not `make test`); consolidate support files under
`tests/checkpoint_support/`; rename the guard to the un-promotable
`ThrowawayServerGuard`; add a negative (unknown-installation) scenario; add the
supersede-and-delete clause and a per-scenario-startup note versus §9; pin the
`simulacat-core` git dependency to a commit SHA and pin `octocrab` to `0.51.0`
in the lockfile; use runtime-only test signing keys; and keep the ADR's
in-process `wiremock` stub as a diagnostic triage tool (declined as the primary
proof, which ADR 001 reserves for the real client). Wafflecat's wiremock-first
alternative was considered and recorded but not adopted because it conflicts
with ADR 001's decision drivers.
