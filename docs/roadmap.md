# Rentaneko roadmap

This roadmap turns the
[terms of reference](terms-of-reference.md), [prototype design](rentaneko-design.md),
and [ADR 001](adr-001-use-simulacat-core-for-octocrab-spike.md) into a
short-circuited build order for the Podbot 3.3.1 spike. It follows the Goals,
Ideas, Steps, and Tasks (GIST) model: each phase states a falsifiable idea,
each step answers one delivery question, and each task is a review-sized
execution unit.

The roadmap records intended sequencing only. It does not promise dates.

## 1. Foundation: prove the client contract before process machinery

Idea: if Rentaneko proves the real `octocrab` and Simulacat Core token contract
before building the managed runner, the spike can fail fast on the only
load-bearing GitHub compatibility assumption and avoid hiding simulator drift
behind lifecycle code.

This phase settles the cross-repo contract, the Bun-to-Rust process contract,
and the smallest public Rust fixture surface needed for the Podbot token-writer
proof.

### 1.1. Prove Octocrab can consume the Simulacat Core token route

This step answers whether the existing simulator route is sufficient for the
walking skeleton. Its outcome decides whether Rentaneko can proceed to process
lifecycle work or must first request Simulacat Core compatibility changes. See
rentaneko-design.md §5 and adr-001-use-simulacat-core-for-octocrab-spike.md.

- [x] 1.1.1. Add the minimal Octocrab-to-Simulacat compatibility checkpoint.
  - See rentaneko-design.md §5.
  - Use a hand-started or throwaway Simulacat Core process, the minimum
    installation state, App ID `1`, installation ID `2000`, and real
    `octocrab` 0.51.0 App authentication.
  - Outcome: the opt-in `rstest-bdd` checkpoint and default quality gates pass.
    The real installation-scoped client receives `FAKE_GITHUB_TOKEN` for
    installation `2000` and a typed `404 Not Found` GitHub error for `9999`.
    The required request header is `Content-Type: application/json`; without
    it, Simulacat Core rejects the request before the route is evaluated.
  - Audit status: `make audit` includes documented repo-owned ignores for the
    test-only `rsa` / `jsonwebtoken` advisory (`RUSTSEC-2023-0071`) and the
    existing `rstest-bdd-macros` `proc-macro-error` warning
    (`RUSTSEC-2024-0370`). The AWS-LC JWT backend was evaluated for 1.1.1 but
    failed deterministic compile gates in this environment.
- [x] 1.1.2. Record the exact upstream outcome of the checkpoint.
  - Requires 1.1.1.
  - See rentaneko-design.md §12.
  - Outcome: no Simulacat Core payload or route change is required. The client
    must send `Content-Type: application/json` for the existing endpoint's
    request schema; then its token payload and `404` error response are
    compatible with Octocrab.
  - Success: the documented client configuration preserves the real response
    and avoids a Rust-side token-payload fork.

### 1.2. Pin the Bun-to-Rust contracts

This step answers what Rust may assume about runner configuration and startup
events. Its outcome prevents the readiness protocol and config shape from
living only in prose or incidental implementation. See rentaneko-design.md
§§7-8 and ADR 001.

- [ ] 1.2.1. Implement the v1 runner configuration schema.
  - Requires 1.1.1.
  - See rentaneko-design.md §7.
  - Accept `version`, `initialState`, and `bind`; require `bind.host` to be
    `127.0.0.1` and `bind.port` to be `0` for the prototype.
  - Success: invalid required fields produce a versioned `error` event and a
    non-zero runner exit.
- [ ] 1.2.2. Implement the v1 NDJSON runner event parser.
  - Requires 1.2.1.
  - See rentaneko-design.md §8.
  - Treat `event` as the discriminant, tolerate additive fields, ignore
    unclassified lines while retaining diagnostics, and validate
    `listening.host` and `listening.port`.
  - Success: parser tests cover valid readiness, runner error, non-JSON noise,
    unknown events, malformed readiness, and timeout diagnostics.

### 1.3. Establish the simulator process boundary

This step answers whether Rust can manage the same simulator lifecycle that
Simulacat manages from Python while closing the process-fixture sharp edges
identified in the review. Its outcome informs error handling, temporary file
ownership, and fixture sharing. See rentaneko-design.md §§3-4 and §13.

