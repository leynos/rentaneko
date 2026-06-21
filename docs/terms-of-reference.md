# Rentaneko terms of reference

- **Status:** Draft v0.1.
- **Audience:** Rentaneko maintainers, Podbot maintainers, Simulacat Core
  maintainers, and reviewers evaluating the prototype boundary.
- **Companion documents:** This document is read with the design, ADR 001,
  roadmap, documentation contents, and repository-layout guide.
- **Last updated:** 2026-06-21.

## 1. Background and motivation

Rentaneko exists to test Rust GitHub integrations against a local GitHub-shaped
simulator without replacing the Rust client library under test. The immediate
prototype is driven by Podbot roadmap task 3.3.1, which needs confidence that a
real `octocrab` GitHub App installation token can be acquired before Podbot
writes that token into a protected host runtime directory.

The timing is specific. Simulacat Core already owns the GitHub digital twin
state and exposes a scriptable installation-token route. Simulacat already
proves the companion pattern for Python by wrapping the simulator in pytest
fixtures and a `github3.py` client. Podbot now needs the Rust equivalent for
`rstest` and `octocrab`.

## 2. Domain

The domain is GitHub API simulation for automated tests. The relevant practice
is client-shaped testing: tests should drive the same client library used in
production, while the simulator owns GitHub-shaped state and HTTP behaviour.

The local prior art is:

- Simulacat Core: the TypeScript simulator and GitHub state model.
- Simulacat: the Python pytest fixture and `github3.py` adapter.
- Podbot: the Rust consumer that uses `octocrab` for GitHub App credential
  validation and installation-token acquisition.

The spike covers GitHub App installation-token acquisition only. It does not
attempt to model Git smart Hypertext Transfer Protocol (HTTP), repository
cloning, rate limiting, or GitHub's full authentication and authorization rules.

## 3. Market context

The practical alternatives are narrow and already visible in the adjacent
repositories.

| Alternative            | Current value                                              | Deficiency for this spike                                                                          |
| ---------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Handwritten Rust mocks | Fast unit tests for Podbot traits                          | They do not exercise `octocrab` request construction, authentication state, or response parsing.   |
| In-process HTTP stub   | Proves one fixed token route with minimal operational risk | It duplicates simulator payloads and provides no reuse for refresh or repository-preflight slices. |
| Wire-level HTTP stubs  | Can return GitHub-shaped JSON                              | They duplicate simulator behaviour and drift away from Simulacat Core.                             |
| Simulacat              | Proven pytest process fixture                              | It is Python- and `github3.py`-shaped, and its GitHub App support is metadata-only.                |
| Live GitHub            | Highest fidelity                                           | It requires network access, real credentials, and mutable external state.                          |

_Table 1: Alternatives for Podbot GitHub App token tests._

Rentaneko addresses the gap between unit mocks and live GitHub by giving Rust
tests a managed local simulator and an `octocrab` client that points at it.

## 4. Users and stakeholders

| Group                      | Context                                                                 | What they need                                                                                       | What they will reject                                                                      |
| -------------------------- | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Podbot maintainers         | Rust maintainers implementing GitHub App and container credential flows | A fixture that proves Podbot can acquire a token through real `octocrab` before writing it to disk   | A fixture that requires live GitHub credentials or a running container for the 3.3.1 proof |
| Rentaneko maintainers      | Rust library maintainers building the fixture crate                     | A small crate boundary that can grow without committing to every future simulator control surface    | A broad v1 API designed before the first consumer proves the core path                     |
| Simulacat Core maintainers | Owners of the TypeScript simulator and GitHub state model               | Clear upstream contract gaps, not duplicated simulator logic in Rust                                 | Rust-side forks of GitHub domain state                                                     |
| Simulacat maintainers      | Owners of the Python fixture prior art                                  | A sibling design that preserves the process-fixture pattern while respecting Rust client constraints | Python-specific compromises copied into Rust without justification                         |

_Table 2: Stakeholder map for the Rentaneko prototype._

Non-users include projects that need general-purpose HTTP mocking, Git smart
HTTP simulation, or live-GitHub acceptance tests. Those users should use a
dedicated HTTP mock, a Git server fixture, or real GitHub.

## 5. Job to be done

When a Rust project uses `octocrab` to drive GitHub App workflows, the
maintainer wants to test those workflows against deterministic GitHub-shaped
state, so they can catch client compatibility failures without contacting live
GitHub.

For the first Podbot slice, when Podbot implements the token-daemon runtime
directory and atomic token writer, the maintainer wants one integration proof
that the token being written came from a real `octocrab` installation-token
call, so the filesystem test does not hide a broken client boundary.

## 6. Scope

### 6.1. Goals

- Start and stop Simulacat Core from Rust tests through a managed process
  boundary.
- Seed the minimum GitHub App installation state needed for
  `POST /app/installations/{installation_id}/access_tokens`.
- Return an `octocrab::Octocrab` client configured for GitHub App
  authentication and pointed at the simulator.
- Prove Podbot can acquire `FAKE_GITHUB_TOKEN` through its existing
  installation-token port.
- Keep simulator domain behaviour in Simulacat Core rather than reimplementing
  GitHub state in Rust.

### 6.2. Non-goals

