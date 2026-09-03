// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::equations::{CarrierEquation, PlaneEquation};
use super::{existing_plane_agrees_with_topology, placed_carriers, transfer_topology_bound_planes};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

fn carrier_surface(id: u32, geometry: SurfaceGeometry) -> Surface {
    Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry,
        source_object: None,
    }
}

fn cylinder_surface(id: u32, radius: f64) -> Surface {
    carrier_surface(
        id,
        SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        },
    )
}

fn carrier_row(id: u32, kind: crate::surface::SurfaceKind) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 1,
        reversed: false,
        boundary_type: 1,
        next_surface: 0,
        offset: 0,
    }
}

fn topology_plane() -> PlaneEquation {
    PlaneEquation {
        origin: [0.0, 0.0, 4.0],
        normal: [0.0, 0.0, 1.0],
    }
}

#[test]
fn existing_plane_carrier_accepts_reversed_normal() {
    let existing = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 4.0),
        normal: Vector3::new(0.0, 0.0, -1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        Some(true)
    );
}

#[test]
fn existing_plane_carrier_rejects_offset_conflict() {
    let existing = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 5.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        Some(false)
    );
}

#[test]
fn existing_unknown_carrier_does_not_compete_with_topology() {
    let existing = SurfaceGeometry::Unknown { record: None };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        None
    );
}

#[test]
fn existing_non_plane_carrier_conflicts_with_topology() {
    let existing = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 4.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };

    assert_eq!(
        existing_plane_agrees_with_topology(&existing, topology_plane()),
        Some(false)
    );
}

#[test]
fn placed_carriers_reject_duplicate_model_surface_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(7, crate::surface::SurfaceKind::Cylinder));
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([cylinder_surface(7, 2.0), cylinder_surface(7, 3.0)]);

    assert!(!placed_carriers(&scan, &ir).contains_key(&7));
}

#[test]
fn placed_carriers_prefers_unique_positional_cylinder_frame() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(7, crate::surface::SurfaceKind::Cylinder));
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
                origin: [-12.5, 4.0, 0.0],
                axis: [0.0, 1.0, 0.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 0.75,
                length: Some(34.0),
            }),
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(cylinder_surface(7, 0.75));
    ir.model.surfaces[0].geometry = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 12.5),
        axis: Vector3::new(0.0, -1.0, 0.0),
        ref_direction: Vector3::new(0.0, 0.0, -1.0),
        radius: 0.75,
    };

    let carriers = placed_carriers(&scan, &ir);
    assert!(matches!(
        carriers.get(&7),
        Some(CarrierEquation::Cylinder(cylinder))
            if cylinder.origin == [-12.5, 4.0, 0.0]
                && cylinder.axis == [0.0, 1.0, 0.0]
                && cylinder.ref_direction == [1.0, 0.0, 0.0]
                && cylinder.radius == 0.75
    ));
}

#[test]
fn placed_carriers_keeps_non_inline_class913_model_carrier() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 913,
        header: [0, 0],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    let mut row = carrier_row(7, crate::surface::SurfaceKind::Cylinder);
    row.feature_id = 913;
    scan.surfaces.rows.push(row);
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
                origin: [-30.0, 6.5, -14.0],
                axis: [
                    std::f64::consts::FRAC_1_SQRT_2,
                    0.0,
                    std::f64::consts::FRAC_1_SQRT_2,
                ],
                ref_direction: [0.0, -1.0, 0.0],
                radius: 0.8,
                length: Some(0.282_842_712_474_619),
            }),
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        });
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(cylinder_surface(7, 0.2));

    let carriers = placed_carriers(&scan, &ir);
    assert!(matches!(
        carriers.get(&7),
        Some(CarrierEquation::Cylinder(cylinder)) if cylinder.origin == [0.0, 0.0, 0.0]
            && cylinder.axis == [0.0, 0.0, 1.0]
            && cylinder.radius == 0.2
    ));
}

#[test]
fn duplicate_model_surface_ids_remove_native_carrier() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(7, crate::surface::SurfaceKind::Plane));
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 7,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 0,
        });
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        carrier_surface(
            7,
            SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
        carrier_surface(
            7,
            SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 1.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
    ]);

    assert!(!placed_carriers(&scan, &ir).contains_key(&7));
}

