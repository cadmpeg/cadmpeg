// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::design::dimensions::point_lies_on_sketch_geometry;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{Angle, Length, SketchProfileBoundaryUse, SketchProfileRegion};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
};

fn local_arrangement_budget() -> WorkBudget<'static> {
    WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK)
}

#[test]
fn empty_profile_table_arranges_face_around_open_sketch_branch() {
    let sketch_id = SketchId("sketch-with-overhang".into());
    let line = |id: &str, start: Point2, end: Point2, construction: bool| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch_id.clone(),
        construction,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let entities = vec![
        line(
            "bottom",
            Point2::new(0.0, 0.0),
            Point2::new(37.0, 0.0),
            false,
        ),
        line(
            "right",
            Point2::new(31.0, 0.0),
            Point2::new(31.0, 19.0),
            false,
        ),
        line(
            "top",
            Point2::new(31.0, 19.0),
            Point2::new(0.0, 19.0),
            false,
        ),
        line("left", Point2::new(0.0, 19.0), Point2::new(0.0, 0.0), false),
        line(
            "construction",
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 19.0),
            true,
        ),
    ];
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: Vec::new(),
        native_ref: None,
    };
    let arrangement_budget = local_arrangement_budget();

    let Some(SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }) = arrangement_region_containing_points(
        &sketch,
        &entities,
        &[
            Point2::new(0.0, 0.0),
            Point2::new(31.0, 0.0),
            Point2::new(31.0, 19.0),
            Point2::new(0.0, 19.0),
        ],
        1.0e-6,
        &arrangement_budget,
    )
    else {
        panic!("selected face must resolve from raw sketch geometry")
    };
    assert!(hole_boundaries.is_empty());
    let mut boundary_entities = outer_boundary
        .iter()
        .map(|use_| use_.entity.0.as_str())
        .collect::<Vec<_>>();
    boundary_entities.sort_unstable();
    assert_eq!(boundary_entities, ["bottom", "left", "right", "top"]);
}

#[test]
fn historical_point_inside_unique_closed_line_profile_selects_region() {
    let sketch_id = SketchId("sketch".into());
    let mut entities = Vec::new();
    let mut profile = Vec::new();
    for (ordinal, (start, end)) in [
        (Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        (Point2::new(4.0, 0.0), Point2::new(4.0, 3.0)),
        (Point2::new(4.0, 3.0), Point2::new(0.0, 3.0)),
        (Point2::new(0.0, 3.0), Point2::new(0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let id = SketchEntityId(format!("line-{ordinal}"));
        profile.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity {
            id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line { start, end },
        });
    }
    let circle_id = SketchEntityId("unrelated-circle".into());
    let profiles = vec![
        profile,
        vec![SketchEntityUse {
            entity: circle_id.clone(),
            reversed: false,
        }],
    ];
    entities.push(SketchEntity {
        id: circle_id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(20.0, 20.0),
            radius: Length(1.0),
        },
    });
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(10.0, 20.0, 5.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(12.0, 21.0, 12.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: Vec::new(),
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(15.0, 21.0, 12.0)], 1.0e-6,),
        None
    );

    let mut incomplete = sketch.clone();
    let ellipse = SketchEntityId("unsupported-ellipse".into());
    incomplete.profiles.push(vec![SketchEntityUse {
        entity: ellipse.clone(),
        reversed: false,
    }]);
    entities.push(SketchEntity {
        id: ellipse,
        sketch: incomplete.id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Ellipse {
            center: Point2::new(30.0, 30.0),
            major_angle: Angle(0.0),
            major_radius: Length(2.0),
            minor_radius: Length(1.0),
            start_angle: None,
            end_angle: None,
        },
    });
    assert_eq!(
        region_containing_points(
            &incomplete,
            &entities,
            &[Point3::new(12.0, 21.0, 12.0)],
            1.0e-6,
        ),
        None
    );
}

