# Rentaneko prototype design

- **Status:** Draft v0.1.
- **Audience:** Rentaneko implementers, Podbot reviewers, and Simulacat Core
  maintainers.
- **Scope:** Walking skeleton for the Podbot 3.3.1 token-writer spike.
- **Companion documents:** [terms of reference](terms-of-reference.md),
  [ADR 001](adr-001-use-simulacat-core-for-octocrab-spike.md), [roadmap](roadmap.md),
  and [repository layout](repository-layout.md).
- **Last updated:** 2026-06-21.

## 1. Problem statement

Podbot needs to implement a host-side token-daemon runtime directory and atomic
token writer. That task does not need the full Git clone flow, but it does need
one integration proof that the token written by Podbot can originate from a real
`octocrab` GitHub App installation-token request.

Rentaneko provides that proof by starting Simulacat Core, seeding one GitHub
App installation, and returning an `octocrab::Octocrab` client configured to
call the simulator. Podbot remains responsible for filesystem permissions and
atomic rename semantics.

## 2. Goals and non-goals

### 2.1. Goals

- Manage the lifecycle of a Simulacat Core process from Rust tests.
- Seed a deterministic installation with ID `2000` and App ID `1`.
- Build an App-authenticated `octocrab::Octocrab` with its base URI set to the
  simulator.
- Prove `installation_token_with_buffer` can return Simulacat Core's fixed
  token value through Podbot's existing token-acquisition adapter.
- Keep the public surface narrow enough to revise after the first consumer test.

### 2.2. Non-goals

- `GET /app` support is not required for the 3.3.1 proof. Podbot credential
  validation already owns that endpoint, and adding it would widen the spike.
- Token refresh scheduling, retry policy, request logs, and error injection are
  deferred until Podbot 3.3.2.
- Git smart HTTP, `GIT_ASKPASS`, and repository clone verification are deferred
  until Podbot 3.4 and later clone work.
- Rentaneko does not model GitHub entities itself. Simulacat Core remains the
  state and route authority.

## 3. Prior art and source constraints

Simulacat establishes the process-fixture pattern. Its Python fixture starts a
Bun entrypoint, waits for a machine-readable listening event, constructs a
client bound to the simulator, and tears the process down even when the test
fails. The Python prior art already uses an operating-system-assigned port and
a bounded terminate-then-kill cleanup path. Rentaneko should copy those proven
parts, but it must add parent-death cleanup through runner stdin because the
searched Simulacat fixture surfaces do not provide that orphan-process guard.

Simulacat Core already exposes `simulation(args)` with `initialState` and
`apiUrl` arguments. Its current capability matrix lists an installation-token
route, installation repository routes, repository installation lookup, and
branch listing. Its REST audit records that the installation token payload is
partially scriptable because the token, expiry, permissions, and repository
selection are fixed while repositories come from the store.

Podbot already exposes the right Rust port. `OctocrabAppClient` wraps a real
`octocrab::Octocrab`, and `acquire_installation_token_with_client` accepts a
client trait. Podbot pins `octocrab` 0.51.0 and uses `rstest` 0.26.1.

GitHub documents `GET /app` as the authenticated App endpoint and
`POST /app/installations/{installation_id}/access_tokens` as the installation
token endpoint. GitHub installation access tokens expire one hour after
creation. `octocrab` 0.51.0 exposes `installation_token_with_buffer` for
acquiring a token that remains valid for at least the requested buffer.

## 4. Architecture

The prototype has three components:

| Component          | Responsibility                                                                         | Owner          |
| ------------------ | -------------------------------------------------------------------------------------- | -------------- |
| Rust fixture crate | Own process lifecycle, temporary files, readiness parsing, and `octocrab` construction | Rentaneko      |
| Bun runner         | Bridge Rust configuration into `simulation({ initialState })` and emit readiness       | Rentaneko      |
| GitHub simulator   | Own GitHub state, REST routes, and token response payloads                             | Simulacat Core |

_Table 1: Prototype component boundaries._

