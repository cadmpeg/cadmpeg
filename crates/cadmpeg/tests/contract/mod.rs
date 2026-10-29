// SPDX-License-Identifier: Apache-2.0
//! Source-framing, partial-part and golden data contract checks.

mod conformance;
mod golden_data;
mod partial_parts;
mod w_closure;

#[cfg(feature = "step")]
mod one_home;

#[cfg(feature = "fcstd")]
mod freecad_one_home;
