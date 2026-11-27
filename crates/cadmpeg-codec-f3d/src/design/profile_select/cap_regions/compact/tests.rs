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

#[test]
fn added_cap_certificate_requires_the_blind_distance_and_analytic_lateral_carriers() {
    use crate::design::geometry::ProfileGeometryWork;
    use crate::design::profile_select::ScopedExtrudeProfileResolution;
    use crate::history_records::AsmDeltaState;
    use cadmpeg_ir::features::extents::{
        ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
    };
    use cadmpeg_ir::features::operation_settings::BooleanOp;
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
    let run = |topology, length, operation: BooleanOp| {
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

type Line = (&'static str, (f64, f64), (f64, f64));

/// The straight sketch curves of an axis-aligned rectangle from `x0` to `x1`.
fn rectangle(name: &'static str, x0: f64, x1: f64) -> [Line; 4] {
    let names: [&'static str; 4] = match name {
        "island" => ["island-bottom", "island-right", "island-top", "island-left"],
        _ => ["bottom", "right", "top", "left"],
    };
    let (y0, y1) = if name == "island" {
        (0.25, 0.75)
    } else {
        (0.0, 1.0)
    };
    [
        (names[0], (x0, y0), (x1, y0)),
        (names[1], (x1, y0), (x1, y1)),
        (names[2], (x1, y1), (x0, y1)),
        (names[3], (x0, y1), (x0, y0)),
    ]
}

/// A body face at `height` over the unit-high strip from `x0` to `x1`.
fn strip(face: i64, height: f64, x0: f64, x1: f64) -> super::super::tests::PlaneFace {
    (
        face,
        height,
        vec![vec![(x0, 0.0), (x1, 0.0), (x1, 1.0), (x0, 1.0)]],
    )
}

/// Run `intersect_candidate` on a sketch of `lines` and a transition from the
/// body faces `before` to the body faces `after`. The result and the
/// assumption flags the call left on the shared work.
fn run_intersect(
    lines: &[Line],
    before: &[super::super::tests::PlaneFace],
    after: &[super::super::tests::PlaneFace],
    operation: BooleanOp,
    named: &[&str],
    sweep: (ExtrudeStart, ExtrudeDirection, ExtrudeExtent),
) -> (Option<PlanarProfileRef>, [bool; 11]) {
    run_intersect_with_gaps(lines, before, after, operation, named, sweep, &[])
}

/// `run_intersect` with the loops of the faces `gaps` removed, so that those
/// faces have no proven cap.
fn run_intersect_with_gaps(
    lines: &[Line],
    before: &[super::super::tests::PlaneFace],
    after: &[super::super::tests::PlaneFace],
    operation: BooleanOp,
    named: &[&str],
    sweep: (ExtrudeStart, ExtrudeDirection, ExtrudeExtent),
    gaps: &[i64],
) -> (Option<PlanarProfileRef>, [bool; 11]) {
    use super::super::tests::plane_faces;
    use crate::design::geometry::ProfileGeometryWork;
    use crate::design::profile_select::ScopedExtrudeProfileResolution;
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTransition, AsmHistory, AsmStateSnapshot,
    };
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        SketchPlacement, SketchProfiles,
    };
    use std::collections::{BTreeMap, HashSet};

    let id = SketchId::mint("synthetic:test:sketch#intersect").unwrap();
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
    let entities = lines
        .iter()
        .map(|(name, start, end)| {
            SketchEntity::new(
                SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap(),
                id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start.0, start.1),
                    end: Point2::new(end.0, end.1),
                })
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let mut topologies = [plane_faces(before), plane_faces(after)];
    for topology in &mut topologies {
        topology
            .face_loops
            .retain(|relation| !gaps.contains(&relation.owner_ref));
    }
    let empty = BTreeMap::new();
    let snapshot = |index: usize| AsmStateSnapshot {
        state_id: i64::try_from(index + 1).unwrap(),
        topology: &topologies[index],
        versions: &empty,
    };
    let states = (0..2usize)
        .map(|index| {
            let transition = AsmHistoricalTransition::between(
                index.checked_sub(1).map(snapshot),
                snapshot(index),
            )
            .unwrap();
            AsmDeltaState {
                id: format!("synthetic:test:state#{}", index + 1),
                parent: "synthetic:test:history#intersect".into(),
                byte_offset: 0,
                state_id: i64::try_from(index + 1).unwrap(),
                version_flag: 1,
                state_flag: 0,
                previous_ref: (index == 0).then_some(2),
                next_ref: (index == 1).then_some(1),
                node_index: i64::try_from(index + 1).unwrap(),
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: Some(transition),
            }
        })
        .collect::<Vec<_>>();
    let history = AsmHistory::new(
        "synthetic:test:history#intersect".into(),
        0,
        None,
        false,
        states,
    );
    let named = named
        .iter()
        .map(|name| SketchEntityId::mint(format!("synthetic:test:curve#{name}")).unwrap())
        .collect::<HashSet<_>>();
    let (start, direction, extent) = sweep;
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
        let selected = super::intersect_candidate(
            &sketch,
            resolution,
            (2, 1),
            super::Sweep {
                operation: &operation,
                start: &start,
                direction: &direction,
                extent: &extent,
            },
            &named,
        )
        .unwrap();
        (selected, work.assumed_flags())
    })
}