#[test]
fn nested_line_profiles_resolve_atomic_regions_and_immediate_holes() {
    let sketch_id = SketchId("sketch".into());
    let mut entities = Vec::new();
    let mut profiles = Vec::new();
    for (profile_index, (minimum, maximum)) in [
        (Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)),
        (Point2::new(2.0, 2.0), Point2::new(8.0, 8.0)),
        (Point2::new(4.0, 4.0), Point2::new(6.0, 6.0)),
    ]
    .into_iter()
    .enumerate()
    {
        let corners = [
            minimum,
            Point2::new(maximum.u, minimum.v),
            maximum,
            Point2::new(minimum.u, maximum.v),
        ];
        let mut profile = Vec::new();
        for edge_index in 0..corners.len() {
            let id = SketchEntityId(format!("line-{profile_index}-{edge_index}"));
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            entities.push(SketchEntity {
                id,
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: corners[edge_index],
                    end: corners[(edge_index + 1) % corners.len()],
                },
            });
        }
        profiles.push(profile);
    }
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles,
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(3.0, 3.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 1,
            holes: vec![2],
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(5.0, 5.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 2,
            holes: Vec::new(),
        })
    );
    assert_eq!(
        region_containing_points(
            &sketch,
            &entities,
            &[Point3::new(0.0, 5.0, 0.0), Point3::new(2.0, 5.0, 0.0)],
            1.0e-6,
        ),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        })
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(2.0, 5.0, 0.0)], 1.0e-6),
        None
    );
}

#[test]
fn nonperiodic_nurbs_boundary_resolves_atomic_region() {
    let sketch_id = SketchId("sketch".into());
    let definitions = [
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        },
        SketchGeometry::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(10.0, 10.0),
        },
        SketchGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(10.0, 10.0),
                Point2::new(5.0, 12.0),
                Point2::new(0.0, 10.0),
            ],
            weights: Some(vec![1.0, 0.75, 1.0]),
            periodic: false,
        },
        SketchGeometry::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        },
    ];
    let mut entities = Vec::new();
    let outer = definitions
        .into_iter()
        .enumerate()
        .map(|(index, geometry)| {
            let id = SketchEntityId(format!("outer-{index}"));
            entities.push(SketchEntity {
                id: id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry,
            });
            SketchEntityUse {
                entity: id,
                reversed: false,
            }
        })
        .collect::<Vec<_>>();
    let corners = [
        Point2::new(3.0, 3.0),
        Point2::new(7.0, 3.0),
        Point2::new(7.0, 7.0),
        Point2::new(3.0, 7.0),
    ];
    let inner = (0..corners.len())
        .map(|index| {
            let id = SketchEntityId(format!("inner-{index}"));
            entities.push(SketchEntity {
                id: id.clone(),
                sketch: sketch_id.clone(),
                construction: false,
                native_ref: None,
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: SketchGeometry::Line {
                    start: corners[index],
                    end: corners[(index + 1) % corners.len()],
                },
            });
            SketchEntityUse {
                entity: id,
                reversed: false,
            }
        })
        .collect::<Vec<_>>();
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![outer, inner],
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6),
        Some(SketchProfileRegion::Loops {
            outer: 0,
            holes: vec![1],
        })
    );
}

fn coincident_circle_arc_arrangement() -> (Sketch, Vec<SketchEntity>, SketchEntityId, SketchEntityId)
{
    let sketch_id = SketchId("sketch".into());
    let line_id = SketchEntityId("diameter".into());
    let arc_id = SketchEntityId("left-arc".into());
    let circle_id = SketchEntityId("circle".into());
    let entity = |id, geometry| SketchEntity {
        id,
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let entities = vec![
        entity(
            line_id.clone(),
            SketchGeometry::Line {
                start: Point2::new(0.0, -1.0),
                end: Point2::new(0.0, 1.0),
            },
        ),
        entity(
            arc_id.clone(),
            SketchGeometry::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
                start_angle: Angle(std::f64::consts::FRAC_PI_2),
                end_angle: Angle(3.0 * std::f64::consts::FRAC_PI_2),
            },
        ),
        entity(
            circle_id.clone(),
            SketchGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length(1.0),
            },
        ),
    ];
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![
            vec![
                SketchEntityUse {
                    entity: line_id.clone(),
                    reversed: false,
                },
                SketchEntityUse {
                    entity: arc_id.clone(),
                    reversed: false,
                },
            ],
            vec![SketchEntityUse {
                entity: circle_id,
                reversed: false,
            }],
        ],
        native_ref: None,
    };
    (sketch, entities, line_id, arc_id)
}

