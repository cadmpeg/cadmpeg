// SPDX-License-Identifier: Apache-2.0

use super::{bounded_lateral_cylinder_cap, bounded_lateral_ellipse, Piece, PlaneFrame};
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
        &topology,
        1,
        7,
        &frame,
        &outline,
        (-4.0, 0.0),
        TOLERANCE
    ));
    assert!(!bounded_lateral_ellipse(
        &topology,
        1,
        7,
        &frame,
        &outline,
        (-2.5, 0.0),
        TOLERANCE
    ));
    let narrow = [Piece::Line(Point2::new(-0.5, 0.0), Point2::new(0.5, 0.0))];
    assert!(!bounded_lateral_ellipse(
        &topology,
        1,
        7,
        &frame,
        &narrow,
        (-4.0, 0.0),
        TOLERANCE
    ));
}

fn prism_topology() -> AsmHistoricalTopology {
    prism_over(1.0)
}

/// A prism of height 2 over the rectangle from the origin to `(width, 1)`.
pub(in crate::design::profile_select) fn prism_over(width: f64) -> AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCurveAxis, AsmHistoricalEdge, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalPoint,
    };
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(width, 0.0, 0.0),
        Point3::new(width, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(width, 0.0, 2.0),
        Point3::new(width, 1.0, 2.0),
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

/// `added_candidate` over the unit square sketch on z = 0 against `topology`
/// as the state after a transition from an empty state.
fn added_run(
    topology: AsmHistoricalTopology,
    start: &cadmpeg_ir::features::extents::ExtrudeStart,
    direction: &cadmpeg_ir::features::extents::ExtrudeDirection,
    length: f64,
    operation: cadmpeg_ir::features::operation_settings::BooleanOp,
) -> Option<cadmpeg_ir::features::profiles::PlanarProfileRef> {
    use crate::design::geometry::ProfileGeometryWork;
    use crate::design::profile_select::ScopedExtrudeProfileResolution;
    use crate::history_records::AsmDeltaState;
    use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
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
                operation: &operation,
                start,
                direction,
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
}

#[test]
fn added_cap_certificate_requires_the_blind_distance_and_analytic_lateral_carriers() {
    use cadmpeg_ir::features::extents::{ExtrudeDirection, ExtrudeStart};
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    let run = |topology, length, operation: BooleanOp| {
        added_run(
            topology,
            &ExtrudeStart::ProfilePlane {},
            &ExtrudeDirection::ProfileNormal {},
            length,
            operation,
        )
    };
    assert!(run(prism_topology(), 2.0, BooleanOp::Join).is_some());
    assert!(run(prism_topology(), 2.0, BooleanOp::NewBody).is_some());
    assert!(run(prism_topology(), 1.0, BooleanOp::Join).is_none());
    // A Cut or Intersect far cap cannot show a profile over no material.
    assert!(run(prism_topology(), 2.0, BooleanOp::Cut).is_none());
    assert!(run(prism_topology(), 2.0, BooleanOp::Intersect).is_none());
    let mut unsupported = prism_topology();
    unsupported.surface_planes[2].origin = Point3::new(0.0, 0.5, 0.0);
    assert!(run(unsupported, 2.0, BooleanOp::Join).is_none());
}

#[test]
fn an_offset_slab_proves_the_regions_of_the_profile_plane_extrude_moved_to_its_interval() {
    use cadmpeg_ir::features::extents::{ExtrudeDirection, ExtrudeStart};
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    use cadmpeg_ir::scalar::Length;
    let offset = |value| ExtrudeStart::OffsetProfilePlane {
        offset: Length::new(value).unwrap(),
    };
    // A slab of thickness 2 from z = -4 to z = -2, swept from the offset
    // plane z = -2 against the profile normal.
    let slab = || prism_moved(Vector3::new(0.0, 0.0, -4.0));
    let reversed = added_run(
        slab(),
        &offset(-2.0),
        &ExtrudeDirection::ReversedProfileNormal {},
        2.0,
        BooleanOp::Join,
    );
    assert!(reversed.is_some());
    // The same slab from the offset plane z = -4 along the profile normal.
    let forward = added_run(
        slab(),
        &offset(-4.0),
        &ExtrudeDirection::ProfileNormal {},
        2.0,
        BooleanOp::Join,
    );
    assert_eq!(forward, reversed);
    // The profile-plane extrude of the same prism sitting on z = 0.
    let on_plane = added_run(
        prism_topology(),
        &ExtrudeStart::ProfilePlane {},
        &ExtrudeDirection::ProfileNormal {},
        2.0,
        BooleanOp::Join,
    );
    assert_eq!(on_plane, reversed);
}

#[test]
fn an_offset_slab_whose_caps_do_not_match_is_not_proven() {
    use cadmpeg_ir::features::extents::{ExtrudeDirection, ExtrudeStart};
    use cadmpeg_ir::features::operation_settings::BooleanOp;
    use cadmpeg_ir::scalar::Length;
    let offset = |value| ExtrudeStart::OffsetProfilePlane {
        offset: Length::new(value).unwrap(),
    };
    let slab = || prism_moved(Vector3::new(0.0, 0.0, -4.0));
    let run = |start: &ExtrudeStart, direction: &ExtrudeDirection, length: f64| {
        added_run(slab(), start, direction, length, BooleanOp::Join)
    };
    let along = &ExtrudeDirection::ProfileNormal {};
    let against = &ExtrudeDirection::ReversedProfileNormal {};
    // The far cap is not at the end of the interval.
    assert!(run(&offset(-2.0), against, 1.0).is_none());
    assert!(run(&offset(-2.0), along, 2.0).is_none());
    assert!(run(&offset(-3.0), against, 2.0).is_none());
    // An offset start that is not the slab, read as the profile plane.
    assert!(run(&ExtrudeStart::ProfilePlane {}, against, 2.0).is_none());
    // A Cut leaves no far cap over empty material.
    assert!(added_run(
        slab(),
        &offset(-2.0),
        &ExtrudeDirection::ReversedProfileNormal {},
        2.0,
        BooleanOp::Cut
    )
    .is_none());
}

/// `prism_over(1.0)` moved by `offset`.
fn prism_moved(offset: Vector3) -> AsmHistoricalTopology {
    let mut topology = prism_topology();
    for row in &mut topology.point_positions {
        row.position = row.position.translated(offset, 1.0);
    }
    for row in &mut topology.surface_planes {
        row.origin = row.origin.translated(offset, 1.0);
    }
    for row in &mut topology.curve_axes {
        row.origin = row.origin.translated(offset, 1.0);
    }
    topology
}

#[test]
fn lateral_face_beyond_the_sweep_continues_a_deleted_face_on_its_carrier() {
    let frame = PlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u: Vector3::new(1.0, 0.0, 0.0),
        v: Vector3::new(0.0, 1.0, 0.0),
    };
    // Face 2 of the prism lies on the plane y = 0 from height 0 to height 2.
    let after = prism_topology();
    let outline = [Piece::Line(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))];
    let agrees = |before: &AsmHistoricalTopology, far: f64| {
        let deleted = super::deleted_sides(before, &[2], &frame);
        super::inserted_sides_agree(
            &after,
            &[2],
            &deleted,
            &frame,
            &outline,
            (far.min(0.0), far.max(0.0)),
            TOLERANCE,
        )
    };
    let none = AsmHistoricalTopology::default();
    assert!(agrees(&none, 2.0));
    assert!(!agrees(&none, 1.0));
    // A deleted face on the same plane from height 1 to height 3 covers the
    // part of the inserted face beyond the travel of 1.
    assert!(agrees(&prism_moved(Vector3::new(0.0, 0.0, 1.0)), 1.0));
    // A gap between the travel and the deleted face.
    assert!(!agrees(&prism_moved(Vector3::new(0.0, 0.0, 1.5)), 1.0));
    // A deleted face on a parallel plane.
    assert!(!agrees(&prism_moved(Vector3::new(0.0, 0.5, 1.0)), 1.0));
    // A deleted face that is parallel to the sketch plane.
    let mut parallel = prism_moved(Vector3::new(0.0, 0.0, 1.0));
    parallel.face_surfaces[2].carrier = parallel.face_surfaces[0].carrier;
    assert!(!agrees(&parallel, 1.0));
}

