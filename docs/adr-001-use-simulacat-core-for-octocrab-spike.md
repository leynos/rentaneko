# Architectural decision record (ADR) 001: Use Simulacat Core for the Octocrab spike

## Status

Accepted on 2026-06-21. Rentaneko will use Simulacat Core as the backend for
the walking skeleton, but the first implementation step must prove `octocrab`
installation-token compatibility before building the managed Bun runner.

## Date

2026-06-21.

## Context and problem statement

Rentaneko is intended to be the Rust counterpart to Simulacat: a fixture crate
that starts a GitHub-shaped simulator and returns a real language-native GitHub
client. The immediate Podbot 3.3.1 spike only needs one proof that an
installation token written to disk can originate from a real
`octocrab::Octocrab` request.

The design review identified two competing pressures:

- an in-process Rust HTTP stub would prove the narrow Podbot 3.3.1 client
  boundary with less operational machinery;
- Simulacat Core is the intended long-term digital twin, and duplicating
  GitHub response logic in Rust would create a second source of truth before
  the fixture has its first consumer.

The review also identified the load-bearing assumptions that must be tested
early: Simulacat Core's existing installation-token route must be compatible
with `octocrab` 0.51.0, and the simulator must accept the App-authenticated
request without requiring JWT validation that the current slice does not
implement.

## Decision drivers

- Keep GitHub domain state and response payload ownership in Simulacat Core.
- Prove the real Rust client boundary used by Podbot, not a hand-rolled trait
  mock.
- Avoid committing to a broad Rentaneko public API before the first consumer
  test exists.
- Fail fast on `octocrab` compatibility before investing in process lifecycle
  code.
- Make the cross-language Bun-to-Rust process contract explicit enough to test
  and maintain.

## Options considered

| Option                                                    | Strengths                                                                                 | Weaknesses                                                                                    | Outcome                                   |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | ----------------------------------------- |
| Simulacat Core backend with fail-fast compatibility proof | Preserves one GitHub simulator authority and grows toward the reusable Rentaneko product. | Requires Bun, process lifecycle handling, and a two-language contract.                        | Chosen.                                   |
| In-process Rust HTTP stub                                 | Removes Bun, readiness parsing, child teardown, and port-race risk for the 3.3.1 slice.   | Duplicates token JSON and provides no reuse for refresh, repository preflight, or `GET /app`. | Rejected as the product path.             |
| Handwritten Podbot mocks                                  | Fast and simple unit tests.                                                               | Does not exercise `octocrab` request construction, authentication state, or response parsing. | Rejected for the integration proof.       |
| Live GitHub                                               | Highest external fidelity.                                                                | Requires credentials, network access, mutable external state, and rate-limit management.      | Rejected for deterministic test fixtures. |

_Table 1: Backend options for the Rentaneko walking skeleton._

## Decision outcome

Rentaneko will use Simulacat Core as the backend for the walking skeleton.
However, the first roadmap step is a compatibility checkpoint: run a minimal
real `octocrab` 0.51.0 installation-token request against a hand-started or
throwaway Simulacat Core process and confirm that `FAKE_GITHUB_TOKEN` parses.

The in-process Rust stub remains a diagnostic option only. It may be used to
isolate whether a failure is caused by `octocrab` payload parsing or by the
Simulacat Core route, but it must not become the Rentaneko backend for the
Podbot integration proof.

The managed runner must bind port zero and report the selected port through a
versioned newline-delimited JSON readiness event. The runner configuration and
readiness event are part of Rentaneko's internal contract and must be covered
by Rentaneko-side tests.

## Requirements

### Functional requirements

- Prove `installation_token_with_buffer` receives `FAKE_GITHUB_TOKEN` from
  Simulacat Core before implementing the persistent Bun runner.
- Return a real `octocrab::Octocrab` client from Rentaneko's public API.
- Let Podbot consume that client through its existing `OctocrabAppClient`
  adapter.
- Keep filesystem atomicity tests in Podbot.

### Technical requirements

- The Bun runner must bind `127.0.0.1:0` and report the actual port.
- Runner stdout must use a versioned newline-delimited JSON event contract.
- Rust teardown must be synchronous in `Drop`: bounded graceful shutdown,
  followed by a bounded kill fallback. Exact durations are implementation
  constants rather than ADR policy.
- The runner must self-terminate when its parent-side stdin closes, so a normal
  fixture drop does not rely solely on signal delivery.
- Rentaneko must include a drift tripwire that starts the packaged runner and
  performs the real `octocrab` token request.

## Known risks and limitations

- The chosen path accepts a cross-language maintenance cost: implementers may
  need to debug Rust async code, `octocrab`, Bun, and Simulacat Core together.
  This is an accepted cost because Rentaneko's product goal is a reusable
  simulator-backed fixture, not a one-off Podbot stub.
- Parent-death cleanup relies on runner stdin reaching end-of-file. On Linux,
  forcefully killing the parent test process normally closes the parent-side
  pipe file descriptor, so this covers the common hard-abort case in practice.
  If CI later shows leaked Bun processes, a narrow stray-runner reaper should
  be added as the backstop rather than expanding the fixture API.
- Simulacat Core can still drift. The Rentaneko-side drift tripwire is required
  because an upstream Simulacat Core contract test alone does not protect
  Rentaneko users who update dependencies independently.

## Architectural rationale

The decision keeps ownership boundaries aligned with the surrounding projects:
Simulacat Core owns GitHub state and HTTP behaviour, Rentaneko owns Rust
fixture lifecycle and `octocrab` construction, and Podbot owns token-file
semantics. Reordering implementation to prove compatibility first reduces the
main spike risk without abandoning the long-term simulator-backed architecture.