fn blind(length: f64) -> ExtrudeSide {
    ExtrudeSide {
        termination: LinearTermination::Blind {
            length: NonZeroLength::new(length).unwrap(),
        },
        draft: None,
    }
}

/// The sweep of two units below the sketch plane.
fn two_down() -> (ExtrudeStart, ExtrudeDirection, ExtrudeExtent) {
    (
        ExtrudeStart::ProfilePlane {},
        ExtrudeDirection::ReversedProfileNormal {},
        ExtrudeExtent::OneSided { side: blind(2.0) },
    )
}

/// The curve identities of the boundary of the one selected region.
fn selected_curves(selection: Option<PlanarProfileRef>) -> Vec<String> {
    let Some(PlanarProfileRef::SketchRegions { regions, .. }) = selection else {
        panic!("sketch regions");
    };
    assert_eq!(regions.len(), 1);
    let SketchProfileRegion::Trimmed { outer_boundary, .. } = &regions[0] else {
        panic!("trimmed region");
    };
    let mut curves: Vec<String> = outer_boundary
        .iter()
        .map(|use_| use_.entity.to_string())
        .collect();
    curves.sort();
    curves.dedup();
    curves
}

use cadmpeg_ir::features::extents::{
    ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination,
};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::scalar::NonZeroLength;

/// Two unit squares side by side whose shared edge is a middle curve. A
/// body over both squares loses the second square at the Intersect.
#[test]
fn an_intersect_keeps_the_regions_its_transition_did_not_remove() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let before = [strip(1, 0.0, 0.0, 2.0), strip(2, -2.0, 0.0, 2.0)];
    let after = [strip(3, 0.0, 0.0, 1.0), strip(4, -2.0, 0.0, 1.0)];
    let run = |operation: BooleanOp, named: &[&str]| {
        run_intersect(&lines, &before, &after, operation, named, two_down()).0
    };
    let selected = run(BooleanOp::Intersect, &["left", "middle", "bottom", "top"]);
    let curves = selected_curves(selected);
    assert!(curves.iter().any(|curve| curve.ends_with("#left")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#right")));
    // Only an Intersect reads its profile from the regions it did not lose.
    assert!(run(BooleanOp::Cut, &["left", "middle", "bottom", "top"]).is_none());
    // A named curve that bounds no kept region leaves the selection native.
    assert!(run(BooleanOp::Intersect, &["right"]).is_none());
    assert!(run(BooleanOp::Intersect, &[]).is_none());
}

/// Three squares B, A and C side by side. The body lies over B and A only, and
/// the Intersect names the boundary of A, including the edge it shares with
/// the empty square C. C holds no material on either side of the transition,
/// so it is not kept.
#[test]
fn an_intersect_does_not_keep_a_region_over_no_material() {
    let mut lines = rectangle("outer", 0.0, 3.0).to_vec();
    lines.push(("first", (1.0, 0.0), (1.0, 1.0)));
    lines.push(("second", (2.0, 0.0), (2.0, 1.0)));
    let before = [strip(1, 0.0, 0.0, 2.0), strip(2, -2.0, 0.0, 2.0)];
    let after = [strip(3, 0.0, 1.0, 2.0), strip(4, -2.0, 1.0, 2.0)];
    let (selected, flags) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["first", "second", "bottom", "top"],
        two_down(),
    );
    // Straight curves and decided islands leave no assumption.
    assert!(flags.iter().all(|flag| !flag));
    let curves = selected_curves(selected);
    assert!(curves.iter().any(|curve| curve.ends_with("#first")));
    assert!(curves.iter().any(|curve| curve.ends_with("#second")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#left")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#right")));
}

/// A plane that the Intersect loses because it lies beyond the extent of the
/// tool changes every region over it. It is no evidence about the profile.
#[test]
fn an_intersect_ignores_the_planes_its_extent_does_not_reach() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    // The body reaches two units below the sketch plane. The tool reaches one
    // unit: its end face lies inside the body, and the deeper face is lost.
    let before = [strip(1, 0.0, 0.0, 2.0), strip(2, -2.0, 0.0, 2.0)];
    let after = [strip(3, 0.0, 1.0, 2.0), strip(4, -1.0, 1.0, 2.0)];
    let one_down = (
        ExtrudeStart::ProfilePlane {},
        ExtrudeDirection::ReversedProfileNormal {},
        ExtrudeExtent::OneSided { side: blind(1.0) },
    );
    let named = ["middle", "right", "bottom", "top"];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &named,
        one_down,
    );
    let curves = selected_curves(selected);
    assert!(curves.iter().any(|curve| curve.ends_with("#right")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#left")));
    // A tool that starts above the body reaches no changed plane.
    let above = (
        ExtrudeStart::OffsetProfilePlane {
            offset: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
        },
        ExtrudeDirection::ProfileNormal {},
        ExtrudeExtent::OneSided { side: blind(1.0) },
    );
    assert!(
        run_intersect(&lines, &before, &after, BooleanOp::Intersect, &named, above)
            .0
            .is_none()
    );
    // An extent that is not a blind distance has no height range.
    let unbounded = (
        ExtrudeStart::ProfilePlane {},
        ExtrudeDirection::ReversedProfileNormal {},
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::ThroughAll {},
                draft: None,
            },
        },
    );
    assert!(run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &named,
        unbounded
    )
    .0
    .is_none());
}

