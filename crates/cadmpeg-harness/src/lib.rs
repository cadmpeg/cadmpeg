// SPDX-License-Identifier: Apache-2.0
//! Subprocess safety and mutation sweeps for cadmpeg codecs.
//!
//! Public codec operations run in child processes with allocation and time
//! limits. Each operation runs twice to check deterministic output.
//!
//! # Running the full sweep
//!
//! ```text
//! cargo test -p cadmpeg-harness --test sweep -- --ignored --nocapture
//! ```
//!
//! The sweep honors `CADMPEG_HARNESS_CORPUS` (corpus root),
//! `CADMPEG_HARNESS_PEAK_BYTES` (peak-allocation envelope), and
//! `CADMPEG_HARNESS_TIMEOUT_MS` (wall-clock ceiling).

pub mod boundary;
pub mod driver;
pub mod execute;
pub mod fixtures;
pub mod limits;
pub mod sweep;

mod model;

pub use model::{Operation, PolicyProfile, ResultClass};
