// SPDX-License-Identifier: Apache-2.0
//! The parent side: spawn the runner child, enforce a hard timeout, capture
//! its status and output, and apply the subprocess checks.
//!
//! Isolation lives here. The child may abort, overflow its stack, or loop
//! forever; the parent observes each of those as an exit status or a timeout
//! kill rather than a crashed or hung test process.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::execute::RunnerOutcome;
use crate::limits::RunLimits;
use crate::model::{Operation, PolicyProfile};

/// The identity of one subprocess run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunKey {
    /// Codec id.
    pub codec: String,
    /// Corpus-relative fixture path.
    pub fixture: String,
    /// Operation label.
    pub operation: String,
    /// Policy-profile label.
    pub profile: String,
}

impl RunKey {
    /// Build a key from typed dimensions.
    pub fn new(codec: &str, fixture: &str, op: Operation, profile: PolicyProfile) -> RunKey {
        RunKey {
            codec: codec.to_owned(),
            fixture: fixture.to_owned(),
            operation: op.id().to_owned(),
            profile: profile.id().to_owned(),
        }
    }

    /// A compact display key.
    pub fn joined(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.codec, self.fixture, self.operation, self.profile
        )
    }
}

/// The measured result of one run.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Identity of the run.
    pub key: RunKey,
    /// Whether the child exited successfully with a valid outcome.
    pub exited_cleanly: bool,
    /// Whether peak allocation was measured and stayed within its limit.
    pub peak_within_limit: Option<bool>,
    /// Whether elapsed time stayed within its limit.
    pub completed_in_time: bool,
    /// Whether two executions produced identical results, when measured.
    pub deterministic: Option<bool>,
    /// Classified result label, when the child reported one.
    pub result_class: Option<String>,
    /// Peak process allocation the child measured, when it reported an outcome.
    pub peak_alloc_bytes: Option<u64>,
    /// Wall-clock time the parent measured.
    pub elapsed: Duration,
    /// Whether the parent killed the child at the ceiling.
    pub timed_out: bool,
    /// Captured child stderr, for diagnosing a broken run.
    pub stderr: String,
}

impl RunResult {
    /// Whether every check passed.
    pub fn all_pass(&self) -> bool {
        self.exited_cleanly
            && self.peak_within_limit == Some(true)
            && self.completed_in_time
            && self.deterministic == Some(true)
    }

    /// Labels for failed or unavailable checks.
    pub fn failures(&self) -> Vec<&'static str> {
        let mut failures = Vec::new();
        if !self.exited_cleanly {
            failures.push("exit");
        }
        if self.peak_within_limit != Some(true) {
            failures.push("peak_alloc");
        }
        if !self.completed_in_time {
            failures.push("wall_clock");
        }
        if self.deterministic != Some(true) {
            failures.push("determinism");
        }
        failures
    }
}

/// Read a child pipe to end on its own thread, so the parent never deadlocks
/// waiting on a full pipe while it is trying to write another.
fn drain<R: Read + Send + 'static>(mut reader: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(reader) = reader.as_mut() {
            let _ = reader.read_to_end(&mut buf);
        }
        buf
    })
}

/// Run one job in a child process and apply the resource and repeatability checks.
///
/// `runner` is the `harness-runner` executable path (an integration test passes
/// `env!("CARGO_BIN_EXE_harness-runner")`). `bytes` is the exact — possibly
/// truncated or mutated — input; it is streamed to the child's stdin.
pub fn run_job(
    runner: &Path,
    key: RunKey,
    bytes: &[u8],
    limits: &RunLimits,
) -> std::io::Result<RunResult> {
    let mut child = Command::new(runner)
        .arg(&key.codec)
        .arg(&key.operation)
        .arg(&key.profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdin = child.stdin.take();
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());

    let input = bytes.to_vec();
    let writer = thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(&input);
            // Dropping `stdin` closes the pipe, signalling EOF to the child.
        }
    });

    let start = Instant::now();
    let mut timed_out = false;
    let mut wait_error = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= limits.wall_clock {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                wait_error = Some(error);
                break None;
            }
        }
    };
    let elapsed = start.elapsed();

    let _ = writer.join();
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    if let Some(error) = wait_error {
        return Err(error);
    }

    let outcome: Option<RunnerOutcome> = if timed_out {
        None
    } else {
        serde_json::from_slice(&stdout).ok()
    };

    // No panic/abort: a normal `Err(CodecError)` still exits 0 with an outcome;
    // only a panic, an abort, or a signal produces a broken exit. A timeout is
    // the wall-clock check's business, not this one.
    let exited_cleanly = !timed_out && status.is_some_and(|s| s.success()) && outcome.is_some();

    let completed_in_time = !timed_out && elapsed <= limits.wall_clock;

    let peak_within_limit = outcome
        .as_ref()
        .map(|outcome| outcome.peak_alloc_bytes <= limits.peak_bytes);

    let deterministic = outcome.as_ref().map(|outcome| outcome.determinism_ok);

    Ok(RunResult {
        key,
        exited_cleanly,
        peak_within_limit,
        completed_in_time,
        deterministic,
        result_class: outcome.as_ref().map(|o| o.result_class.clone()),
        peak_alloc_bytes: outcome.as_ref().map(|o| o.peak_alloc_bytes),
        elapsed,
        timed_out,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}