/// A kept region that encloses an island leaves the selection native, and the
/// same layout without the island selects the region.
#[test]
fn an_intersect_refuses_a_kept_region_that_encloses_an_island() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    lines.extend(rectangle("island", 1.25, 1.75));
    let before = [strip(1, 0.0, 0.0, 2.0), strip(2, -2.0, 0.0, 2.0)];
    let after = [strip(3, 0.0, 1.0, 2.0), strip(4, -2.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["middle", "right", "bottom", "top"],
        two_down(),
    );
    assert!(selected.is_none());
    let (selected, _) = run_intersect(
        &rectangle("outer", 0.0, 2.0)
            .into_iter()
            .chain([("middle", (1.0, 0.0), (1.0, 1.0))])
            .collect::<Vec<_>>(),
        &before,
        &after,
        BooleanOp::Intersect,
        &["middle", "right", "bottom", "top"],
        two_down(),
    );
    assert_eq!(selected_curves(selected).len(), 4);
}

/// A face that the transition leaves unchanged holds material before and
/// after it. The squares side by side have separate body faces, and only the
/// faces over the first square are deleted.
#[test]
fn an_intersect_reads_the_material_of_the_faces_it_left_unchanged() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let before = [
        strip(1, 0.0, 0.0, 1.0),
        strip(2, 0.0, 1.0, 2.0),
        strip(3, -2.0, 0.0, 1.0),
        strip(4, -2.0, 1.0, 2.0),
    ];
    let after = [strip(2, 0.0, 1.0, 2.0), strip(4, -2.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["middle", "right", "bottom", "top"],
        two_down(),
    );
    let curves = selected_curves(selected);
    assert!(curves.iter().any(|curve| curve.ends_with("#right")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#left")));
}

/// A plane that no face of the transition changed shows material that the
/// transition retained, when it lies within the extent of the Intersect.
#[test]
fn an_intersect_reads_the_retained_material_of_an_unchanged_plane() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let named = ["middle", "right", "bottom", "top"];
    let before = [strip(1, 0.0, 0.0, 1.0), strip(2, -1.0, 1.0, 2.0)];
    let after = [strip(2, -1.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &named,
        two_down(),
    );
    let curves = selected_curves(selected);
    assert!(curves.iter().any(|curve| curve.ends_with("#right")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#left")));
}

/// The face over the first square is deleted at `height`. Within the extent
/// of a one-unit tool it removes the square, and beyond it the deletion is no
/// evidence about the profile.
#[test]
fn a_deletion_beyond_the_extent_removes_no_region() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let named = ["middle", "right", "bottom", "top"];
    let one_down = || {
        (
            ExtrudeStart::ProfilePlane {},
            ExtrudeDirection::ReversedProfileNormal {},
            ExtrudeExtent::OneSided { side: blind(1.0) },
        )
    };
    let run = |height: f64| {
        let before = [strip(1, height, 0.0, 1.0), strip(2, -0.5, 1.0, 2.0)];
        let after = [strip(2, -0.5, 1.0, 2.0)];
        run_intersect(
            &lines,
            &before,
            &after,
            BooleanOp::Intersect,
            &named,
            one_down(),
        )
        .0
    };
    let curves = selected_curves(run(-1.0));
    assert!(curves.iter().any(|curve| curve.ends_with("#right")));
    assert!(run(-5.0).is_none());
}

fn three_squares() -> Vec<Line> {
    let mut lines = rectangle("outer", 0.0, 3.0).to_vec();
    lines.push(("first", (1.0, 0.0), (1.0, 1.0)));
    lines.push(("second", (2.0, 0.0), (2.0, 1.0)));
    lines
}

/// A region that a plane holds on both sides is retained material. A profile
/// that does not enclose it leaves the selection native.
#[test]
fn an_intersect_refuses_a_held_region_outside_its_profile() {
    let lines = three_squares();
    let before = [
        strip(1, 0.0, 0.0, 1.0),
        strip(2, 0.0, 1.0, 2.0),
        strip(3, 0.0, 2.0, 3.0),
    ];
    let after = [strip(2, 0.0, 1.0, 2.0), strip(3, 0.0, 2.0, 3.0)];
    let named = ["first", "second", "bottom", "top"];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &named,
        two_down(),
    );
    assert!(selected.is_none());
    // The control without the retained third square selects the second.
    let after = [strip(2, 0.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &named,
        two_down(),
    );
    assert!(selected.is_some());
}

/// A region that one plane loses and another holds has conflicting evidence.
#[test]
fn an_intersect_refuses_a_region_both_lost_and_held() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let before = [
        strip(1, 0.0, 0.0, 1.0),
        strip(2, -1.0, 0.0, 1.0),
        strip(3, -1.0, 1.0, 2.0),
    ];
    let after = [strip(2, -1.0, 0.0, 1.0), strip(3, -1.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["middle", "right", "bottom", "top"],
        two_down(),
    );
    assert!(selected.is_none());
}

/// A changed face with no proven cap on a plane within the extent leaves the
/// selection native, and one beyond the extent does not. An unchanged face
/// with no proven cap on a plane within the extent does the same.
#[test]
fn a_face_without_a_proven_cap_blocks_only_the_planes_in_range() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let named = ["middle", "right", "bottom", "top"];
    let run = |before: &[super::super::tests::PlaneFace],
               after: &[super::super::tests::PlaneFace],
               gaps: &[i64]| {
        run_intersect_with_gaps(
            &lines,
            before,
            after,
            BooleanOp::Intersect,
            &named,
            two_down(),
            gaps,
        )
        .0
    };
    let kept = [strip(2, 0.0, 1.0, 2.0)];
    // Deleted beyond the extent: ignored.
    let before = [
        strip(1, 0.0, 0.0, 1.0),
        strip(2, 0.0, 1.0, 2.0),
        strip(3, -5.0, 0.0, 2.0),
    ];
    assert!(run(&before, &kept, &[3]).is_some());
    // Deleted within the extent: native.
    let before = [
        strip(1, 0.0, 0.0, 1.0),
        strip(2, 0.0, 1.0, 2.0),
        strip(3, -1.0, 1.0, 2.0),
    ];
    assert!(run(&before, &kept, &[3]).is_none());
    // Unchanged within the extent: native.
    let after = [strip(2, 0.0, 1.0, 2.0), strip(3, -1.0, 1.0, 2.0)];
    assert!(run(&before, &after, &[3]).is_none());
    // The same layout with a proven cap selects the region.
    assert!(run(&before, &after, &[]).is_some());
}

/// Four squares B, A, C and D side by side with the body over B and A only.
/// The profile names the boundary of A. D drops out because its right side is
/// an unnamed boundary piece, and C then drops out because its piece shared
/// with D is no longer inside.
#[test]
fn the_faces_of_an_intersect_profile_shrink_until_their_unnamed_pieces_are_inside() {
    let mut lines = rectangle("outer", 0.0, 4.0).to_vec();
    lines.push(("first", (1.0, 0.0), (1.0, 1.0)));
    lines.push(("second", (2.0, 0.0), (2.0, 1.0)));
    lines.push(("third", (3.0, 0.0), (3.0, 1.0)));
    let before = [strip(1, 0.0, 0.0, 2.0), strip(2, -2.0, 0.0, 2.0)];
    let after = [strip(3, 0.0, 1.0, 2.0), strip(4, -2.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["first", "second", "bottom", "top"],
        two_down(),
    );
    let curves = selected_curves(selected);
    assert!(curves.iter().any(|curve| curve.ends_with("#first")));
    assert!(curves.iter().any(|curve| curve.ends_with("#second")));
    assert!(!curves.iter().any(|curve| curve.ends_with("#third")));
}

#[test]
fn the_extent_interval_follows_the_start_direction_and_sides() {
    use cadmpeg_ir::scalar::Length;
    let interval = |start: ExtrudeStart, direction: ExtrudeDirection, extent: ExtrudeExtent| {
        super::extent_interval(&super::Sweep {
            operation: &BooleanOp::Intersect,
            start: &start,
            direction: &direction,
            extent: &extent,
        })
    };
    let two_sided = || ExtrudeExtent::TwoSided {
        first: blind(3.0),
        second: blind(1.0),
    };
    let from_plane = || ExtrudeStart::ProfilePlane {};
    let forward = || ExtrudeDirection::ProfileNormal {};
    let reversed = || ExtrudeDirection::ReversedProfileNormal {};
    let offset = |value: f64| ExtrudeStart::OffsetProfilePlane {
        offset: Length::new(value).unwrap(),
    };
    assert_eq!(
        interval(from_plane(), forward(), two_sided()),
        Some((-1.0, 3.0))
    );
    assert_eq!(
        interval(from_plane(), reversed(), two_sided()),
        Some((-3.0, 1.0))
    );
    assert_eq!(
        interval(
            from_plane(),
            forward(),
            ExtrudeExtent::Symmetric { side: blind(4.0) }
        ),
        Some((-2.0, 2.0))
    );
    // A symmetric extent mirrors across the profile plane, whatever the start.
    assert_eq!(
        interval(
            offset(2.0),
            forward(),
            ExtrudeExtent::Symmetric { side: blind(4.0) }
        ),
        Some((-2.0, 2.0))
    );
    assert_eq!(
        interval(
            offset(2.0),
            reversed(),
            ExtrudeExtent::OneSided { side: blind(1.0) }
        ),
        Some((1.0, 2.0))
    );
    assert_eq!(
        interval(
            offset(-2.0),
            forward(),
            ExtrudeExtent::TwoSided {
                first: blind(1.0),
                second: blind(2.0)
            }
        ),
        Some((-4.0, -1.0))
    );
    assert_eq!(
        interval(
            ExtrudeStart::Unresolved {},
            forward(),
            ExtrudeExtent::Symmetric { side: blind(4.0) }
        ),
        None
    );
}

/// The profile names the boundary of the square A and the empty square C
/// together. The material of the body decides only what the Intersect removes:
/// the faces left over are what the named curves enclose.
#[test]
fn an_intersect_keeps_a_region_over_no_material_when_its_profile_names_it() {
    let mut lines = rectangle("outer", 0.0, 3.0).to_vec();
    lines.push(("first", (1.0, 0.0), (1.0, 1.0)));
    lines.push(("second", (2.0, 0.0), (2.0, 1.0)));
    let before = [strip(1, 0.0, 0.0, 2.0), strip(2, -2.0, 0.0, 2.0)];
    let after = [strip(3, 0.0, 1.0, 2.0), strip(4, -2.0, 1.0, 2.0)];
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["first", "right", "bottom", "top"],
        two_down(),
    );
    let Some(PlanarProfileRef::SketchRegions { regions, .. }) = selected else {
        panic!("sketch regions");
    };
    assert_eq!(regions.len(), 2);
    // The curve between A and C is inside the profile, so naming it too
    // leaves the profile without a consistent boundary.
    let (selected, _) = run_intersect(
        &lines,
        &before,
        &after,
        BooleanOp::Intersect,
        &["first", "second", "right", "bottom", "top"],
        two_down(),
    );
    assert!(selected.is_none());
}

/// Material that a plane gains over a removed face after the transition
/// contradicts the removal.
#[test]
fn an_intersect_refuses_material_gained_over_a_removed_region() {
    let mut lines = rectangle("outer", 0.0, 2.0).to_vec();
    lines.push(("middle", (1.0, 0.0), (1.0, 1.0)));
    let named = ["middle", "right", "bottom", "top"];
    let before = [strip(1, 0.0, 0.0, 1.0)];
    let run = |after: &[super::super::tests::PlaneFace]| {
        run_intersect(
            &lines,
            &before,
            after,
            BooleanOp::Intersect,
            &named,
            two_down(),
        )
        .0
    };
    assert!(run(&[]).is_some());
    assert!(run(&[strip(2, -1.0, 0.0, 1.0)]).is_none());
}
