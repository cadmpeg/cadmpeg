// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

fn boundary_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 0,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 42,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 2,
        },
    ]);
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 42,
            directions: [1, 1],
            faces: [2, 1],
            next_edges: [11, 11],
            offset: 11,
        });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 1,
        });
    scan
}

fn boundary_circle() -> cadmpeg_ir::geometry::Curve {
    cadmpeg_ir::geometry::Curve {
        id: CurveId("creo:visibgeom:curve#11".to_string()),
        geometry: CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        },
        source_object: None,
    }
}

fn model_plane(origin: [f64; 3]) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: SurfaceId("creo:visibgeom:surface#1".to_string()),
        geometry: SurfaceGeometry::Plane {
            origin: origin.into(),
            normal: [0.0, 0.0, 1.0].into(),
            u_axis: [1.0, 0.0, 0.0].into(),
        },
        source_object: None,
    }
}

#[test]
fn model_surface_geometry_lookup_rejects_duplicate_native_ids() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));
    assert!(super::unique_model_surface_geometries(&ir).is_some());

    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.5]));
    assert!(super::unique_model_surface_geometries(&ir).is_none());
}

#[test]
fn boundary_circle_uses_native_plane_carrier_when_model_plane_is_absent() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        Some((1, Point3::new(0.0, 0.0, 0.0), [0.0, 0.0, 1.0]))
    );
}

#[test]
fn boundary_circle_uses_model_plane_carrier_when_native_plane_is_absent() {
    let mut scan = boundary_scan();
    scan.planes.positional_frames.clear();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.0]));

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        Some((1, Point3::new(0.0, 0.0, 0.0), [0.0, 0.0, 1.0]))
    );
}

#[test]
fn boundary_circle_rejects_conflicting_model_plane_carrier() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());
    ir.model.surfaces.push(model_plane([0.0, 0.0, 0.5]));

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        None
    );
}

#[test]
fn boundary_circle_rejects_duplicate_model_curves() {
    let scan = boundary_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .curves
        .extend([boundary_circle(), boundary_circle()]);

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        None
    );
}

#[test]
fn boundary_circle_rejects_duplicate_surface_rows() {
    let mut scan = boundary_scan();
    let duplicate = scan.surfaces.rows[0].clone();
    scan.surfaces.rows.push(duplicate);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.curves.push(boundary_circle());

    assert_eq!(
        super::counterbore_source_boundary_circle(&scan, &ir, 42, &[2], 1.0),
        None
    );
}

#[test]
fn radius_anchored_counterbore_accepts_signed_depth() {
    let table = crate::feature::FeatureDimensionTable {
        declared_count: 4,
        entity_ref: Some(88),
        rows: [
            (2, 0.098, 0),
            (2, 0.463_628_944_932_919_5, 1),
            (1, -0.15, 2),
            (2, 0.3125, 3),
        ]
        .into_iter()
        .map(
            |(dimension_type, value, external_id)| crate::feature::FeatureDimension {
                dimension_type,
                value: Some(value),
                value_body: Vec::new(),
                unresolved_value_token: None,
                value_unit: crate::feature::DimensionUnit::Millimeters,
                direction_byte: 0,
                auxiliary_value: Some(0.0),
                auxiliary_body: Vec::new(),
                external_id,
                references: None,
                offset: 0,
            },
        )
        .collect(),
        offset: 0,
    };

    assert_eq!(
        super::counterbore_dimension_values(std::iter::once(&table), &[0.3125]),
        Some((0.196, 0.625, 0.15))
    );
}

#[test]
fn counterbore_dimension_tuple_restricts_source_radii() {
    let dimensions = (40.0, 120.0, 8.0);

    assert!(super::counterbore_dimension_tuple_matches_radius(
        dimensions, 20.0
    ));
    assert!(super::counterbore_dimension_tuple_matches_radius(
        dimensions, 60.0
    ));
    assert!(!super::counterbore_dimension_tuple_matches_radius(
        dimensions, 24.5
    ));
}

#[test]
fn counterbore_source_patches_require_a_complete_carrier_pair() {
    let carrier = SurfaceGeometry::Cylinder {
        origin: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 0.3125,
    };
    let sources = vec![vec![10, 11], vec![30, 31]];
    let existing = BTreeMap::from([(30, carrier)]);

    assert!(
        super::counterbore_source_patch_geometries(&sources, &existing, 0.196, 0.625,).is_none()
    );
}

fn counterbore_corner_pairs() -> [[[[f64; 3]; 2]; 2]; 2] {
    [
        [
            [[-20.0, 763.0, -160.0], [20.0, 812.0, -140.0]],
            [[-20.0, 763.0, -140.0], [20.0, 812.0, -120.0]],
        ],
        [
            [[-60.0, 812.0, -200.0], [60.0, 820.0, -140.0]],
            [[-60.0, 812.0, -140.0], [60.0, 820.0, -80.0]],
        ],
    ]
}

#[test]
fn corner_envelopes_construct_dimensioned_source_cylinders() {
    let sources = vec![vec![2636, 2662], vec![2640, 2666]];
    let geometries = super::counterbore_source_corner_patch_geometries(
        &sources,
        &counterbore_corner_pairs(),
        40.0,
        120.0,
        8.0,
    )
    .expect("complete paired corner envelopes select one counterbore assignment");
    let expected = |radius| SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 820.0, -140.0),
        axis: Vector3::new(0.0, -1.0, 0.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    assert_eq!(
        geometries,
        vec![
            (2636, expected(20.0)),
            (2662, expected(20.0)),
            (2640, expected(60.0)),
            (2666, expected(60.0)),
        ]
    );
}

#[test]
fn corner_envelopes_reject_incomplete_or_inconsistent_source_joins() {
    let sources = vec![vec![2636, 2662], vec![2640, 2666]];
    let corners = counterbore_corner_pairs();
    assert!(super::counterbore_source_corner_patch_geometries(
        &sources, &corners, 40.0, 120.0, 7.0,
    )
    .is_none());

    let mut shifted = corners;
    shifted[1][0][0][0] = -59.0;
    assert!(super::counterbore_source_corner_patch_geometries(
        &sources, &shifted, 40.0, 120.0, 8.0
    )
    .is_none());

    assert!(super::counterbore_source_corner_patch_geometries(
        &[vec![2636], vec![2640, 2666]],
        &corners,
        40.0,
        120.0,
        8.0,
    )
    .is_none());
    assert!(super::counterbore_source_corner_patch_geometries(
        &[vec![2636, 2662], vec![2662, 2666]],
        &corners,
        40.0,
        120.0,
        8.0,
    )
    .is_none());
}