- Rentaneko will not implement Git smart HTTP or `git clone` simulation in this
  spike; Podbot's clone path remains later work.
- Rentaneko will not prove atomic rename semantics for Podbot. Podbot owns that
  filesystem behaviour and should test it directly.
- Rentaneko will not implement a refresh loop, token sequence scripting,
  request logging, or error injection for the 3.3.1 proof.
- Rentaneko will not validate real GitHub App JSON Web Tokens (JWTs) unless
  Simulacat Core exposes that as a supported compatibility surface.
- Rentaneko will not expose a stable broad public API before the first Podbot
  integration proof lands.

## 7. Success criteria

- A Rust test can start the simulator, acquire an installation token through
  `octocrab`, and observe the fixed simulator token value.
- Podbot can consume the fixture without replacing its production
  `OctocrabAppClient` path.
- The minimum simulator state is documented and does not require repository,
  branch, or blob fixtures for token acquisition.
- The documented upstream Simulacat Core dependency is either already present
  or named as a precise contract-test task.
- The prototype leaves no ambiguity about which richer behaviours belong to
  Podbot 3.3.2, Podbot 3.4, or later Simulacat Core hardening.

## 8. Constraints and assumptions

### 8.1. Hard constraints

- The simulator backend remains Simulacat Core.
- The Rust client boundary remains `octocrab`; Rentaneko must not swap in a
  bespoke HTTP client for the proof.
- A static in-process HTTP stub may be used only as a diagnostic aid for
  isolating `octocrab` response parsing. It is not the Rentaneko backend for
  the Podbot integration proof.
- The test framework target is `rstest`, matching Podbot's current test
  texture.
- The first Podbot consumer is roadmap task 3.3.1, not the full Git clone
  workflow.
- Documentation and implementation must follow this repository's Rust and
  Markdown standards.

### 8.2. Assumptions

- Simulacat Core's existing installation-token route is compatible with the
  `octocrab` method Podbot uses. The roadmap now proves this before process
  lifecycle work; if the checkpoint fails, the spike must add an upstream
  Simulacat Core contract task before expanding Rentaneko.
- Podbot can construct or accept a simulator-pointed `Octocrab` client for the
  integration proof. If the production seam is too closed, Podbot must add a
  test-facing client injection path.
- A fixed token value is sufficient for 3.3.1. If Podbot moves directly to
  refresh-loop behaviour, token sequencing becomes a new simulator requirement.
- Bun and the Simulacat Core package are available to the fixture runner. If
  not, Rentaneko must fail or skip with a precise diagnostic.

### 8.3. Dependencies

- Simulacat Core installation-token route:
  `POST /app/installations/{installation_id}/access_tokens`.
- Podbot's existing `OctocrabAppClient` and
  `acquire_installation_token_with_client` boundary.
- `octocrab` 0.51.0 compatibility for Podbot's current dependency graph.
- `rstest` fixture usage in the consuming Rust test suite.

## 9. Open questions

| Question                                                                                          | Why it matters                                                                             | Resolution path                                                                                  |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| Should the thin `OctocrabFixture` wrapper remain prototype-only or become a stable public helper? | The design resolves the core API as constructor-shaped, but wrapper stability is separate. | Resolve after the first Podbot integration test shows whether the wrapper earns its API surface. |
| Should Simulacat Core add `GET /app` before or after the 3.3.1 proof?                             | Credential validation uses `GET /app`, but 3.3.1 only requires token acquisition.          | Defer unless Podbot cannot bypass startup credential validation in the integration test.         |
| How should token sequencing be represented for Podbot 3.3.2?                                      | Refresh-loop tests need token A then token B, expiry control, and failure modes.           | Capture in a later design update after 3.3.1 lands.                                              |

## Handoff

The terms of reference is sufficient to begin the design and roadmap for the
prototype. Candidate future Architecture Decision Records (ADRs) are:

- whether the thin `OctocrabFixture` wrapper remains a prototype convenience or
  becomes a stable public helper;
- whether Simulacat Core should treat Octocrab compatibility as a first-class
  roadmap lane alongside `github3.py`.

## References

- [ADR 001: Use Simulacat Core for the Octocrab spike](adr-001-use-simulacat-core-for-octocrab-spike.md).
- Podbot design:
  <https://github.com/leynos/podbot/blob/main/docs/podbot-design.md>.
- Podbot roadmap:
  <https://github.com/leynos/podbot/blob/main/docs/podbot-roadmap.md>.
- Simulacat Core API reference:
  <https://github.com/leynos/simulacat-core/blob/main/docs/api-reference.md>.
- Simulacat Core roadmap:
  <https://github.com/leynos/simulacat-core/blob/main/docs/roadmap.md>.
- Simulacat design:
  <https://github.com/leynos/simulacat/blob/main/docs/simulacat-design.md>.
- GitHub REST API endpoints for GitHub Apps:
  <https://docs.github.com/en/rest/apps/apps?apiVersion=2022-11-28>.
- `octocrab` 0.51.0 `installation_token_with_buffer` documentation:
  <https://docs.rs/octocrab/0.51.0/octocrab/struct.Octocrab.html#method.installation_token_with_buffer>.
- `rstest` crate documentation: <https://docs.rs/rstest/latest/rstest/>.