The Rust crate writes a versioned JSON runner configuration into a temporary
directory, spawns the Bun runner, reads newline-delimited standard output until
it sees a compatible readiness event, and retains the child process until
fixture drop. The runner imports Simulacat Core, calls
`simulation({ initialState })`, binds `127.0.0.1:0`, reports the actual local
port, and closes on `SIGINT`, `SIGTERM`, or parent-side stdin closure.

The implementation must bind port zero in the runner from the first slice.
Pre-selecting a localhost port in Rust is rejected because it introduces a
bind-release race that is easy to avoid at the only process boundary.

## 5. Fail-fast compatibility checkpoint

Before implementing the managed Bun runner, Rentaneko must prove the
load-bearing client assumption directly: `octocrab` 0.51.0 must be able to call
Simulacat Core's existing installation-token route and parse
`FAKE_GITHUB_TOKEN`.

The checkpoint may use a hand-started or throwaway Simulacat Core process. It
does not need the final Rust lifecycle handle. It must use a real
App-authenticated `octocrab::Octocrab`, installation ID `2000`, and the current
minimum simulator state from this document.

The checkpoint answers two questions before process machinery is built:

- whether Simulacat Core's token payload is compatible with
  `installation_token_with_buffer`;
- whether the route accepts the App-authenticated request under the simulator's
  current permissive authentication policy.

If either question fails, Rentaneko should stop the spike and create the
smallest Simulacat Core compatibility task rather than compensating with a
Rust-side response patch.

## 6. Simulator state contract

The minimum initial state is intentionally small:

```json
{
  "users": [],
  "installations": [
    {
      "id": 2000,
      "account": "rentaneko",
      "app_id": 1
    }
  ],
  "organizations": [],
  "repositories": [],
  "branches": [],
  "blobs": []
}
```

No repository or branch is required because Podbot 3.3.1 only consumes the
token string. Repository-scoped preflight belongs to a later slice. If
Simulacat Core starts requiring an organization row for installation payload
shape, the fixture should add `{"login": "rentaneko"}` to `organizations`
without adding broader scenario builders.

The expected token endpoint response is the current Simulacat Core default:
`FAKE_GITHUB_TOKEN` for a known installation and `404` for an unknown
installation. Rentaneko should not patch the response in Rust.

## 7. Runner configuration contract

The Rust crate and Bun runner exchange one versioned JSON document. The v1
configuration shape is:

```json
{
  "version": 1,
  "initialState": {
    "users": [],
    "installations": [
      {
        "id": 2000,
        "account": "rentaneko",
        "app_id": 1
      }
    ],
    "organizations": [],
    "repositories": [],
    "branches": [],
    "blobs": []
  },
  "bind": {
    "host": "127.0.0.1",
    "port": 0
  }
}
```

The contract rules are:

- `version` is required and must be `1` for the prototype.
- `initialState` is passed through to Simulacat Core as the authoritative
  state object.
- `bind.host` is required and must be `127.0.0.1` for the prototype.
- `bind.port` is required and must be `0`; the runner owns port selection.
- Unknown top-level fields are reserved for future versions and should be
  ignored by the runner when they do not conflict with required fields.
- Missing or invalid required fields must emit an `error` event and exit with a
  non-zero status.

## 8. Runner event contract

Runner standard output is newline-delimited JSON (NDJSON), encoded as UTF-8.
The Rust parser reads one line at a time until it observes a classified event
or the startup timeout expires.

The parser rules are:

- every classified event is a JSON object with an integer `version` field and a
  string `event` discriminant;
- non-JSON lines, JSON values that are not objects, and objects with unknown
  event names are unclassified noise;
- unclassified lines are ignored for state transitions but retained for error
  diagnostics;
- additive fields on known events are permitted;
- the parser must not require the readiness line to be the first line on
  stdout.

The v1 readiness event is:

```json
{"version":1,"event":"listening","host":"127.0.0.1","port":49152}
```

The v1 error event is:

```json
{"version":1,"event":"error","message":"Server failed to bind to a port"}
```

The `listening` event is valid only when `host` is `127.0.0.1` and `port` is a
positive integer. The Rust handle constructs the base URI from those two fields.

