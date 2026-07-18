// SPDX-License-Identifier: Apache-2.0
//! Feature-gated entry points for focused parser fuzzing.

pub fn spline_curves(data: &[u8]) {
    let _ = crate::brep::spline::scan_curve_carriers(data);
}

pub fn spline_surfaces(data: &[u8]) {
    let _ = crate::brep::spline::scan_surface_carriers(data);
}

pub fn topology(data: &[u8]) {
    let _ = crate::brep::topology::scan(data);
}

pub fn entity(data: &[u8]) {
    let _ = crate::brep::entity::scan(data);
}
