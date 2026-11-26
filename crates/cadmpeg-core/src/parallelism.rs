// SPDX-License-Identifier: Apache-2.0
//! The number of worker threads one decode, measurement or digest may use.

use std::num::NonZeroUsize;
use std::sync::OnceLock;

/// Names the environment variable that caps the worker threads of one call.
/// A test runner that already runs a test per core sets it to 1.
pub const THREADS_ENV: &str = "CADMPEG_THREADS";

/// Returns the worker threads one call may use: `CADMPEG_THREADS` when it
/// holds a positive integer, else the available parallelism. Every value
/// gives the same output; 1 takes each site's serial path.
#[must_use]
pub fn parallelism() -> usize {
    static THREADS: OnceLock<usize> = OnceLock::new();
    *THREADS.get_or_init(|| {
        std::env::var(THREADS_ENV)
            .ok()
            .and_then(|value| value.parse::<NonZeroUsize>().ok())
            .or_else(|| std::thread::available_parallelism().ok())
            .map_or(1, NonZeroUsize::get)
    })
}
