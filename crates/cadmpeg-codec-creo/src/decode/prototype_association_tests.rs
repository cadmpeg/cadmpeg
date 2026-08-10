use super::*;

fn row(offset: usize, id: u32, kind: crate::surface::SurfaceKind) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 1,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset,
    }
}

#[test]
fn prototype_uses_the_preceding_same_family_row() {
    let rows = [
        row(100, 10, crate::surface::SurfaceKind::Plane),
        row(200, 20, crate::surface::SurfaceKind::Plane),
    ];

    assert_eq!(
        first_instance_surface_row(&rows, 100, 300, 150, crate::surface::SurfaceKind::Plane)
            .map(|row| row.id),
        Some(10)
    );
}

#[test]
fn prototype_before_frame_rows_uses_the_following_same_family_row() {
    let rows = [row(100, 10, crate::surface::SurfaceKind::Plane)];

    assert_eq!(
        first_instance_surface_row(&rows, 100, 300, 50, crate::surface::SurfaceKind::Plane)
            .map(|row| row.id),
        Some(10)
    );
}

#[test]
fn prototype_after_a_different_family_uses_the_following_family_row() {
    let rows = [
        row(100, 10, crate::surface::SurfaceKind::Cylinder),
        row(200, 20, crate::surface::SurfaceKind::Plane),
    ];

    assert_eq!(
        first_instance_surface_row(&rows, 100, 300, 150, crate::surface::SurfaceKind::Plane),
        Some(&rows[1])
    );
}