#[test]
fn coincident_circle_arc_arrangement_resolves_trimmed_faces() {
    let (sketch, entities, line_id, arc_id) = coincident_circle_arc_arrangement();
    let arrangement_budget = local_arrangement_budget();
    let faces = sketch_arrangement_faces(&sketch, &entities, 1.0e-7, &arrangement_budget)
        .expect("endpoint arrangement faces");
    assert_eq!(faces.len(), 2);
    let selected = arrangement_region_containing_points(
        &sketch,
        &entities,
        &[
            Point2::new(0.0, -1.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
        ],
        1.0e-7,
        &arrangement_budget,
    )
    .expect("left half-disk arrangement face");
    let SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    } = selected
    else {
        panic!("arrangement selection must emit a trimmed boundary")
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 2);
    assert!(outer_boundary.iter().any(|use_| use_.entity == line_id));
    assert!(outer_boundary.iter().any(|use_| use_.entity == arc_id));
    assert!(outer_boundary.iter().all(|use_| matches!(
        use_,
        SketchProfileBoundaryUse {
            parameter_range: [start, end],
            ..
        } if start != end
    )));
}

#[test]
fn sketch_arrangement_faces_declines_when_session_work_budget_is_exhausted() {
    let (sketch, entities, _, _) = coincident_circle_arc_arrangement();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("root context for session work budget");
    let budget = ctx.work_budget(MAX_ARRANGEMENT_WALK_WORK as u64);

    assert!(sketch_arrangement_faces(&sketch, &entities, 1.0e-7, &budget).is_none());
    assert!(budget.exhausted());
}

#[test]
fn analytic_arrangement_intersections_include_hidden_second_crossing() {
    let line = ProfileBoundarySegment::Line {
        start: Point2::new(-2.0, 0.0),
        end: Point2::new(2.0, 0.0),
    };
    let circle = ProfileBoundarySegment::Arc {
        center: Point2::new(0.0, 0.0),
        radius: 1.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::TAU,
    };

    assert_eq!(
        analytic_segment_intersections(&line, &circle)
            .expect("analytic intersection family")
            .len(),
        2
    );
}

#[test]
fn polygon_and_circle_boundaries_resolve_one_atomic_region() {
    let sketch_id = SketchId("sketch".into());
    let corners = [
        Point2::new(-5.0, -5.0),
        Point2::new(5.0, -5.0),
        Point2::new(5.0, 5.0),
        Point2::new(-5.0, 5.0),
    ];
    let mut entities = Vec::new();
    let mut outer = Vec::new();
    for index in 0..corners.len() {
        let id = SketchEntityId(format!("line-{index}"));
        outer.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity {
            id,
            sketch: sketch_id.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Line {
                start: corners[index],
                end: corners[(index + 1) % corners.len()],
            },
        });
    }
    let circle = SketchEntityId("circle".into());
    entities.push(SketchEntity {
        id: circle.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    });
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![
            outer,
            vec![SketchEntityUse {
                entity: circle,
                reversed: false,
            }],
        ],
        native_ref: None,
    };
    let expected = SketchProfileRegion::Loops {
        outer: 0,
        holes: vec![1],
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(4.0, 0.0, 0.0)], 1.0e-6,),
        Some(expected.clone())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(0.0, 0.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::Loops {
            outer: 1,
            holes: Vec::new(),
        })
    );
}

