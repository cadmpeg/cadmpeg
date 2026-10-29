// SPDX-License-Identifier: Apache-2.0
//! Partial-decode and golden data contract checks.

mod conformance;
mod golden_data;

#[cfg(feature = "step")]
mod one_home;

#[cfg(feature = "fcstd")]
mod freecad_one_home;
