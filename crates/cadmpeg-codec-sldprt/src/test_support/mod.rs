// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for crate tests.

#![allow(clippy::unwrap_used)]

mod container;
mod parasolid;

pub(crate) use container::*;
pub(crate) use parasolid::*;
