// SPDX-License-Identifier: Apache-2.0
//! Stage-1 subprocess oracle harness for cadmpeg codecs.
//!
//! The harness wraps the public [`Codec`](cadmpeg_ir::Codec) API and is what
//! makes the decode platform's failure, ownership, and budget contracts
//! falsifiable. It runs each fixture batch in a **child process** so an
//! allocator abort, a stack overflow, or an infinite loop is contained and
//! observable instead of taking down the test runner, and so a peak-allocation
//! counter is never polluted by a concurrent test.
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
//! - [`oracle`] defines the stage-1 oracles and their calibrated envelopes.
//! - [`baseline`] is the multidimensional baseline schema and the regression
//!   check the gate test runs against committed baselines.
//! - [`stage2`] resolves the §7 stage-2 capability matrix per codec from its
//!   `parser-manifest.toml`: which oracle rows gate, and the runtime predicates
//!   ([`judge_report`](stage2::CodecStage2Status::judge_report)) that judge a
//!   decode's report against the byte-accounting and no-silent-fallback rows.
//!
//! # Oracles
//!
//! Every run is judged by four stage-1 oracles: no panic or abort, peak process
//! allocation within its own calibrated envelope (a separate, larger constant
//! than the budget's `K` — the process pays for IR, serde, and report memory
//! the budget never meters), a wall-clock ceiling, and decode-twice determinism
//! (identical IR JSON, report, and losses).
//!
//! # Gating
//!
//! Baselines are keyed `codec × fixture × operation × oracle × policy-profile`
//! — never one aggregate count, which would let one oracle regress while
//! another improves. The committed baseline is a ratchet: the fast
//! [regression gate](baseline::regressions) fails only when an oracle that
//! passed now fails.
//!
//! # Running the full sweep
//!
//! The fast gate covers the curated base fixtures so `cargo test` stays quick,
//! and a bounded `sweep_smoke` touches every discovered fixture through a small
//! prefix of its truncation cases so the sweep pipeline itself runs in the fast
//! gate. The exhaustive truncation and mutation sweep across every discovered
//! fixture — the calibration source for the peak envelope and the run counts the
//! phase gates cite — is behind an ignored test that a scheduled CI job runs:
//!
//! ```text
//! cargo test -p cadmpeg-harness --test sweep -- --ignored --nocapture
//! ```
//!
//! To re-bless the committed baselines after an intended behavior change:
//!
//! ```text
//! cargo test -p cadmpeg-harness --test gate -- --ignored bless_baselines
//! ```
//!
//! Both honor `CADMPEG_HARNESS_CORPUS` (corpus root),
//! `CADMPEG_HARNESS_PEAK_BYTES` (peak-allocation envelope), and
//! `CADMPEG_HARNESS_TIMEOUT_MS` (wall-clock ceiling).

pub mod baseline;
pub mod boundary;
pub mod driver;
pub mod execute;
pub mod fixtures;
pub mod oracle;
pub mod stage2;
pub mod sweep;

mod model;

pub use model::{Operation, PolicyProfile, ResultClass, ENVELOPE_VERSION};
