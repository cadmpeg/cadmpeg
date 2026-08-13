// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic 3DM byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build archive bytes only; owner suites own the assertions.
#![allow(clippy::unwrap_used)]

mod test_archive;
pub(crate) mod test_dump;

pub(crate) use test_archive::*;
