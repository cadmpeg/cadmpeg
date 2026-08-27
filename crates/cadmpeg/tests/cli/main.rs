// SPDX-License-Identifier: Apache-2.0
//! CLI integration tests.
//!
//! One binary, split by the surface each group asserts on. `support` holds
//! the fixture builders the groups share.

#![allow(clippy::unwrap_used)]

mod support;

mod convert;
mod diff;
mod inspect;
mod refusals;
mod reports;