#[test]
fn two_sided_lateral_bounds_require_continuations_at_both_ends() {
    let frame = PlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u: Vector3::new(1.0, 0.0, 0.0),
        v: Vector3::new(0.0, 1.0, 0.0),
    };
    let after = prism_moved(Vector3::new(0.0, 0.0, -1.0));
    let outline = [Piece::Line(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))];
    let side = |low, high| super::DeletedSide {
        carrier: super::SideCarrier::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
        },
        low,
        high,
    };
    let agrees = |deleted: &[super::DeletedSide]| {
        super::inserted_sides_agree(
            &after,
            &[2],
            deleted,
            &frame,
            &outline,
            (-0.5, 0.5),
            TOLERANCE,
        )
    };
    assert!(agrees(&[side(-1.0, -0.5), side(0.5, 1.0)]));
    assert!(!agrees(&[side(-1.0, -0.6), side(0.5, 1.0)]));
    assert!(!agrees(&[side(-1.0, -0.5), side(0.6, 1.0)]));
    assert!(!agrees(&[side(-1.0, -0.5)]));
    assert!(!agrees(&[side(0.5, 1.0)]));
}

fn concentric_circles() -> (
    cadmpeg_ir::sketches::Sketch,
    Vec<cadmpeg_ir::sketches::SketchEntity>,
) {
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        SketchPlacement, SketchProfiles,
    };
    let sketch = Sketch {
        id: SketchId::mint("synthetic:test:sketch#floor").unwrap(),
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
    let entities = [("outer", 3.0), ("inner", 1.75)]
        .into_iter()
        .map(|(name, radius)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
                sketch.id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length::new(radius).unwrap(),
                })
                .unwrap(),
            )
        })
        .collect();
    (sketch, entities)
}

