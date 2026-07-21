//! Versioned runner configuration written to the owned temporary directory.
//!
//! The Rust crate and Bun runner exchange one v1 JSON document as described in
//! `rentaneko-design.md` §7. The runner reads `initialState` verbatim, binds the
//! requested loopback host on an ephemeral port, and reports the selected port
//! back over stdout.

use std::path::PathBuf;

use cap_std::{ambient_authority, fs::Dir};
use serde::Serialize;
use tempfile::TempDir;

use crate::error::RentanekoError;

/// App ID seeded into the simulator state.
pub(super) const SEED_APP_ID: u64 = 1;
/// Installation ID seeded into the simulator state.
pub(super) const SEED_INSTALLATION_ID: u64 = 2000;

const CONFIG_FILE_NAME: &str = "config.json";

/// The v1 runner configuration document.
#[derive(Debug, Serialize)]
struct RunnerConfig {
    version: u32,
    #[serde(rename = "initialState")]
    initial_state: InitialState,
    bind: Bind,
}

#[derive(Debug, Serialize)]
struct InitialState {
    users: Vec<Empty>,
    installations: Vec<Installation>,
    organizations: Vec<Organization>,
    repositories: Vec<Empty>,
    branches: Vec<Empty>,
    blobs: Vec<Empty>,
}

#[derive(Debug, Serialize)]
struct Installation {
    id: u64,
    account: String,
    app_id: u64,
}

#[derive(Debug, Serialize)]
struct Organization {
    login: String,
}

#[derive(Debug, Serialize)]
struct Bind {
    host: String,
    port: u16,
}

/// A placeholder for the intentionally empty seed collections.
///
/// The seed carries no users, repositories, branches, or blobs, but the runner
/// still expects the keys to be present as empty arrays.
#[derive(Debug, Serialize)]
struct Empty {}

impl RunnerConfig {
    /// Builds the minimal seed described in `rentaneko-design.md` §6.
    ///
    /// The single `organizations` row is the §6-sanctioned deviation: Simulacat
    /// Core requires it to shape the installation-token payload, and the 1.1.1
    /// checkpoint confirmed the seeded row keeps installation `2000` serving
    /// `FAKE_GITHUB_TOKEN`.
    fn seed() -> Self {
        Self {
            version: 1,
            initial_state: InitialState {
                users: Vec::new(),
                installations: vec![Installation {
                    id: SEED_INSTALLATION_ID,
                    account: "rentaneko".to_owned(),
                    app_id: SEED_APP_ID,
                }],
                organizations: vec![Organization {
                    login: "rentaneko".to_owned(),
                }],
                repositories: Vec::new(),
                branches: Vec::new(),
                blobs: Vec::new(),
            },
            bind: Bind {
                host: "127.0.0.1".to_owned(),
                port: 0,
            },
        }
    }
}

/// Returns the configuration path inside an owned temporary directory.
pub(super) fn config_path(dir: &TempDir) -> PathBuf { dir.path().join(CONFIG_FILE_NAME) }

/// Writes the seed configuration into a freshly created temporary directory.
///
/// The returned [`TempDir`] owns the directory; dropping it removes the file.
///
/// # Errors
///
/// Returns [`RentanekoError::ConfigWriteFailed`] if the temporary directory or
/// configuration file cannot be created.
pub(super) fn write_seed_config() -> Result<TempDir, RentanekoError> {
    let dir = tempfile::Builder::new()
        .prefix("rentaneko-simulator-")
        .tempdir()
        .map_err(|source| RentanekoError::ConfigWriteFailed {
            path: "<temporary directory>".to_owned(),
            source,
        })?;
    write_config(&dir, &RunnerConfig::seed())?;
    Ok(dir)
}

fn write_config(dir: &TempDir, config: &RunnerConfig) -> Result<(), RentanekoError> {
    let display_path = config_path(dir).display().to_string();
    let json =
        serde_json::to_vec_pretty(config).map_err(|error| RentanekoError::ConfigWriteFailed {
            path: display_path.clone(),
            source: std::io::Error::other(error),
        })?;
    // Route the write through a capability handle on the owned directory rather
    // than an ambient `std::fs` call, per the repository filesystem policy.
    let handle = Dir::open_ambient_dir(dir.path(), ambient_authority()).map_err(|source| {
        RentanekoError::ConfigWriteFailed {
            path: display_path.clone(),
            source,
        }
    })?;
    handle
        .write(CONFIG_FILE_NAME, json)
        .map_err(|source| RentanekoError::ConfigWriteFailed {
            path: display_path,
            source,
        })
}
