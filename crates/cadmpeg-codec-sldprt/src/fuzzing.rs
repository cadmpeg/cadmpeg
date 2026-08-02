// SPDX-License-Identifier: Apache-2.0
//! Feature-gated entry points for focused parser fuzzing.

/// Exercise spline-curve carrier scanning.
pub fn spline_curves(data: &[u8]) {
    let _ = crate::brep::spline::scan_curve_carriers(data);
}

/// Exercise spline-surface carrier scanning.
pub fn spline_surfaces(data: &[u8]) {
    let _ = crate::brep::spline::scan_surface_carriers(data);
}

/// Exercise topology record scanning.
pub fn topology(data: &[u8]) {
    let _ = crate::brep::topology::scan(data);
}

/// Exercise entity record scanning.
pub fn entity(data: &[u8]) {
    let _ = crate::brep::entity::scan(data);
}
