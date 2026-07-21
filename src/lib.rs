//! Rentaneko: a managed Simulacat Core simulator lifecycle for Rust tests.
//!
//! The crate owns the lifecycle of a throwaway Simulacat Core process launched
//! through a private Bun runner. [`Simulator::start`] is the single lifecycle
//! authority: it writes the versioned runner configuration into an owned
//! temporary directory, spawns the runner in its own process group where the
//! platform supports it, captures standard output and standard error into
//! bounded buffers, and waits for the readiness event before returning a handle.
//!
//! Teardown is deterministic. [`Simulator::shutdown`] closes the owned stdin
//! pipe so the runner self-terminates on end-of-file, requests a graceful
//! process-group stop, waits for a bounded interval, force-kills the whole owned
//! process group when graceful shutdown fails or times out, reaps the direct
//! child, and surfaces any failure through [`RentanekoError`]. The synchronous
//! [`Drop`] path is best-effort last-resort cleanup only; it guarantees that no
//! known process-group descendant keeps running but does not provide bounded
//! graceful shutdown or asynchronous reaping.

mod error;
mod simulator;

pub use error::RentanekoError;
pub use simulator::Simulator;
