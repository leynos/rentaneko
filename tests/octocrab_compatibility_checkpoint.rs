//! Opt-in compatibility checkpoint for real Octocrab against Simulacat Core.

mod checkpoint_support;

use std::io;

use chrono::Duration;
use http::header::CONTENT_TYPE;
use jsonwebtoken::EncodingKey;
use octocrab::{
    Octocrab,
    models::{AppId, InstallationId},
};
use pretty_assertions::assert_eq;
use rstest::{fixture, rstest};
use rstest_bdd_macros::{given, scenario, then, when};
use secrecy::ExposeSecret;
use uselesskey::{Factory, RsaFactoryExt, RsaSpec};

const APP_ID: u64 = 1;

#[derive(Default)]
struct CheckpointState {
    server: Option<checkpoint_support::ThrowawayServerGuard>,
    client: Option<Octocrab>,
    requested_installation_id: Option<u64>,
    token_result: Option<Result<String, octocrab::Error>>,
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl CheckpointState {
    async fn shutdown(self) -> Result<(), BoxError> {
        if let Some(server) = self.server {
            server.shutdown().await?;
        }
        Ok(())
    }
}

#[fixture]
fn checkpoint_state() -> CheckpointState {
    // `rstest-bdd` currently lint-warns on a single-expression fixture body, so
    // keep the explicit empty state instead of delegating to `Default`.
    CheckpointState {
        server: None,
        client: None,
        requested_installation_id: None,
        token_result: None,
    }
}

#[rstest]
#[case::valid_listening_line(
    r#"{"version":1,"event":"listening","host":"127.0.0.1","port":49213}"#,
    Some(49213)
)]
#[case::ignores_error_events(
    r#"{"version":1,"event":"error","host":"127.0.0.1","port":49213}"#,
    None
)]
#[case::ignores_non_json_noise("Simulacat Core started", None)]
#[case::requires_loopback_host(
    r#"{"version":1,"event":"listening","host":"0.0.0.0","port":49213}"#,
    None
)]
#[case::requires_port(r#"{"version":1,"event":"listening","host":"127.0.0.1"}"#, None)]
#[case::rejects_oversized_port(
    r#"{"version":1,"event":"listening","host":"127.0.0.1","port":70000}"#,
    None
)]
fn parse_listening_port_returns_expected_port(
    #[case] line: &str,
    #[case] expected_port: Option<u16>,
) {
    assert_eq!(
        checkpoint_support::parse_listening_port(line),
        expected_port
    );
}

#[given("a throwaway Simulacat Core seeded with installation 2000 for app 1")]
async fn seeded_throwaway_simulacat_core(
    checkpoint_state: &mut CheckpointState,
) -> Result<(), BoxError> {
    checkpoint_state.server = Some(checkpoint_support::start_throwaway_server().await?);
    Ok(())
}

#[given("an App-authenticated octocrab client pointed at the simulator")]
fn app_authenticated_octocrab_client(
    checkpoint_state: &mut CheckpointState,
) -> Result<(), BoxError> {
    let Some(server) = checkpoint_state.server.as_ref() else {
        return Err(boxed_error("throwaway Simulacat Core was not started"));
    };
    checkpoint_state.client = Some(build_app_client(server.base_uri())?);
    Ok(())
}

#[when("the client requests an installation token for installation {installation_id:u64}")]
async fn request_installation_token_for(
    checkpoint_state: &mut CheckpointState,
    installation_id: u64,
) -> Result<(), BoxError> {
    request_installation_token(checkpoint_state, installation_id).await
}

#[then("the token equals {expected_token:string}")]
fn token_equals(
    checkpoint_state: &CheckpointState,
    expected_token: String,
) -> Result<(), BoxError> {
    let Some(token_result) = checkpoint_state.token_result.as_ref() else {
        return Err(boxed_error("installation token was not requested"));
    };
    let actual_token = match token_result {
        Ok(actual_token) => actual_token,
        Err(error) => {
            return Err(boxed_error(format!(
                "installation token request failed: {error}"
            )));
        }
    };
    assert_eq!(actual_token, &expected_token);
    Ok(())
}

#[then("octocrab reports that installation 9999 is unknown")]
fn octocrab_reports_unknown_installation(
    checkpoint_state: &CheckpointState,
) -> Result<(), BoxError> {
    assert_eq!(checkpoint_state.requested_installation_id, Some(9999));
    let Some(token_result) = checkpoint_state.token_result.as_ref() else {
        return Err(boxed_error("installation token was not requested"));
    };

    let Err(error) = token_result else {
        return Err(boxed_error(
            "unknown installation unexpectedly returned a token",
        ));
    };
    let octocrab::Error::GitHub { source, .. } = error else {
        return Err(boxed_error(format!(
            "unknown installation returned an unexpected Octocrab error: {error}"
        )));
    };
    assert_eq!(source.status_code.as_u16(), 404);
    assert_eq!(source.message, "Not Found");
    Ok(())
}

#[scenario(
    path = "tests/features/octocrab_compatibility.feature",
    name = "Acquire an installation token from a throwaway Simulacat Core"
)]
#[ignore = "requires Bun and Simulacat Core; run with --run-ignored"]
#[tokio::test(flavor = "current_thread")]
async fn octocrab_compatibility_acquires_token(
    checkpoint_state: CheckpointState,
) -> Result<(), BoxError> {
    checkpoint_state.shutdown().await
}

#[scenario(
    path = "tests/features/octocrab_compatibility.feature",
    name = "An unknown installation is rejected"
)]
#[ignore = "requires Bun and Simulacat Core; run with --run-ignored"]
#[tokio::test(flavor = "current_thread")]
async fn octocrab_compatibility_rejects_unknown_installation(
    checkpoint_state: CheckpointState,
) -> Result<(), BoxError> {
    checkpoint_state.shutdown().await
}

async fn request_installation_token(
    checkpoint_state: &mut CheckpointState,
    installation_id: u64,
) -> Result<(), BoxError> {
    checkpoint_state.requested_installation_id = Some(installation_id);
    let Some(client) = checkpoint_state.client.as_ref() else {
        return Err(boxed_error(
            "App-authenticated octocrab client was not configured",
        ));
    };
    let token_result = async {
        let installation_client = client.installation(InstallationId(installation_id))?;
        let token = installation_client
            .installation_token_with_buffer(Duration::seconds(60))
            .await?;
        Ok::<String, octocrab::Error>(token.expose_secret().to_owned())
    }
    .await;
    checkpoint_state.token_result = Some(token_result);
    Ok(())
}

fn build_app_client(base_uri: &str) -> Result<Octocrab, BoxError> {
    let key = runtime_signing_key()?;
    Ok(Octocrab::builder()
        .base_uri(base_uri)?
        .add_header(CONTENT_TYPE, "application/json".to_owned())
        .app(AppId(APP_ID), key)
        .build()?)
}

fn runtime_signing_key() -> Result<EncodingKey, BoxError> {
    let signing_key = Factory::random().rsa("octocrab-compatibility", RsaSpec::rs256());
    Ok(EncodingKey::from_rsa_pem(
        signing_key.private_key_pkcs8_pem().as_bytes(),
    )?)
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_generated_rsa_key_builds_app_client() -> Result<(), BoxError> {
    build_app_client("http://127.0.0.1:65535")?;
    Ok(())
}

fn boxed_error(message: impl Into<String>) -> BoxError {
    Box::new(io::Error::other(message.into()))
}