- [ ] 1.3.1. Add the private Bun runner for Simulacat Core startup.
  - Requires 1.2.1.
  - See rentaneko-design.md §§4, 7, and 8.
  - Read the v1 JSON config file, call `simulation({ initialState })`, bind
    `127.0.0.1:0`, emit the v1 `listening` event with the actual port, close
    on `SIGINT` or `SIGTERM`, and self-terminate when stdin closes.
  - Success: the runner can be launched by hand with a minimal config and
    emits `{"version":1,"event":"listening","host":"127.0.0.1","port":N}`.
- [ ] 1.3.2. Implement the Rust process handle.
  - Requires 1.2.2 and 1.3.1.
  - See rentaneko-design.md §§8, 9, and 13.
  - Own the child process, stdin pipe, temporary directory, base URI, and
    installation ID. Keep `Simulator::start` as the single lifecycle authority.
  - Success: a Rust test starts the runner, waits for readiness, observes the
    selected port, and tears down only the owned child with bounded graceful
    wait and bounded kill fallback.

### 1.4. Produce the Octocrab-shaped Rust fixture

This step answers whether Podbot can use Rentaneko without replacing its
production GitHub adapter. Its outcome decides whether direct `rstest` fixture
exports are needed now or can wait. See rentaneko-design.md §§6, 9, and 10.

- [ ] 1.4.1. Add the deterministic installation seed.
  - Requires 1.3.2.
  - See rentaneko-design.md §6.
  - Seed App ID `1`, installation ID `2000`, and empty user, organization,
    repository, branch, and blob collections.
  - Success: the managed runner exposes
    `POST /app/installations/2000/access_tokens` for the seeded installation.
- [ ] 1.4.2. Build the App-authenticated `octocrab` factory.
  - Requires 1.1.1 and 1.4.1.
  - See rentaneko-design.md §10.
  - Embed a test RSA key, configure `Octocrab::builder()` with the simulator
    base URI and App ID `1`, and align the `octocrab` crate version with
    Podbot's incubator dependency.
  - Success: `installation_token_with_buffer` against installation `2000`
    returns `FAKE_GITHUB_TOKEN` through the managed runner.
- [ ] 1.4.3. Add the narrow `OctocrabFixture` convenience type.
  - Requires 1.4.2.
  - See rentaneko-design.md §9.
  - Compose `Simulator` and `octocrab::Octocrab` while exposing `client()` and
    `installation_id()`. Do not introduce a second process lifecycle owner.
  - Success: a consumer can wrap `OctocrabFixture::start()` in an `rstest`
    fixture without Rentaneko depending on `rstest` macros in the core API.

### 1.5. Add Rentaneko-side drift tripwires

This step answers whether Rentaneko will fail loudly when the Simulacat Core
process contract, `simulation()` API, token route, or payload shape drifts. Its
outcome complements the proposed upstream Simulacat Core contract test. See
rentaneko-design.md §§12 and 14.

- [ ] 1.5.1. Add the packaged-runner Octocrab contract test.
  - Requires 1.4.2.
  - See rentaneko-design.md §§12 and 14.
  - Start the packaged runner, wait for the v1 `listening` event, call the real
    `octocrab` installation-token method, and assert `FAKE_GITHUB_TOKEN`.
  - Success: changing the runner contract, Simulacat Core route, token payload,
    or permissive authentication behaviour breaks a Rentaneko test.

## 2. Podbot spike: prove token acquisition feeds the atomic writer

Idea: if Podbot writes a token acquired through the Rentaneko fixture and
proves its own filesystem invariants separately, the 3.3.1 implementation can
stay small while still exercising the real Rust GitHub client boundary.

This phase belongs mostly in Podbot, but Rentaneko tracks it because it is the
acceptance consumer for the walking skeleton.

### 2.1. Wire Rentaneko into the Podbot 3.3.1 integration test

This step answers whether the fixture shape is adequate for a real consumer.
Its outcome informs whether Rentaneko needs a broader public fixture API. See
terms-of-reference.md §§5-7 and rentaneko-design.md §11.

