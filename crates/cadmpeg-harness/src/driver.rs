// SPDX-License-Identifier: Apache-2.0
//! The parent side: spawn the runner child, enforce a hard timeout, capture
//! its status and output, and apply the stage-1 oracles.
//!
//! Isolation lives here. The child may abort, overflow its stack, or loop
//! forever; the parent observes each of those as an exit status or a timeout
//! kill rather than a crashed or hung test process.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::execute::{ReportSummary, RunnerOutcome};
use crate::model::{Operation, PolicyProfile};
use crate::oracle::{Oracle, OracleLimits, OracleStatus};

/// The identity of one run: the full baseline key minus the oracle dimension.
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

    /// The `codec|fixture|operation|profile` string used as the baseline key.
    pub fn joined(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.codec, self.fixture, self.operation, self.profile
        )
    }
}

/// The parent's judgment of one run.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Identity of the run.
    pub key: RunKey,
    /// Each oracle's verdict.
    pub oracles: BTreeMap<Oracle, OracleStatus>,
    /// Classified result label, when the child reported one.
    pub result_class: Option<String>,
    /// Peak process allocation the child measured, when it reported an outcome.
    /// Retained as a measured value beside the [`Oracle::PeakAlloc`] pass/fail
    /// verdict so a large regression that stays inside the envelope is still
    /// visible to the performance ratchet (doc §10 Phase 2 performance gate).
    pub peak_alloc_bytes: Option<u64>,
    /// The child's decode-report summary, when the operation ran a successful
    /// decode. The §7 stage-2 report oracles judge this.
    pub report: Option<ReportSummary>,
    /// Wall-clock time the parent measured.
    pub elapsed: Duration,
    /// Whether the parent killed the child at the ceiling.
    pub timed_out: bool,
    /// Captured child stderr, for diagnosing a broken run.
    pub stderr: String,
}

impl RunResult {
    /// Whether every oracle passed.
    pub fn all_pass(&self) -> bool {
        self.oracles.values().all(|s| *s == OracleStatus::Pass)
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

/// Run one job in a child process and judge it against the oracles.
///
/// `runner` is the `harness-runner` executable path (an integration test passes
/// `env!("CARGO_BIN_EXE_harness-runner")`). `bytes` is the exact — possibly
/// truncated or mutated — input; it is streamed to the child's stdin.
pub fn run_job(
    runner: &Path,
    key: RunKey,
    bytes: &[u8],
    limits: &OracleLimits,
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
            Err(_) => break None,
        }
    };
    let elapsed = start.elapsed();

    let _ = writer.join();
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();

    let outcome: Option<RunnerOutcome> = if timed_out {
        None
    } else {
        serde_json::from_slice(&stdout).ok()
    };

    let mut oracles = BTreeMap::new();

    // No panic/abort: a normal `Err(CodecError)` still exits 0 with an outcome;
    // only a panic, an abort, or a signal produces a broken exit. A timeout is
    // the wall-clock oracle's business, not this one.
    let no_panic = if timed_out {
        OracleStatus::Unevaluated
    } else if status.is_some_and(|s| s.success()) && outcome.is_some() {
        OracleStatus::Pass
    } else {
        OracleStatus::Fail
    };
    oracles.insert(Oracle::NoPanic, no_panic);

    let wall_clock = if timed_out || elapsed > limits.wall_clock {
        OracleStatus::Fail
    } else {
        OracleStatus::Pass
    };
    oracles.insert(Oracle::WallClock, wall_clock);

    let peak_alloc = match &outcome {
        Some(outcome) if outcome.peak_alloc_bytes <= limits.peak_envelope_bytes => {
            OracleStatus::Pass
        }
        Some(_) => OracleStatus::Fail,
        None => OracleStatus::Unevaluated,
    };
    oracles.insert(Oracle::PeakAlloc, peak_alloc);

    let determinism = match &outcome {
        Some(outcome) if outcome.determinism_ok => OracleStatus::Pass,
        Some(_) => OracleStatus::Fail,
        None => OracleStatus::Unevaluated,
    };
    oracles.insert(Oracle::Determinism, determinism);

    Ok(RunResult {
        key,
        oracles,
        result_class: outcome.as_ref().map(|o| o.result_class.clone()),
        peak_alloc_bytes: outcome.as_ref().map(|o| o.peak_alloc_bytes),
        report: outcome.as_ref().and_then(|o| o.report.clone()),
        elapsed,
        timed_out,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}
