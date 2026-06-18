# Rentaneko prototype design

- **Status:** Draft v0.1.
- **Audience:** Rentaneko implementers, Podbot reviewers, and Simulacat Core
  maintainers.
- **Scope:** Walking skeleton for the Podbot 3.3.1 token-writer spike.
- **Companion documents:** [terms of reference](terms-of-reference.md),
  [roadmap](roadmap.md), and [repository layout](repository-layout.md).
- **Last updated:** 2026-06-18.

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
fails.

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

The Rust crate writes a JSON runner configuration into a temporary directory,
spawns the Bun runner, reads newline-delimited standard output until it sees a
`{"event":"listening","port":N}` event, and retains the child process until
fixture drop. The runner imports Simulacat Core, calls
`simulation({ initialState })`, listens on the requested or selected local
port, and closes on `SIGINT` or `SIGTERM`.

The first implementation may pre-select an available localhost port in Rust and
pass it to the runner. The more robust follow-up is to let the runner bind port
zero and report the actual port, matching Simulacat's process contract.

## 5. Simulator state contract

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

## 6. Rust API skeleton

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
but those fields remain private. `Drop` should request shutdown and then wait;
it may kill the child only when graceful shutdown does not complete.

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
library API.

## 7. Octocrab construction

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

## 8. Podbot integration proof

The integration test should make one narrow assertion chain:

1. Start `OctocrabFixture`.
2. Wrap `fixture.client().clone()` in Podbot's `OctocrabAppClient`.
3. Call `acquire_installation_token_with_client` with installation ID `2000`.
4. Pass the returned token string to Podbot's token-file writer.
5. Assert that the final token file contains `FAKE_GITHUB_TOKEN`.
6. Assert directory mode `0700`, file mode `0600`, and no published temporary
   token file.

Podbot should test atomicity separately with large old and new token values,
concurrent readers, and the invariant that every read returns either the full
old token or the full new token.

## 9. Upstream Simulacat Core dependency

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

## 10. Failure modes

| Failure                                       | Required behaviour                                                                         |
| --------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Bun is missing                                | Return or skip with a clear diagnostic naming the missing executable.                      |
| Runner exits before readiness                 | Include captured standard error and the config path in the error.                          |
| Readiness line is malformed                   | Report the malformed line without hiding preceding output.                                 |
| Simulator returns 404 for installation `2000` | Treat this as fixture setup failure, not as a Podbot token-writer failure.                 |
| `octocrab` rejects the base URI               | Return a construction error that includes the URI value.                                   |
| Child process survives graceful shutdown      | Kill only the owned child process and wait for it; do not inspect or kill other processes. |

_Table 2: Prototype failure handling._

## 11. Verification strategy

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

## 12. Deferred scope

- `GET /app` compatibility for Podbot credential validation.
- Configurable token sequences and expiry payloads for refresh-loop tests.
- Request-log capture and error injection for retry assertions.
- Repository access preflight using installation repository and branch routes.
- Git smart HTTP or a separate Git server fixture for clone end-to-end tests.
- A stabilized `rstest` fixture macro or plugin API.

## References

- [Rentaneko terms of reference](terms-of-reference.md).
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