- [ ] 2.1.1. Add a Podbot integration test that acquires the token through
  Rentaneko.
  - Requires 1.4.3.
  - See rentaneko-design.md §11 and
    <https://github.com/leynos/podbot/blob/main/docs/podbot-roadmap.md#33-refresh-tokens-through-a-host-side-daemon>.
  - Construct Podbot's `OctocrabAppClient` from the Rentaneko client and call
    `acquire_installation_token_with_client`.
  - Success: the token value observed by Podbot is `FAKE_GITHUB_TOKEN` and no
    mock `GitHubInstallationTokenClient` participates in the integration proof.
- [ ] 2.1.2. Feed the acquired token into Podbot's runtime token-file writer.
  - Requires 2.1.1 and Podbot roadmap task 3.3.1.
  - See rentaneko-design.md §§11 and 14.
  - Write the acquired token through Podbot's writer without asserting
    Rentaneko-owned filesystem details.
  - Success: Podbot observes `FAKE_GITHUB_TOKEN` after it passes through real
    `octocrab` and the token-file writer.

### 2.2. Keep atomicity proof in Podbot

This step answers whether Rentaneko is carrying the right responsibility. Its
outcome prevents simulator concerns from obscuring Podbot's filesystem
invariant. See rentaneko-design.md §14.

- [ ] 2.2.1. Add Podbot's simulator-free atomic writer test.
  - Requires Podbot roadmap task 3.3.1.
  - See rentaneko-design.md §§11 and 14.
  - Use large old and new token values with concurrent readers while replacing
    the token by same-directory rename.
  - Success: every read observes either the complete old token or the complete
    new token; no read observes an empty, partial, or mixed value.

## 3. Deferred extensions: broaden only after the first proof is boring

Idea: if the 3.3.1 proof is already deterministic and easy to review, later
GitHub App and clone-workflow extensions can be evaluated on product value
instead of being smuggled into the walking skeleton.

These items are intentionally out of the prototype path.

### 3.1. Support credential validation and richer GitHub App identity

This step would answer whether Rentaneko should cover Podbot's startup
credential validation path. See rentaneko-design.md §§2 and 15.

- [ ] 3.1.1. Add Simulacat Core `GET /app` compatibility for `octocrab`.
  - Requires phase 2.
  - See rentaneko-design.md §15.
  - Success: Podbot's `validate_app_credentials` can run against the simulator
    without a special bypass.

### 3.2. Support refresh-loop and retry behaviour

This step would answer whether Rentaneko can support Podbot 3.3.2 without
turning the fixture into a bespoke mock server. See rentaneko-design.md §15.

- [ ] 3.2.1. Add configurable token sequences and expiry metadata.
  - Requires phase 2 and Simulacat Core token-scenario support.
  - See rentaneko-design.md §15 and
    <https://github.com/leynos/simulacat-core/blob/main/docs/roadmap.md#95-harden-protocol-breadth-for-supported-surfaces>.
  - Success: a Podbot refresh-loop test can observe token A, then token B,
    through real `octocrab`.
- [ ] 3.2.2. Add request-log and error-injection hooks when Simulacat Core
  exposes them.
  - Requires 3.2.1.
  - See rentaneko-design.md §15.
  - Success: Podbot can assert retry count and permanent-failure behaviour
    without replacing the simulator with test-local HTTP handlers.

### 3.3. Support repository access and clone-adjacent workflows

This step would answer whether Rentaneko can prove Podbot's later
repository-preflight and clone boundaries. See terms-of-reference.md §6.2 and
rentaneko-design.md §15.

- [ ] 3.3.1. Add a GitHub App installed-on-repository scenario helper.
  - Requires phase 2 and Simulacat Core actor-aware fixture builders.
  - See rentaneko-design.md §15 and
    <https://github.com/leynos/simulacat-core/blob/main/docs/roadmap.md#13-centralize-writes-and-reusable-fixture-construction>.
  - Success: consumers can seed an installation, repository, and branch without
    writing raw state by hand.
- [ ] 3.3.2. Evaluate a Git server or Git smart HTTP fixture separately from
  Rentaneko's API simulator role.
  - Requires 3.3.1.
  - See terms-of-reference.md §6.2.
  - Success: clone end-to-end tests have a documented backend choice and do not
    force Git protocol behaviour into Simulacat Core by accident.
