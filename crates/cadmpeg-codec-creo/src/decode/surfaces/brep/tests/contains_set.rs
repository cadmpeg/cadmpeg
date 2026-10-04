// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::AnnotationBuilder;

use super::super::{transfer_native_brep, NativeBrepCurveEvidence};

fn derived_intersection_curve_fixture() -> (crate::container::ContainerScan<'static>, CadIr) {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.declared_body_count = Some(1);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 5,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 0,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 0,
        });
    let points = [
        [[0.0, 0.0], [1.0, 0.0]],
        [[1.0, 0.0], [1.0, 1.0]],
        [[1.0, 1.0], [0.0, 0.0]],
    ];
    scan.curves.topology_rows = [10_u32, 11, 12]
        .into_iter()
        .map(|id| crate::curve::CurveTopologyRow {
            id,
            type_byte: 0,
            feature_id: 0,
            directions: [0x01, 0xf6],
            faces: [std::num::NonZeroU32::new(5), None],
            next_edges: [id, 0],
            offset: 0,
        })
        .collect();
    scan.curves.pcurves = [10_u32, 11, 12]
        .into_iter()
        .zip(points)
        .map(|(curve_id, endpoints)| crate::curve::PcurveEndpoints {
            curve_id,
            faces: [5, 0].map(std::num::NonZeroU32::new),
            face_0_endpoints: endpoints,
            face_1_endpoints: [[0.0, 0.0], [0.0, 0.0]],
            offset: 0,
        })
        .collect();
    scan.topology.half_edges = [10_u32, 11, 12]
        .into_iter()
        .map(|curve_id| crate::topology::HalfEdge {
            id: crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(5),
            next: None,
        })
        .chain(
            [10_u32, 11, 12]
                .into_iter()
                .map(|curve_id| crate::topology::HalfEdge {
                    id: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::One,
                    },
                    face_id: None,
                    next: None,
                }),
        )
        .collect();
    scan.topology.loops.push(crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        [10_u32, 11, 12]
            .into_iter()
            .map(|curve_id| crate::topology::HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    ));
    scan.topology.face_components.push(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![5], vec![10, 11, 12])
        })
        .expect("component admission")
        .expect("valid component fixture"),
    );
    scan.topology.vertices = [1_u32, 2, 3]
        .into_iter()
        .zip([10_u32, 11, 12])
        .map(|(id, curve_id)| {
            crate::decode::with_test_decode_ctx(|ctx| {
                crate::topology::TopologicalVertex::new_for_test(
                    ctx,
                    id,
                    vec![crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::Zero,
                    }],
                )
            })
            .expect("vertex admission")
            .expect("valid vertex fixture")
        })
        .collect();
    let endpoint_pairs = [(10, 1, 2), (11, 2, 3), (12, 3, 1)];
    scan.topology.half_edge_vertex_incidence = endpoint_pairs
        .into_iter()
        .flat_map(|(curve_id, start, end)| {
            [
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::Zero,
                    },
                    start_vertex_id: std::num::NonZeroU32::new(start)
                        .expect("one-based vertex fixture"),
                    end_vertex_id: std::num::NonZeroU32::new(end),
                },
                crate::topology::HalfEdgeVertexIncidence {
                    half_edge: crate::topology::HalfEdgeId {
                        curve_id,
                        side: crate::topology::Side::One,
                    },
                    start_vertex_id: std::num::NonZeroU32::new(end)
                        .expect("one-based vertex fixture"),
                    end_vertex_id: std::num::NonZeroU32::new(start),
                },
            ]
        })
        .collect();

    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#5".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });
    for (id, origin, direction) in [
        (10, Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (11, Point3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        (
            12,
            Point3::new(1.0, 1.0, 0.0),
            Vector3::new(-1.0, -1.0, 0.0),
        ),
    ] {
        let curve = Curve {
            id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("identity grammar"),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    origin,
                    direction.unit().expect("valid LineCurve fixture"),
                )
                .expect("valid LineCurve fixture"),
            )),
            source_object: None,
        };
        ir.model.curves.push(curve);
    }

    (scan, ir)
}

#[test]
fn derived_intersection_curve_lookup_refuses_work_and_preserves_service_result() {
    let (scan, ir) = derived_intersection_curve_fixture();
    let curve_id = CurveId::mint("creo:visibgeom:curve#10").expect("identity grammar");
    let derived_intersections = BTreeSet::from([curve_id]);
    let result = crate::test_support::assert_work_boundaries(
        &["creo derived intersection curve lookup"],
        |ctx| {
            let mut service_ir = ir.clone();
            let summary = transfer_native_brep(
                ctx,
                &scan,
                &mut service_ir,
                &mut AnnotationBuilder::new(),
                NativeBrepCurveEvidence {
                    derived_intersections: &derived_intersections,
                    nurbs_endpoints: &BTreeSet::new(),
                },
                &mut Vec::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )?;
            Ok((summary, service_ir))
        },
    );
    let (summary, service_ir) = result;
    assert_eq!(summary.topological_point_count, 3);
    assert_eq!(summary.native_topological_edge_count, 3);
    assert_eq!(service_ir.model.edges.len(), 3);
    assert!(service_ir
        .model
        .edges
        .iter()
        .any(|edge| edge.id.as_str() == "creo:visibgeom:edge#10"));
}
