# Rentaneko roadmap

This roadmap turns the [terms of reference](terms-of-reference.md) and
[prototype design](rentaneko-design.md) into a short-circuited build order for
the Podbot 3.3.1 spike. It follows the Goals, Ideas, Steps, and Tasks (GIST)
model: each phase states a falsifiable idea, each step answers one delivery
question, and each task is a review-sized execution unit.

The roadmap records intended sequencing only. It does not promise dates.

## 1. Foundation: prove the Rust fixture can start the simulator and acquire one token

Idea: if Rentaneko can start Simulacat Core and drive one real `octocrab`
installation-token request before adding richer scenario machinery, the
prototype can validate Podbot 3.3.1 without designing a broad fixture framework
too early.

This phase builds only the process skeleton, deterministic installation state,
and `octocrab` client path needed for the token-writer spike.

### 1.1. Establish the simulator process boundary

This step answers whether Rust can manage the same simulator lifecycle that
simulacat manages from Python. Its outcome informs error handling, temporary
file ownership, and fixture teardown. See rentaneko-design.md §§3-4.

- [ ] 1.1.1. Add the private Bun runner for Simulacat Core startup.
  - See rentaneko-design.md §4.
  - Read a JSON config file, call `simulation({ initialState })`, listen on a
    local port, emit a single JSON readiness line, and close on `SIGINT` or
    `SIGTERM`.
  - Success: the runner can be launched by hand with a minimal config and emits
    `{"event":"listening","port":N}`.
- [ ] 1.1.2. Implement the Rust process handle.
  - Requires 1.1.1.
  - See rentaneko-design.md §§4 and 10.
  - Own the child process, temporary directory, base URI, and installation ID.
  - Success: a Rust test starts the runner, waits for readiness, observes
    `/health`, and cleans up only its own child process.

### 1.2. Seed the minimum GitHub App installation state

This step answers whether the Podbot 3.3.1 proof needs repository fixtures or
only an installation row. Its outcome informs the first public fixture builder
and any upstream Simulacat Core compatibility work. See rentaneko-design.md §5.

- [ ] 1.2.1. Add the deterministic installation seed.
  - Requires 1.1.2.
  - See rentaneko-design.md §5.
  - Seed App ID `1`, installation ID `2000`, and empty user, organization,
    repository, branch, and blob collections.
  - Success: `POST /app/installations/2000/access_tokens` returns
    `FAKE_GITHUB_TOKEN`, and an unknown installation returns `404`.
- [ ] 1.2.2. Document the upstream Simulacat Core contract-test request.
  - Requires 1.2.1.
  - See rentaneko-design.md §9.
  - Record the proposed Simulacat Core task beside roadmap step 1.4: contract
    test installation-token acquisition through real `octocrab`.
  - Success: the Rentaneko docs name the exact upstream dependency and do not
    imply that Rentaneko owns token payload generation.

### 1.3. Produce the Octocrab-shaped Rust fixture

This step answers whether Podbot can use Rentaneko without replacing its
production GitHub adapter. Its outcome decides whether direct `rstest` fixture
exports are needed now or can wait. See rentaneko-design.md §§6-8.

- [ ] 1.3.1. Build the App-authenticated `octocrab` factory.
  - Requires 1.2.1.
  - See rentaneko-design.md §7.
  - Embed a test RSA key, configure `Octocrab::builder()` with the simulator
    base URI and App ID `1`, and align the `octocrab` crate version with
    Podbot's incubator dependency.
  - Success: `installation_token_with_buffer` against installation `2000`
    returns `FAKE_GITHUB_TOKEN`.
- [ ] 1.3.2. Add the narrow `OctocrabFixture` convenience type.
  - Requires 1.3.1.
  - See rentaneko-design.md §6.
  - Own both the simulator handle and client while exposing `client()` and
    `installation_id()`.
  - Success: a consumer can wrap `OctocrabFixture::start()` in an `rstest`
    fixture without Rentaneko depending on `rstest` macros in the core API.

## 2. Podbot spike: prove token acquisition feeds the atomic writer

Idea: if Podbot writes a token acquired through the Rentaneko fixture and
proves its own filesystem invariants separately, the 3.3.1 implementation can
stay small while still exercising the real Rust GitHub client boundary.

