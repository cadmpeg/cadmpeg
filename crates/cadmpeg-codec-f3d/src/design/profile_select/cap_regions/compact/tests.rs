// SPDX-License-Identifier: Apache-2.0

use super::{bounded_lateral_ellipse, Piece, PlaneFrame};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalPlane,
    AsmHistoricalRelation, AsmHistoricalTopology,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const TOLERANCE: f64 = 1.0e-6;

#[test]
fn lateral_ellipse_certificate_bounds_the_complete_carrier_and_its_projection() {
    let frame = PlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u: Vector3::new(1.0, 0.0, 0.0),
        v: Vector3::new(0.0, 1.0, 0.0),
    };
    let topology = AsmHistoricalTopology {
        surface_planes: vec![AsmHistoricalPlane {
            surface: 1,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
        }],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 2,
            origin: Point3::new(0.0, 0.0, -2.0),
            axis: Vector3::new(0.0, 0.8, 0.6),
            radius: 1.0,
        }],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 1,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 2,
            },
        ],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 11,
                member_refs: vec![20],
            },
            AsmHistoricalRelation {
                owner_ref: 12,
                member_refs: vec![10],
            },
        ],
        coedge_topology: [(21, 20), (22, 10)]
            .into_iter()
            .map(|(coedge, owner_loop)| AsmHistoricalCoedge {
                coedge,
                owner_loop,
                edge: 7,
                next: 0,
                previous: 0,
                radial_next: 0,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    };
    let outline = [Piece::Line(Point2::new(-2.0, 0.0), Point2::new(2.0, 0.0))];
    assert!(bounded_lateral_ellipse(
        &topology, 1, 7, &frame, &outline, -4.0, TOLERANCE
    ));
    assert!(!bounded_lateral_ellipse(
        &topology, 1, 7, &frame, &outline, -2.5, TOLERANCE
    ));
    let narrow = [Piece::Line(Point2::new(-0.5, 0.0), Point2::new(0.5, 0.0))];
    assert!(!bounded_lateral_ellipse(
        &topology, 1, 7, &frame, &narrow, -4.0, TOLERANCE
    ));
}

fn prism_topology() -> AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCurveAxis, AsmHistoricalEdge, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalPoint,
    };
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(1.0, 0.0, 2.0),
        Point3::new(1.0, 1.0, 2.0),
        Point3::new(0.0, 1.0, 2.0),
    ];
    let facets = [
        ([0, 3, 2, 1], Vector3::new(0.0, 0.0, -1.0)),
        ([4, 5, 6, 7], Vector3::new(0.0, 0.0, 1.0)),
        ([0, 1, 5, 4], Vector3::new(0.0, -1.0, 0.0)),
        ([1, 2, 6, 5], Vector3::new(1.0, 0.0, 0.0)),
        ([2, 3, 7, 6], Vector3::new(0.0, 1.0, 0.0)),
        ([3, 0, 4, 7], Vector3::new(-1.0, 0.0, 0.0)),
    ];
    let mut topology = AsmHistoricalTopology::default();
    for (index, position) in points.into_iter().enumerate() {
        let index = i64::try_from(index).unwrap();
        topology.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: 1000 + index,
            carrier: 2000 + index,
        });
        topology.point_positions.push(AsmHistoricalPoint {
            point: 2000 + index,
            position,
        });
    }
    for (index, (corners, normal)) in facets.into_iter().enumerate() {
        let face = i64::try_from(index).unwrap();
        topology.faces.push(face);
        topology.surfaces.push(100 + face);
        topology.face_surfaces.push(AsmHistoricalCarrierBinding {
            entity: face,
            carrier: 100 + face,
        });
        topology.surface_planes.push(AsmHistoricalPlane {
            surface: 100 + face,
            origin: points[corners[0]],
            normal,
        });
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![200 + face],
        });
        let mut coedges = Vec::new();
        for side in 0..4 {
            let edge = 300 + face * 4 + i64::try_from(side).unwrap();
            let start = corners[side];
            let end = corners[(side + 1) % 4];
            coedges.push(edge);
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: edge,
                owner_loop: 200 + face,
                edge,
                next: 0,
                previous: 0,
                radial_next: 0,
            });
            topology.edge_vertices.push(AsmHistoricalEdge {
                edge,
                start_vertex: 1000 + i64::try_from(start).unwrap(),
                end_vertex: 1000 + i64::try_from(end).unwrap(),
            });
            topology
                .edge_curves
                .push(AsmHistoricalOptionalCarrierBinding {
                    entity: edge,
                    carrier: Some(edge + 1000),
                });
            topology.curve_axes.push(AsmHistoricalCurveAxis {
                curve: edge + 1000,
                origin: points[start],
                direction: points[end].vector_from(points[start]),
            });
        }
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: 200 + face,
            member_refs: coedges,
        });
    }
    topology
}

