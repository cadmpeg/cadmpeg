// SPDX-License-Identifier: Apache-2.0
//! Subprocess safety and mutation sweeps for cadmpeg codecs.
//!
//! The harness runs public [`Codec`](cadmpeg_ir::Codec) operations in child
//! processes so an
//! allocator abort, a stack overflow, or an infinite loop is contained and
//! observable instead of taking down the test runner.
//!
//! # Layout
//!
//! - [`driver`] is the parent side: it spawns the [`harness-runner`] child with
//!   a hard wall-clock timeout, feeds it one deterministic input plus a policy
//!   profile, captures exit status and stderr, and reads back a machine-readable
//!   [`RunnerOutcome`](execute::RunnerOutcome).
//! - [`execute`] is the in-child work: it runs one [`Operation`] under one
//!   [`PolicyProfile`] twice and reports the classified result and a
//!   determinism digest. The `harness-runner` binary installs a peak-tracking
//!   global allocator around it.
//! - [`boundary`] names each codec's record/entry boundaries for the sweep.
//! - [`sweep`] turns a fixture plus its boundaries into truncation and
//!   single-byte mutation cases.
//! - [`fixtures`] discovers per-codec inputs from the checked-in corpora.
//! - [`oracle`] defines the subprocess checks and their resource envelopes.
//!
//! # Checks
//!
//! Every run checks for panic or abort, peak process
//! allocation within its own envelope (a separate, larger constant
//! than the budget's `K` — the process pays for IR, serde, and report memory
//! the budget never meters), a wall-clock ceiling, and decode-twice determinism
//! (identical IR JSON, report, and source-fidelity sidecar).
//!
//! # Running the full sweep
//!
//! A bounded `sweep_smoke` touches every discovered fixture through a small
//! prefix of its truncation cases so the sweep pipeline itself runs in the fast
//! test. The exhaustive truncation and mutation sweep across every discovered
//! fixture is behind an ignored test:
//!
//! ```text
//! cargo test -p cadmpeg-harness --test sweep -- --ignored --nocapture
//! ```
//!
//! Both honor `CADMPEG_HARNESS_CORPUS` (corpus root),
//! `CADMPEG_HARNESS_PEAK_BYTES` (peak-allocation envelope), and
//! `CADMPEG_HARNESS_TIMEOUT_MS` (wall-clock ceiling).

pub mod boundary;
pub mod driver;
pub mod execute;
pub mod fixtures;
pub mod oracle;
pub mod sweep;

mod model;

pub use model::{Operation, PolicyProfile, ResultClass};
