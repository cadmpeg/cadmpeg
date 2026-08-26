//! Tests: NURBS endpoint witnesses in carrier-intersection selection.

use crate::curve::CurveTopologyRow;
use crate::decode::surfaces::transfer_carrier_intersection_curves;
use crate::topology::{HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence, TopologicalVertex};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, NurbsCurve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use std::collections::BTreeSet;

const EPS_POSITION: f64 = 1e-12;

fn topology_row(id: u32, faces: [u32; 2]) -> CurveTopologyRow {
    CurveTopologyRow {
        id,
        type_byte: 0x05,
        feature_id: 0,
        directions: [0x01, 0xf6],
        faces,
        next_edges: [id, id],
        offset: 0,
    }
}

fn half_edge(curve_id: u32, side: u8, face_id: u32) -> HalfEdge {
    HalfEdge {
        id: HalfEdgeId { curve_id, side },
        face_id,
        next: None,
    }
}

fn incidence(
    curve_id: u32,
    side: u8,
    start_vertex_id: u32,
    end_vertex_id: u32,
) -> HalfEdgeVertexIncidence {
    HalfEdgeVertexIncidence {
        half_edge: HalfEdgeId { curve_id, side },
        start_vertex_id,
        end_vertex_id: Some(end_vertex_id),
    }
}

fn carrier_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows = [1_u32, 2, 3, 4]
        .into_iter()
        .map(|id| {
            let kind = if id <= 2 {
                crate::surface::SurfaceKind::Cylinder
            } else {
                crate::surface::SurfaceKind::Plane
            };
            crate::surface::SurfaceRow {
                id,
                type_byte: kind.canonical_type_byte(),
                kind,
                feature_id: 0,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            }
        })
        .collect();
    scan.curves.topology_rows = vec![topology_row(10, [0, 0]), topology_row(20, [1, 2])];
    scan.topology.half_edges = vec![
        half_edge(10, 0, 0),
        half_edge(10, 1, 0),
        half_edge(20, 0, 1),
        half_edge(20, 1, 2),
        half_edge(30, 0, 3),
        half_edge(30, 1, 0),
        half_edge(31, 0, 4),
        half_edge(31, 1, 0),
    ];
    scan.topology.vertices = vec![
        TopologicalVertex {
            id: 1,
            half_edges: vec![
                HalfEdgeId {
                    curve_id: 10,
                    side: 0,
                },
                HalfEdgeId {
                    curve_id: 20,
                    side: 0,
                },
                HalfEdgeId {
                    curve_id: 30,
                    side: 0,
                },
                HalfEdgeId {
                    curve_id: 31,
                    side: 0,
                },
            ],
        },
        TopologicalVertex {
            id: 2,
            half_edges: vec![
                HalfEdgeId {
                    curve_id: 10,
                    side: 1,
                },
                HalfEdgeId {
                    curve_id: 20,
                    side: 1,
                },
            ],
        },
    ];
    scan.topology.half_edge_vertex_incidence = vec![
        incidence(10, 0, 1, 2),
        incidence(10, 1, 2, 1),
        incidence(20, 0, 1, 2),
        incidence(20, 1, 2, 1),
    ];
    scan
}

fn source_ir() -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("creo:visibgeom:surface#1".to_string()),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#3".to_string()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 5.0_f64.sqrt(), 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#4".to_string()),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: SurfaceId("creo:visibgeom:surface#2".to_string()),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(4.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        },
    ]);
    let y = 5.0_f64.sqrt();
    ir.model.curves.push(Curve {
        id: CurveId("creo:visibgeom:curve#10".to_string()),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(2.0, y, 0.0), Point3::new(2.0, y, 5.0)],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });
    ir
}

#[test]
fn carrier_intersection_uses_nurbs_boundary_endpoints_to_select_a_generator() {
    let scan = carrier_scan();
    let witness = BTreeSet::from([CurveId("creo:visibgeom:curve#10".to_string())]);

    let mut with_witness = source_ir();
    let transferred = transfer_carrier_intersection_curves(
        &scan,
        &mut with_witness,
        &mut AnnotationBuilder::new(),
        &witness,
    );
    assert_eq!(
        transferred,
        BTreeSet::from([CurveId("creo:visibgeom:curve#20".to_string())])
    );
    assert!(matches!(
        with_witness
            .model
            .curves
            .iter()
            .find(|curve| curve.id == CurveId("creo:visibgeom:curve#20".to_string()))
        .map(|curve| &curve.geometry),
        Some(CurveGeometry::Line { origin, direction })
            if (origin.x - 2.0).abs() <= EPS_POSITION
                && (origin.y - 5.0_f64.sqrt()).abs() <= EPS_POSITION
                && direction.z == 1.0
    ));

    let mut without_witness = source_ir();
    assert!(transfer_carrier_intersection_curves(
        &scan,
        &mut without_witness,
        &mut AnnotationBuilder::new(),
        &BTreeSet::new(),
    )
    .is_empty());
}
