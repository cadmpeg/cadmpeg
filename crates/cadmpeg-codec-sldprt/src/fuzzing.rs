// SPDX-License-Identifier: Apache-2.0
//! Feature-gated entry points for focused parser fuzzing.
//!
//! Each wrapper feeds arbitrary bytes to one crate-private `brep` leaf scanner
//! and discards the result; the contract is that no input may panic. The facade
//! keeps those scanners reachable from the fuzz harness without widening the
//! stable API, and is gated behind the `fuzzing` feature.

/// Exercise spline curve-carrier scanning.
pub fn spline_curves(data: &[u8]) {
    let _ = crate::brep::spline::scan_curve_carriers(data);
}

/// Exercise spline surface-carrier scanning.
pub fn spline_surfaces(data: &[u8]) {
    let _ = crate::brep::spline::scan_surface_carriers(data);
}

/// Exercise `brep` topology table scanning.
pub fn topology(data: &[u8]) {
    let _ = crate::brep::topology::scan(data);
}

/// Exercise `brep` entity-record scanning.
pub fn entity(data: &[u8]) {
    let _ = crate::brep::entity::scan(data);
}
