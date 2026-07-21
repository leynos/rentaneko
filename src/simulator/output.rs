//! Bounded output capture and NDJSON readiness classification.
//!
//! The runner emits newline-delimited JSON on stdout (`rentaneko-design.md` §8).
//! [`pump_stdout`] scans it for the first classified readiness or error event,
//! reports the outcome once over a oneshot channel, and keeps draining so the
//! pipe never blocks the runner. Both streams are copied into a bounded
//! [`OutputCapture`] so diagnostics can never grow without limit.

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, BufReader},
    sync::oneshot,
};

const MAX_CAPTURED_BYTES: usize = 16 * 1024;
const TRUNCATION_MARKER: &str = "<earlier output truncated>\n";

/// The classified outcome of scanning the runner's stdout for readiness.
#[derive(Debug)]
pub(super) enum ReadinessOutcome {
    /// A valid `listening` event reporting the selected loopback port.
    Port(u16),
    /// A structured `error` event carrying the runner's message.
    ErrorEvent(String),
    /// A readiness-shaped line that could not be parsed into a valid event.
    Malformed(String),
    /// The stdout stream closed before any readiness event was observed.
    Eof,
}

/// Reads runner stdout, reporting the first readiness outcome once and draining
/// the remainder into `capture`.
pub(super) async fn pump_stdout(
    stdout: impl AsyncRead + Unpin,
    readiness_tx: oneshot::Sender<ReadinessOutcome>,
    capture: Arc<Mutex<OutputCapture>>,
) {
    let mut readiness_slot = Some(readiness_tx);
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        append(&capture, &line);
        append(&capture, "\n");
        if readiness_slot.is_some()
            && let Some(outcome) = classify_line(&line)
        {
            send_once(&mut readiness_slot, outcome);
        }
    }
    send_once(&mut readiness_slot, ReadinessOutcome::Eof);
}

/// Reads runner stderr into `capture` until the stream closes.
pub(super) async fn pump_stderr(
    mut stderr: impl AsyncRead + Unpin,
    capture: Arc<Mutex<OutputCapture>>,
) {
    let mut buffer = [0_u8; 1024];
    while let Ok(bytes_read) = stderr.read(&mut buffer).await {
        if bytes_read == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..bytes_read) else {
            break;
        };
        append(&capture, &String::from_utf8_lossy(chunk));
    }
}

/// Returns the current bounded capture contents for diagnostics.
pub(super) fn snapshot(capture: &Arc<Mutex<OutputCapture>>) -> String {
    capture.lock().map_or_else(
        |_| String::from("<capture unavailable>"),
        |guard| guard.as_str(),
    )
}

fn send_once(
    readiness_tx: &mut Option<oneshot::Sender<ReadinessOutcome>>,
    outcome: ReadinessOutcome,
) {
    if let Some(sender) = readiness_tx.take() {
        // The receiver is dropped when startup is cancelled; ignore that.
        match sender.send(outcome) {
            Ok(()) | Err(_) => {}
        }
    }
}

fn append(capture: &Arc<Mutex<OutputCapture>>, text: &str) {
    if let Ok(mut guard) = capture.lock() {
        guard.append(text);
    }
}

fn classify_line(line: &str) -> Option<ReadinessOutcome> {
    let value: Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    object.get("version").and_then(Value::as_u64)?;
    match object.get("event").and_then(Value::as_str)? {
        "listening" => Some(parse_listening_port(line).map_or_else(
            || ReadinessOutcome::Malformed(line.to_owned()),
            ReadinessOutcome::Port,
        )),
        "error" => Some(ReadinessOutcome::ErrorEvent(error_message(object))),
        _ => None,
    }
}

fn error_message(object: &Map<String, Value>) -> String {
    object
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Simulacat Core reported an error")
        .to_owned()
}

/// Parses a v1 `listening` line into its selected loopback port.
///
/// Returns `None` for noise, unclassified events, or a `listening` event whose
/// version, host, or port fails validation.
pub(super) fn parse_listening_port(line: &str) -> Option<u16> {
    let value: Value = serde_json::from_str(line).ok()?;
    let version = value.get("version").and_then(Value::as_u64)?;
    let event = value.get("event").and_then(Value::as_str)?;
    let host = value.get("host").and_then(Value::as_str)?;
    let port = value.get("port").and_then(Value::as_u64)?;
    listening_port(version, event, host, port)
}

fn listening_port(version: u64, event: &str, host: &str, port: u64) -> Option<u16> {
    if is_supported_readiness(version, event) && is_loopback_endpoint(host, port) {
        u16::try_from(port).ok()
    } else {
        None
    }
}

fn is_supported_readiness(version: u64, event: &str) -> bool {
    version == 1 && event == "listening"
}

fn is_loopback_endpoint(host: &str, port: u64) -> bool {
    // The runner must emit this literal host so the Rust handle never targets a
    // non-loopback listener.
    host == "127.0.0.1" && port != 0
}

/// A bounded diagnostic buffer that retains only the most recent output.
#[derive(Debug, Default)]
pub(super) struct OutputCapture {
    output: String,
    was_truncated: bool,
}

impl OutputCapture {
    pub(super) fn append(&mut self, output: &str) {
        self.output.push_str(output);
        self.truncate_to_capacity();
    }

    pub(super) fn as_str(&self) -> String {
        if self.was_truncated {
            format!("{TRUNCATION_MARKER}{}", self.output)
        } else {
            self.output.clone()
        }
    }

    fn truncate_to_capacity(&mut self) {
        let bytes_to_remove = self.output.len().saturating_sub(MAX_CAPTURED_BYTES);
        if bytes_to_remove == 0 {
            return;
        }
        let start = first_char_boundary_at_or_after(&self.output, bytes_to_remove);
        self.output.drain(..start);
        self.was_truncated = true;
    }
}

const fn first_char_boundary_at_or_after(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod capacity_tests {
    //! Tests that the bounded output buffer never exceeds its capacity.

    use googletest::prelude::*;

    use super::{MAX_CAPTURED_BYTES, OutputCapture, TRUNCATION_MARKER};

    #[test]
    fn retains_only_the_latest_output() -> Result<()> {
        let mut capture = OutputCapture::default();
        capture.append(&"discarded".repeat(MAX_CAPTURED_BYTES));
        capture.append("retained");

        let output = capture.as_str();
        verify_that!(output, starts_with(TRUNCATION_MARKER))?;
        verify_that!(output, ends_with("retained"))?;
        verify_that!(
            output.len(),
            le(TRUNCATION_MARKER.len() + MAX_CAPTURED_BYTES)
        )
    }
}