This phase belongs mostly in Podbot, but Rentaneko tracks it because it is the
acceptance consumer for the walking skeleton.

### 2.1. Wire Rentaneko into the Podbot 3.3.1 integration test

This step answers whether the fixture shape is adequate for a real consumer.
Its outcome informs whether Rentaneko needs a broader public fixture API. See
terms-of-reference.md §§5-7 and rentaneko-design.md §8.

- [ ] 2.1.1. Add a Podbot integration test that acquires the token through
  Rentaneko.
  - Requires 1.3.2.
  - See rentaneko-design.md §8 and
    <https://github.com/leynos/podbot/blob/main/docs/podbot-roadmap.md#33-refresh-tokens-through-a-host-side-daemon>.
  - Construct Podbot's `OctocrabAppClient` from the Rentaneko client and call
    `acquire_installation_token_with_client`.
  - Success: the token value observed by Podbot is `FAKE_GITHUB_TOKEN` and no
    mock `GitHubInstallationTokenClient` participates in the integration proof.
- [ ] 2.1.2. Feed the acquired token into Podbot's runtime token-file writer.
  - Requires 2.1.1 and Podbot roadmap task 3.3.1.
  - See rentaneko-design.md §§8 and 11.
  - Assert final token contents, directory mode `0700`, token-file mode `0600`,
    and removal of temporary token files.
  - Success: Podbot proves the 3.3.1 filesystem result using a token that came
    through real `octocrab`.

### 2.2. Keep atomicity proof in Podbot

This step answers whether Rentaneko is carrying the right responsibility. Its
outcome prevents simulator concerns from obscuring Podbot's filesystem
invariant. See rentaneko-design.md §11.

- [ ] 2.2.1. Add Podbot's simulator-free atomic writer test.
  - Requires Podbot roadmap task 3.3.1.
  - See rentaneko-design.md §§8 and 11.
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
credential validation path. See rentaneko-design.md §§2 and 12.

- [ ] 3.1.1. Add Simulacat Core `GET /app` compatibility for `octocrab`.
  - Requires phase 2.
  - See rentaneko-design.md §12.
  - Success: Podbot's `validate_app_credentials` can run against the simulator
    without a special bypass.

### 3.2. Support refresh-loop and retry behaviour

This step would answer whether Rentaneko can support Podbot 3.3.2 without
turning the fixture into a bespoke mock server. See rentaneko-design.md §12.

- [ ] 3.2.1. Add configurable token sequences and expiry metadata.
  - Requires phase 2 and Simulacat Core token-scenario support.
  - See rentaneko-design.md §12 and
    <https://github.com/leynos/simulacat-core/blob/main/docs/roadmap.md#95-harden-protocol-breadth-for-supported-surfaces>.
  - Success: a Podbot refresh-loop test can observe token A, then token B,
    through real `octocrab`.
- [ ] 3.2.2. Add request-log and error-injection hooks when Simulacat Core
  exposes them.
  - Requires 3.2.1.
  - See rentaneko-design.md §12.
  - Success: Podbot can assert retry count and permanent-failure behaviour
    without replacing the simulator with test-local HTTP handlers.

### 3.3. Support repository access and clone-adjacent workflows

This step would answer whether Rentaneko can prove Podbot's later
repository-preflight and clone boundaries. See terms-of-reference.md §6.2 and
rentaneko-design.md §12.

- [ ] 3.3.1. Add a GitHub App installed-on-repository scenario helper.
  - Requires phase 2 and Simulacat Core actor-aware fixture builders.
  - See rentaneko-design.md §12 and
    <https://github.com/leynos/simulacat-core/blob/main/docs/roadmap.md#13-centralize-writes-and-reusable-fixture-construction>.
  - Success: consumers can seed an installation, repository, and branch without
    hand-writing raw state.
- [ ] 3.3.2. Evaluate a Git server or Git smart HTTP fixture separately from
  Rentaneko's API simulator role.
  - Requires 3.3.1.
  - See terms-of-reference.md §6.2.
  - Success: clone end-to-end tests have a documented backend choice and do not
    force Git protocol behaviour into Simulacat Core by accident.
