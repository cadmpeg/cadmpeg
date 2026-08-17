// SPDX-License-Identifier: Apache-2.0

use super::unique_feature_surface_row;

fn surface_row(
    id: u32,
    feature_id: u32,
    kind: crate::surface::SurfaceKind,
) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    }
}

#[test]
fn generated_surface_binding_requires_one_matching_row() {
    let row = surface_row(31, 7, crate::surface::SurfaceKind::Plane);
    assert!(unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        8,
        crate::surface::SurfaceKind::Plane,
    ));
    assert!(!unique_feature_surface_row(
        std::slice::from_ref(&row),
        31,
        7,
        crate::surface::SurfaceKind::Cylinder,
    ));
    assert!(!unique_feature_surface_row(
        &[row.clone(), row],
        31,
        7,
        crate::surface::SurfaceKind::Plane,
    ));
}
