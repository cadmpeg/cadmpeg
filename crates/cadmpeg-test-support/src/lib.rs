// SPDX-License-Identifier: Apache-2.0
//! Test-only helpers shared by cadmpeg codec crates.
//!
//! This crate is `publish = false`. Production crates must not depend on it.

pub mod golden;
pub mod roundtrip;