fn disk_cap(radius: f64) -> super::Cap {
    let ring = (0..64)
        .map(|k| {
            let angle = std::f64::consts::TAU * f64::from(k) / 64.0;
            Point2::new(radius * angle.cos(), radius * angle.sin())
        })
        .collect();
    super::Cap::new(vec![ring], Vec::new(), Vec::new(), 0.0, Vec::new())
}

/// Floor regions of two concentric circles of radius 3 and 1.75, with an
/// inserted face on `offset` that covers the ring between the circles.
fn floor_selection(offset: f64, far: Option<f64>, before: usize) -> Vec<usize> {
    let (sketch, entities) = concentric_circles();
    let ring_floor = {
        let outer = disk_cap(3.0);
        let inner = disk_cap(1.75);
        super::Cap::new(
            outer.loops.into_iter().chain(inner.loops).collect(),
            Vec::new(),
            Vec::new(),
            0.0,
            Vec::new(),
        )
    };
    let planes = vec![super::PlaneSides {
        offset,
        before: (0..before).map(|_| disk_cap(3.0)).collect(),
        after: vec![ring_floor],
    }];
    crate::test_support::with_decode_context(|ctx| {
        let work = crate::design::geometry::ProfileGeometryWork::new();
        let regions = crate::design::geometry::atomic_sketch_regions(
            &sketch, &entities, TOLERANCE, &work, ctx,
        )
        .unwrap()
        .unwrap();
        let sampled = super::sample_regions(&regions, TOLERANCE, &work).unwrap();
        let selected = super::floor_regions(&sampled, &planes, far, TOLERANCE);
        // Only the ring has a hole region nested in it.
        selected
            .iter()
            .map(|index| sampled[*index].nested.len())
            .collect()
    })
}

#[test]
fn a_floor_face_selects_the_ring_it_covers_and_not_the_disk_inside_it() {
    assert_eq!(floor_selection(-20.0, Some(-20.0), 0), vec![1]);
}

#[test]
fn a_floor_needs_the_signed_length_and_an_empty_before_side() {
    assert!(floor_selection(-20.0, Some(20.0), 0).is_empty());
    assert!(floor_selection(-20.0, None, 0).is_empty());
    assert!(floor_selection(-19.0, Some(-20.0), 0).is_empty());
    assert!(floor_selection(-20.0, Some(-20.0), 1).is_empty());
}

/// A lateral cylinder of radius `radius` along the Z axis and a cylinder of
/// radius 2 along the Y axis through height -3 that cut each other along
/// edge 7.
fn crossing_cylinders(radius: f64) -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        surface_cylinders: vec![
            AsmHistoricalCylinder {
                surface: 1,
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radius,
            },
            AsmHistoricalCylinder {
                surface: 2,
                origin: Point3::new(0.0, 0.0, -3.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                radius: 2.0,
            },
        ],
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
    }
}

#[test]
fn a_lateral_edge_cut_by_a_transverse_cylinder_is_bounded_on_its_branch() {
    let frame = PlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u: Vector3::new(1.0, 0.0, 0.0),
        v: Vector3::new(0.0, 1.0, 0.0),
    };
    let bounded = |topology: &AsmHistoricalTopology, height: f64, travel: (f64, f64)| {
        bounded_lateral_cylinder_cap(
            topology,
            1,
            7,
            &frame,
            Point3::new(1.0, 0.0, height),
            travel,
            TOLERANCE,
        )
    };
    let topology = crossing_cylinders(1.0);
    // The edge keeps heights from -3 + sqrt(3) to -3 + 2 on the upper branch
    // and the mirrored heights on the lower branch.
    assert!(bounded(&topology, -1.2, (-4.0, 0.0)));
    assert!(bounded(&topology, -1.2, (-1.3, -1.0)));
    assert!(!bounded(&topology, -1.2, (-1.1, 0.0)));
    assert!(!bounded(&topology, -1.2, (-4.0, -1.1)));
    assert!(bounded(&topology, -4.8, (-5.0, -4.7)));
    assert!(!bounded(&topology, -4.8, (-4.0, 0.0)));
    // A lateral circle that reaches the silhouette of the second cylinder
    // leaves its branch.
    assert!(!bounded(&crossing_cylinders(2.5), -1.2, (-9.0, 9.0)));
}
