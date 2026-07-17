// SPDX-License-Identifier: Apache-2.0
//! The isolated child process.
//!
//! Reads one job from `argv` (`<codec> <operation> <profile>`) and the input
//! bytes from stdin, runs the operation twice under a peak-tracking global
//! allocator, and writes a [`RunnerOutcome`](cadmpeg_harness::execute::RunnerOutcome)
//! as JSON to stdout. A panic or abort here is intended to surface to the parent
//! as a non-zero exit; nothing is caught.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use cadmpeg_harness::execute::{execute, RunnerOutcome};
use cadmpeg_harness::{Operation, PolicyProfile};

/// Live bytes currently allocated process-wide.
static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// The high-water mark of [`CURRENT`].
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// A `System`-backed allocator that records the process's peak live heap.
///
/// The peak is meaningful only because each job runs in its own process; a
/// shared-process counter would be polluted by concurrent work.
struct PeakAlloc;

unsafe impl GlobalAlloc for PeakAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }
}

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(codec), Some(operation), Some(profile)) = (args.next(), args.next(), args.next())
    else {
        eprintln!("usage: harness-runner <codec> <operation> <profile>  (input on stdin)");
        return ExitCode::from(2);
    };

    let Some(operation) = Operation::from_id(&operation) else {
        eprintln!("unknown operation {operation}");
        return ExitCode::from(2);
    };
    let Some(profile) = PolicyProfile::from_id(&profile) else {
        eprintln!("unknown profile {profile}");
        return ExitCode::from(2);
    };

    let mut bytes = Vec::new();
    if let Err(error) = std::io::stdin().lock().read_to_end(&mut bytes) {
        eprintln!("failed reading stdin: {error}");
        return ExitCode::from(2);
    }

    let exec = execute(&codec, operation, profile, &bytes);
    // Read the peak immediately: `execute` has dropped the decode's large
    // allocations, so this is the operation's high-water mark. The small
    // serialization that follows is deliberately excluded.
    let peak_alloc_bytes = PEAK.load(Ordering::Relaxed) as u64;

    let outcome = RunnerOutcome {
        result_class: exec.result_class.label().to_owned(),
        determinism_ok: exec.determinism_ok,
        peak_alloc_bytes,
        report: exec.report,
    };

    match serde_json::to_string(&outcome) {
        Ok(json) => {
            let mut stdout = std::io::stdout().lock();
            if writeln!(stdout, "{json}").is_err() {
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("failed serializing outcome: {error}");
            ExitCode::from(2)
        }
    }
}