#[test]
fn placed_carriers_admits_unique_rowless_model_surface() {
    let scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(cylinder_surface(7, 2.0));

    let carriers = placed_carriers(&scan, &ir);
    assert!(matches!(
        carriers.get(&7),
        Some(CarrierEquation::Cylinder(cylinder)) if cylinder.radius == 2.0
    ));
}

#[test]
fn placed_carriers_rejects_duplicate_rowless_model_surface_ids() {
    let scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([cylinder_surface(7, 2.0), cylinder_surface(7, 3.0)]);

    assert!(!placed_carriers(&scan, &ir).contains_key(&7));
}

#[test]
fn loop_classifier_rejects_inner_edge_crossing_concave_outer() {
    let outer = crate::topology::Loop {
        face_id: 5,
        half_edges: (0..8)
            .map(|index| crate::topology::HalfEdgeId {
                curve_id: 10 + index,
                side: 0,
            })
            .collect(),
    };
    let inner = crate::topology::Loop {
        face_id: 5,
        half_edges: (0..3)
            .map(|index| crate::topology::HalfEdgeId {
                curve_id: 20 + index,
                side: 0,
            })
            .collect(),
    };
    let vertices = [
        (1, [0.0, 0.0, 0.0]),
        (2, [6.0, 0.0, 0.0]),
        (3, [6.0, 1.0, 0.0]),
        (4, [2.0, 1.0, 0.0]),
        (5, [2.0, 5.0, 0.0]),
        (6, [6.0, 5.0, 0.0]),
        (7, [6.0, 6.0, 0.0]),
        (8, [0.0, 6.0, 0.0]),
        (9, [1.0, 0.5, 0.0]),
        (10, [5.0, 0.5, 0.0]),
        (11, [5.0, 5.5, 0.0]),
    ];
    let bindings = outer
        .half_edges
        .iter()
        .copied()
        .zip(1..=8)
        .chain(inner.half_edges.iter().copied().zip(9..=11))
        .map(
            |(half_edge, start_vertex_id)| crate::topology::HalfEdgeVertexIncidence {
                half_edge,
                start_vertex_id,
                end_vertex_id: None,
            },
        )
        .collect::<Vec<_>>();
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices = vertices.into_iter().collect::<BTreeMap<_, _>>();

    assert!(super::ordered_planar_face_loops(
        vec![&outer, &inner],
        PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
        &incidence,
        &solved_vertices,
    )
    .is_none());
}

#[test]
fn parameter_loop_classifier_orders_unique_outer() {
    let outer = crate::topology::Loop {
        face_id: 5,
        half_edges: (0..4)
            .map(|index| crate::topology::HalfEdgeId {
                curve_id: 10 + index,
                side: 0,
            })
            .collect(),
    };
    let inner = crate::topology::Loop {
        face_id: 5,
        half_edges: (0..4)
            .map(|index| crate::topology::HalfEdgeId {
                curve_id: 20 + index,
                side: 0,
            })
            .collect(),
    };
    let outer_polygon = vec![[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]];
    let inner_polygon = vec![[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

    let ordered = super::ordered_parameter_face_loops(
        vec![&inner, &outer],
        &[inner_polygon.clone(), outer_polygon.clone()],
    )
    .expect("one parameter-space outer loop");
    assert_eq!(ordered[0].half_edges[0].curve_id, 10);
    assert_eq!(ordered[1].half_edges[0].curve_id, 20);

    assert!(super::ordered_parameter_face_loops(
        vec![&outer, &inner],
        &[
            outer_polygon,
            vec![[3.0, 3.0], [4.0, 3.0], [4.0, 4.0], [3.0, 4.0]]
        ],
    )
    .is_none());
}

#[test]
fn topology_bound_plane_rejects_duplicate_model_curve_ids() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces
        .rows
        .push(carrier_row(5, crate::surface::SurfaceKind::Plane));
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 1,
            directions: [0; 2],
            faces: [5, 0],
            next_edges: [11, 0],
            offset: 20,
        });
    scan.topology.loops.push(crate::topology::Loop {
        face_id: 5,
        half_edges: vec![crate::topology::HalfEdgeId {
            curve_id: 11,
            side: 0,
        }],
    });

    let curve = Curve {
        id: CurveId("creo:visibgeom:curve#11".to_string()),
        geometry: CurveGeometry::Circle {
            center: Point3::new(2.0, 3.0, 4.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 5.0,
        },
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.curves.extend([curve.clone(), curve]);

    assert_eq!(
        transfer_topology_bound_planes(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &std::collections::BTreeSet::new(),
        ),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}
