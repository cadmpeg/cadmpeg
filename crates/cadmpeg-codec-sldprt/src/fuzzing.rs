// SPDX-License-Identifier: Apache-2.0
//! Feature-gated entry points for focused parser fuzzing.
//!
//! These wrappers drive the crate-private `brep` leaf scanners named as
//! `fuzz_targets` in `parser-manifest.toml` so a declared target reaches the
//! committed path (doc section 10 Phase 2 exit gate item 4). They exist only
//! under the `fuzzing` feature and are not part of the stable API.

/// Exercises spline curve-carrier scanning over arbitrary bytes.
pub fn spline_curves(data: &[u8]) {
    let _ = crate::brep::spline::scan_curve_carriers(data);
}

/// Exercises spline surface-carrier scanning over arbitrary bytes.
pub fn spline_surfaces(data: &[u8]) {
    let _ = crate::brep::spline::scan_surface_carriers(data);
}

/// Exercises Parasolid topology-table scanning over arbitrary bytes.
pub fn topology(data: &[u8]) {
    let _ = crate::brep::topology::scan(data);
}

/// Exercises Parasolid entity-facts scanning over arbitrary bytes.
pub fn entity(data: &[u8]) {
    let _ = crate::brep::entity::scan(data);
}