#[test]
fn circular_arc_loop_uses_analytic_containment_and_distance() {
    let segments = vec![
        ProfileBoundarySegment::Line {
            start: Point2::new(0.0, -2.0),
            end: Point2::new(0.0, 2.0),
        },
        ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 2.0,
            start_angle: std::f64::consts::FRAC_PI_2,
            end_angle: 3.0 * std::f64::consts::FRAC_PI_2,
        },
    ];
    let boundary = ProfileBoundary::CircularArcLoop(segments);
    let hole = ProfileBoundary::Circle {
        center: Point2::new(-1.0, 0.0),
        radius: 0.5,
    };

    assert!(boundary.contains_point(Point2::new(-1.0, 0.0)));
    assert!(!boundary.contains_point(Point2::new(1.0, 0.0)));
    assert!(!boundary.contains_point(Point2::new(-1.0, -2.0)));
    assert!(!boundary.contains_point(Point2::new(-1.0, 2.0)));
    assert!(boundary.strictly_contains(&hole));
}

#[test]
fn polygon_and_arc_loop_containment_requires_disjoint_boundaries() {
    let polygon = ProfileBoundary::Polygon(vec![
        Point2::new(-2.0, -2.0),
        Point2::new(2.0, -2.0),
        Point2::new(2.0, 2.0),
        Point2::new(-2.0, 2.0),
    ]);
    let arc_loop = ProfileBoundary::CircularArcLoop(vec![
        ProfileBoundarySegment::Line {
            start: Point2::new(-1.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
        ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        },
    ]);

    assert!(polygon.strictly_contains(&arc_loop));
    assert!(!arc_loop.strictly_contains(&polygon));

    let crossing = ProfileBoundary::CircularArcLoop(vec![
        ProfileBoundarySegment::Line {
            start: Point2::new(-3.0, 0.0),
            end: Point2::new(3.0, 0.0),
        },
        ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 3.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::PI,
        },
    ]);
    assert!(!polygon.strictly_contains(&crossing));
    assert!(!crossing.strictly_contains(&polygon));
}

#[test]
fn arc_loop_containment_rejects_crossing_and_touching_segments() {
    let d_loop = |center_u: f64, radius: f64| {
        ProfileBoundary::CircularArcLoop(vec![
            ProfileBoundarySegment::Line {
                start: Point2::new(center_u, -radius),
                end: Point2::new(center_u, radius),
            },
            ProfileBoundarySegment::Arc {
                center: Point2::new(center_u, 0.0),
                radius,
                start_angle: std::f64::consts::FRAC_PI_2,
                end_angle: 3.0 * std::f64::consts::FRAC_PI_2,
            },
        ])
    };
    let outer = d_loop(0.0, 2.0);
    let inner = d_loop(-0.5, 0.5);
    let crossing = d_loop(-1.5, 1.0);
    let touching = d_loop(-1.0, 1.0);

    assert!(outer.strictly_contains(&inner));
    assert!(!inner.strictly_contains(&outer));
    assert!(!outer.strictly_contains(&crossing));
    assert!(!outer.strictly_contains(&touching));
}

#[test]
fn historical_edge_positions_require_a_complete_state_chain() {
    let mut topology = crate::history_records::AsmHistoricalTopology {
        edges: vec![7],
        vertices: vec![8, 9],
        points: vec![18, 19],
        edge_vertices: vec![crate::history_records::AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        vertex_points: vec![
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 18,
            },
            crate::history_records::AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 19,
            },
        ],
        point_positions: vec![
            crate::history_records::AsmHistoricalPoint {
                point: 18,
                position: Point3::new(1.0, 2.0, 3.0),
            },
            crate::history_records::AsmHistoricalPoint {
                point: 19,
                position: Point3::new(4.0, 5.0, 6.0),
            },
        ],
        ..crate::history_records::AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::design::geometry::historical_entity_positions(
            crate::records::AsmHistoricalEntityKind::Edge,
            7,
            &topology,
        ),
        Some(vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0),])
    );
    topology.point_positions.pop();
    assert_eq!(
        crate::design::geometry::historical_entity_positions(
            crate::records::AsmHistoricalEntityKind::Edge,
            7,
            &topology,
        ),
        None
    );
}