## 9. Rust API skeleton

The first public surface should be constructor-shaped. Consumers can wrap it in
`rstest` fixtures without forcing Rentaneko to stabilize a reusable fixture API
too early.

```rust,no_run
pub struct Simulator {
    base_uri: String,
    installation_id: u64,
}

impl Simulator {
    pub async fn start() -> Result<Self, RentanekoError>;
    pub fn base_uri(&self) -> &str;
    pub const fn installation_id(&self) -> u64;
    pub fn octocrab(&self) -> Result<octocrab::Octocrab, RentanekoError>;
}
```

The implementation should also own the child process and temporary directory,
but those fields remain private. `Simulator::start` is the single lifecycle
authority. Convenience types may compose `Simulator`, but they must not add a
second process-start or teardown path.

`Drop` is synchronous, so it must not depend on `.await`. It should close the
parent-side stdin pipe, send graceful termination when the process is still
running, wait for a bounded interval, then kill and wait for a shorter bounded
interval if the child remains alive. The drop path is best effort and should
not inspect or kill unrelated processes.

An optional convenience wrapper can own the simulator and client together:

```rust,no_run
pub struct OctocrabFixture {
    simulator: Simulator,
    client: octocrab::Octocrab,
}

impl OctocrabFixture {
    pub async fn start() -> Result<Self, RentanekoError>;
    pub fn client(&self) -> &octocrab::Octocrab;
    pub const fn installation_id(&self) -> u64;
}
```

This type is `rstest`-friendly without depending on `rstest` macros inside the
library API. For the 3.3.1 state, one simulator instance can be shared across
multiple test cases because the seeded installation state is read-only. The
recommended consumer fixture scope is therefore module or package scope when
all cases use the same state, and function scope only when a later test mutates
simulator state.

## 10. Octocrab construction

Rentaneko should align its `octocrab` dependency with Podbot for the incubator
phase. Returning an `octocrab::Octocrab` value across crate boundaries only
works cleanly when the consumer and fixture resolve the same major and minor
crate version. Podbot currently uses `octocrab` 0.51.0.

The fixture should include a static test RSA key. This key is test material,
not a credential. The client factory should:

- parse the embedded private key into `jsonwebtoken::EncodingKey`;
- call `Octocrab::builder().base_uri(simulator_base_uri)?.app(AppId(1), key)`;
- build the client inside an active Tokio runtime;
- return a semantic `RentanekoError` if key parsing, base URI configuration, or
  client construction fails.

The design deliberately avoids `validate_app_credentials` for the first proof.
That Podbot path calls `GET /app`; the token-writer task only needs the
installation-token route.

## 11. Podbot integration proof

The integration test should make one narrow assertion chain:

1. Start `OctocrabFixture`.
2. Wrap `fixture.client().clone()` in Podbot's `OctocrabAppClient`.
3. Call `acquire_installation_token_with_client` with installation ID `2000`.
4. Pass the returned token string to Podbot's token-file writer.
5. Assert that the final token file contains `FAKE_GITHUB_TOKEN`.

Podbot should test atomicity separately with large old and new token values,
concurrent readers, and the invariant that every read returns either the full
old token or the full new token.

## 12. Upstream Simulacat Core dependency

No new Simulacat Core runtime feature is required if real `octocrab` can
consume the existing installation-token response. The one recommended upstream
item is a contract test:

> Add an Octocrab installation-token contract test that starts Simulacat Core
> with a deterministic installation, sends a real App-authenticated
> `installation_token_with_buffer` request, and confirms the client receives
> `FAKE_GITHUB_TOKEN`.

This item belongs beside Simulacat Core roadmap step 1.4 because that step
already asks whether generated REST payloads can be consumed by real GitHub
client libraries. It should not depend on later permission and token scenario
work in step 9.5.2.

Rentaneko still needs its own drift tripwire. A Rentaneko integration test must
start the packaged runner, perform the real `octocrab` token request, and assert
`FAKE_GITHUB_TOKEN`. That test fails loudly when Simulacat Core's
`simulation()` API, token route, payload shape, or permissive authentication
behaviour drifts underneath Rentaneko.

