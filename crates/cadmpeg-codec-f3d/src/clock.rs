//! Monotonic time for decode statistics and observer timings.
//!
//! `wasm32-unknown-unknown` has no clock, and `std::time::Instant::now` panics there. On that
//! target every elapsed time is zero.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use std::time::Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl Instant {
    pub(crate) fn now() -> Self {
        Self
    }

    pub(crate) fn elapsed(self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