#[test]
fn historical_region_faces_follow_complete_ownership_hierarchy() {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    use crate::records::AsmHistoricalEntityKind;

    let topology = AsmHistoricalTopology {
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3, 4],
        }],
        shell_faces: vec![
            AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![7, 5],
            },
            AsmHistoricalRelation {
                owner_ref: 4,
                member_refs: vec![6, 7],
            },
        ],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        crate::design::geometry::historical_owned_faces(
            AsmHistoricalEntityKind::Body,
            1,
            &topology
        ),
        Some(vec![5, 6, 7])
    );
    assert_eq!(
        crate::design::geometry::historical_owned_faces(
            AsmHistoricalEntityKind::Region,
            2,
            &topology
        ),
        Some(vec![5, 6, 7])
    );
    assert_eq!(
        crate::design::geometry::historical_owned_faces(
            AsmHistoricalEntityKind::Shell,
            3,
            &topology
        ),
        Some(vec![5, 7])
    );
}

#[test]
fn historical_point_membership_respects_conic_domains_and_nurbs_endpoints() {
    let sketch = SketchId("sketch".into());
    let entity = |geometry| SketchEntity {
        id: SketchEntityId("curve".into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry,
    };
    let arc = entity(SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(2.0),
        start_angle: cadmpeg_ir::features::Angle(0.0),
        end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
    });
    assert!(point_on_sketch_entity(Point2::new(0.0, 2.0), &arc, 1.0e-6));
    assert!(!point_on_sketch_entity(
        Point2::new(-2.0, 0.0),
        &arc,
        1.0e-6
    ));
    let clockwise_arc = entity(SketchGeometry::Arc {
        center: Point2::new(0.0, 0.0),
        radius: Length(2.0),
        start_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        end_angle: cadmpeg_ir::features::Angle(0.0),
    });
    assert!(point_lies_on_sketch_geometry(
        Point2::new(std::f64::consts::SQRT_2, std::f64::consts::SQRT_2),
        &clockwise_arc.geometry
    ));
    assert!(!point_lies_on_sketch_geometry(
        Point2::new(-2.0, 0.0),
        &clockwise_arc.geometry
    ));

    let ellipse = entity(SketchGeometry::Ellipse {
        center: Point2::new(1.0, -1.0),
        major_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
        major_radius: Length(4.0),
        minor_radius: Length(2.0),
        start_angle: Some(cadmpeg_ir::features::Angle(0.0)),
        end_angle: Some(cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2)),
    });
    assert!(point_on_sketch_entity(
        Point2::new(-1.0, -1.0),
        &ellipse,
        1.0e-6
    ));
    assert!(!point_on_sketch_entity(
        Point2::new(3.0, -1.0),
        &ellipse,
        1.0e-6
    ));
    assert!(!point_on_sketch_entity(
        Point2::new(-1.0, -0.9),
        &ellipse,
        1.0e-6
    ));

    let nurbs = entity(SketchGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(1.0, 2.0),
            Point2::new(2.0, 4.0),
            Point2::new(3.0, 2.0),
        ],
        weights: Some(vec![1.0, 0.5, 1.0]),
        periodic: false,
    });
    assert!(point_on_sketch_entity(
        Point2::new(3.0, 2.0),
        &nurbs,
        1.0e-6
    ));
    assert!(!point_on_sketch_entity(
        Point2::new(2.0, 4.0),
        &nurbs,
        1.0e-6
    ));
    let SketchGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        ..
    } = &nurbs.geometry
    else {
        unreachable!()
    };
    let interior = cadmpeg_ir::eval::nurbs_pcurve_uv(
        *degree,
        knots,
        control_points,
        weights.as_deref(),
        0.375,
    )
    .unwrap();
    assert!(point_on_sketch_entity(interior, &nurbs, 1.0e-9));
}