## 13. Failure modes and errors

| Failure                                       | Required behaviour                                                                         |
| --------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Bun is missing                                | Return or skip with a clear diagnostic naming the missing executable.                      |
| Runner exits before readiness                 | Include captured standard error and the config path in the error.                          |
| Readiness event is malformed                  | Report the malformed event without hiding preceding output.                                |
| Simulator returns 404 for installation `2000` | Treat this as fixture setup failure, not as a Podbot token-writer failure.                 |
| `octocrab` rejects the base URI               | Return a construction error that includes the URI value.                                   |
| Child process survives graceful shutdown      | Kill only the owned child process and wait for it; do not inspect or kill other processes. |
| Parent process exits normally                 | Runner observes stdin closure and self-terminates without requiring an external sweeper.   |

_Table 2: Prototype failure handling._

The semantic error enum should map these cases directly instead of collapsing
them into one stringly failure. The initial `RentanekoError` variants should
include:

- `BunUnavailable`;
- `ConfigWriteFailed`;
- `RunnerSpawnFailed`;
- `ReadinessTimeout`;
- `RunnerExitedBeforeReady`;
- `MalformedReadinessEvent`;
- `RunnerErrorEvent`;
- `BaseUriRejected`;
- `OctocrabBuildFailed`;
- `InstallationTokenUnavailable`;
- `ShutdownFailed`.

## 14. Verification strategy

The design has two specific correctness properties:

- **Client-boundary property:** a token used by the Podbot integration test must
  pass through a real `octocrab` installation-token request against the local
  simulator. A mock Podbot trait is not sufficient for the Rentaneko proof.
- **Atomic-writer separation property:** Rentaneko must not claim to prove
  filesystem atomicity. Podbot proves that property with direct filesystem
  tests because Podbot owns the writer.

The first property is verified by the Rentaneko-backed Podbot integration test.
The second is verified by keeping the Podbot writer test independent from the
simulator and by documenting the separation in this design.

Rentaneko also needs a contract-boundary property: the Bun runner and Rust
handle must agree on the v1 configuration and NDJSON readiness contracts. This
is verified by parser tests for classified, unclassified, and malformed lines,
plus a runner integration test that binds port zero and reports the actual port.

## 15. Deferred scope

- `GET /app` compatibility for Podbot credential validation.
- Configurable token sequences and expiry payloads for refresh-loop tests.
- Request-log capture and error injection for retry assertions.
- Repository access preflight using installation repository and branch routes.
- Git smart HTTP or a separate Git server fixture for clone end-to-end tests.
- A stabilized `rstest` fixture macro or plugin API.

## References

- [Rentaneko terms of reference](terms-of-reference.md).
- [ADR 001: Use Simulacat Core for the Octocrab spike](adr-001-use-simulacat-core-for-octocrab-spike.md).
- Podbot design:
  <https://github.com/leynos/podbot/blob/main/docs/podbot-design.md>.
- Podbot roadmap:
  <https://github.com/leynos/podbot/blob/main/docs/podbot-roadmap.md>.
- Simulacat Core API reference:
  <https://github.com/leynos/simulacat-core/blob/main/docs/api-reference.md>.
- Simulacat Core REST audit:
  <https://github.com/leynos/simulacat-core/blob/main/docs/github-rest-api-audit.md>.
- Simulacat Core roadmap:
  <https://github.com/leynos/simulacat-core/blob/main/docs/roadmap.md>.
- Simulacat design:
  <https://github.com/leynos/simulacat/blob/main/docs/simulacat-design.md>.
- GitHub REST API endpoints for GitHub Apps:
  <https://docs.github.com/en/rest/apps/apps?apiVersion=2022-11-28>.
- `octocrab` 0.51.0 `installation_token_with_buffer` documentation:
  <https://docs.rs/octocrab/0.51.0/octocrab/struct.Octocrab.html#method.installation_token_with_buffer>.
- `rstest` crate documentation: <https://docs.rs/rstest/latest/rstest/>.
