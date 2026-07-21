//! Semantic error type for the Rentaneko simulator lifecycle.
//!
//! The variants map the failure modes in `rentaneko-design.md` §13 onto typed
//! cases a caller can inspect, rather than collapsing them into one stringly
//! error. Only the variants reachable from the lifecycle surface implemented so
//! far are present; `octocrab` construction (roadmap task 1.4.2) will add the
//! remaining `BaseUriRejected` and `OctocrabBuildFailed` cases.

/// A failure raised while managing the Simulacat Core simulator lifecycle.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RentanekoError {
    /// The `bun` executable could not be found or executed.
    #[error("the `bun` executable is unavailable: {source}")]
    BunUnavailable {
        /// The underlying spawn error.
        #[source]
        source: std::io::Error,
    },

    /// The runner configuration file could not be written to the owned
    /// temporary directory.
    #[error("failed to write the runner configuration to {path}: {source}")]
    ConfigWriteFailed {
        /// The configuration path that could not be written.
        path: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The runner process could not be spawned, or its standard I/O pipes were
    /// not available after spawning.
    #[error("failed to start the Simulacat Core runner: {message}")]
    RunnerSpawnFailed {
        /// A human-readable description of the spawn failure.
        message: String,
    },

    /// Readiness was not observed before the startup timeout elapsed.
    #[error("timed out waiting for Simulacat Core readiness; captured stderr: {stderr}")]
    ReadinessTimeout {
        /// Captured runner standard error, bounded for diagnostics.
        stderr: String,
    },

    /// The runner exited before it reported a readiness event.
    #[error("Simulacat Core exited before reporting readiness; captured stderr: {stderr}")]
    RunnerExitedBeforeReady {
        /// Captured runner standard error, bounded for diagnostics.
        stderr: String,
    },

    /// A readiness-shaped line was observed but could not be parsed into a valid
    /// `listening` event.
    #[error("Simulacat Core emitted a malformed readiness event: {line}")]
    MalformedReadinessEvent {
        /// The offending line, retained verbatim for diagnostics.
        line: String,
    },

    /// The runner reported a structured `error` event during startup.
    #[error("Simulacat Core reported a startup error: {message}")]
    RunnerErrorEvent {
        /// The reported error message.
        message: String,
    },

    /// Graceful shutdown or forced teardown of the owned child failed.
    #[error("Simulacat Core shutdown failed: {message}")]
    ShutdownFailed {
        /// A human-readable description of the shutdown failure.
        message: String,
    },
}