#[test]
fn unbranched_closed_sketch_components_project_as_ordered_profiles() {
    let sketch = SketchId("f3d:model:sketch#profile".into());
    let line = |id: &str, start: Point2, end: Point2| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line { start, end },
    };
    let entities = vec![
        line("line-a", Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)),
        line("line-b", Point2::new(2.0, 2.0), Point2::new(2.0, 0.0)),
        line("line-c", Point2::new(2.0, 2.0), Point2::new(0.0, 2.0)),
        line(
            "line-d",
            Point2::new(0.0, 2.0 + 5.0e-7),
            Point2::new(0.0, 0.0),
        ),
        line("open-line", Point2::new(10.0, 0.0), Point2::new(11.0, 0.0)),
        SketchEntity {
            id: SketchEntityId("circle".into()),
            sketch: sketch.clone(),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Circle {
                center: Point2::new(20.0, 20.0),
                radius: Length(3.0),
            },
        },
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(profiles[0][0].entity, SketchEntityId("circle".into()));
    assert_eq!(
        profiles[1]
            .iter()
            .map(|entity_use| (entity_use.entity.0.as_str(), entity_use.reversed))
            .collect::<Vec<_>>(),
        [
            ("line-a", false),
            ("line-b", true),
            ("line-c", false),
            ("line-d", false),
        ]
    );
}

#[test]
fn branched_line_graph_projects_each_bounded_face() {
    let sketch = SketchId("f3d:model:sketch#branched-profile".into());
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    };
    let entities = vec![
        line("bottom-left", (0.0, 0.0), (1.0, 0.0)),
        line("bottom-right", (1.0, 0.0), (2.0, 0.0)),
        line("right", (2.0, 0.0), (2.0, 1.0)),
        line("top-right", (2.0, 1.0), (1.0, 1.0)),
        line("top-left", (1.0, 1.0), (0.0, 1.0)),
        line("left", (0.0, 1.0), (0.0, 0.0)),
        line("divider", (1.0, 0.0), (1.0, 1.0)),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(profiles.len(), 2);
    assert!(profiles.iter().all(|profile| profile.len() == 4));
    assert!(profiles.iter().all(|profile| profile
        .iter()
        .any(|entity_use| entity_use.entity.0 == "divider")));
}

#[test]
fn branched_line_graph_with_a_shared_corner_projects_bounded_faces() {
    let sketch = SketchId("f3d:model:sketch#shared-corner-profile".into());
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| SketchEntity {
        id: SketchEntityId(id.into()),
        sketch: sketch.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    };
    let entities = vec![
        line("outer-bottom", (0.0, 0.0), (31.0, 0.0)),
        line("outer-right", (31.0, 0.0), (31.0, 47.0)),
        line("outer-top", (31.0, 47.0), (0.0, 47.0)),
        line("outer-left", (0.0, 47.0), (0.0, 0.0)),
        line("inner-top", (0.0, 47.0), (9.0, 47.0)),
        line("inner-right", (9.0, 47.0), (9.0, 41.0)),
        line("inner-bottom", (9.0, 41.0), (0.0, 41.0)),
        line("inner-left", (0.0, 41.0), (0.0, 47.0)),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(
        profiles
            .iter()
            .flat_map(|profile| profile
                .iter()
                .map(|entity_use| (entity_use.entity.0.as_str(), entity_use.reversed)))
            .collect::<Vec<_>>(),
        [
            ("outer-left", false),
            ("outer-bottom", false),
            ("outer-right", false),
            ("outer-top", false),
            ("inner-top", false),
            ("inner-right", false),
            ("inner-bottom", false),
            ("inner-left", false),
        ]
    );
}