#[test]
fn added_cap_certificate_requires_the_blind_distance_and_analytic_lateral_carriers() {
    use crate::design::geometry::ProfileGeometryWork;
    use crate::design::profile_select::ScopedExtrudeProfileResolution;
    use crate::history_records::AsmDeltaState;
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
    };
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::NonZeroLength;
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        SketchPlacement, SketchProfiles,
    };
    let id = SketchId::mint("synthetic:test:sketch#added-prism").unwrap();
    let sketch = Sketch {
        id: id.clone(),
        name: None,
        configuration: None,
        visible: None,
        native_ref: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
    };
    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(1.0, 1.0),
        Point2::new(0.0, 1.0),
    ];
    let entities = (0..4)
        .map(|i| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:curve#{i}")).unwrap(),
                id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: corners[i],
                    end: corners[(i + 1) % 4],
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let run = |topology, length| {
        let states = [(2, topology), (1, AsmHistoricalTopology::default())]
            .into_iter()
            .map(|(state_id, topology)| {
                (
                    AsmDeltaState {
                        id: format!("synthetic:test:state#{state_id}"),
                        parent: "synthetic:test:history#prism".into(),
                        byte_offset: 0,
                        state_id,
                        version_flag: 1,
                        state_flag: 0,
                        previous_ref: None,
                        next_ref: None,
                        node_index: state_id,
                        partner_ref: None,
                        owner_ref: 0,
                        bulletin_boards: Vec::new(),
                        records: Vec::new(),
                        transition: None,
                    },
                    Some(topology),
                    Vec::new(),
                )
            })
            .collect();
        let history = crate::test_support::history::history(
            "synthetic:test:history#prism".into(),
            0,
            None,
            false,
            states,
        );
        crate::test_support::with_decode_context(|ctx| {
            let work = ProfileGeometryWork::new();
            let resolution = ScopedExtrudeProfileResolution {
                entities: &entities,
                spatial_entities: &[],
                histories: std::slice::from_ref(&history),
                linear_tolerance: TOLERANCE,
                angular_tolerance: TOLERANCE,
                profile_work: &work,
                ctx,
            };
            super::added_candidate(
                &sketch,
                resolution,
                (2, 1),
                super::Sweep {
                    operation: &BooleanOp::Join,
                    start: &ExtrudeStart::ProfilePlane {},
                    direction: &ExtrudeDirection::ProfileNormal {},
                    extent: &ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::Blind {
                                length: NonZeroLength::new(length).unwrap(),
                            },
                            draft: None,
                        },
                    },
                },
            )
        })
        .unwrap()
    };
    assert!(run(prism_topology(), 2.0).is_some());
    assert!(run(prism_topology(), 1.0).is_none());
    let mut unsupported = prism_topology();
    unsupported.surface_planes[2].origin = Point3::new(0.0, 0.5, 0.0);
    assert!(run(unsupported, 2.0).is_none());
}
