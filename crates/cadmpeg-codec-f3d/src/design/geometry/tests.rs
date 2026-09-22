// SPDX-License-Identifier: Apache-2.0

use super::{
    analytic_segment_intersections, angle_strictly_inside_arc,
    arrangement_region_containing_points, closed_sketch_profiles, point_on_sketch_entity,
    region_containing_points, sketch_arrangement_faces, ProfileBoundary, ProfileBoundarySegment,
    MAX_ARRANGEMENT_WALK_WORK,
};
use crate::design::dimensions::point_lies_on_sketch_geometry;
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::SketchProfileRegion;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};

fn local_arrangement_budget() -> WorkBudget<'static> {
    WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK)
}

#[test]
fn empty_profile_table_arranges_face_around_open_sketch_branch() {
    let sketch_id = SketchId::mint("synthetic:test:id#sketch-with-overhang").unwrap();
    let line = |id: &str, start: Point2, end: Point2, construction: bool| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
        .with_construction(construction)
    };
    let entities = vec![
        line(
            "synthetic:test:id#bottom",
            Point2::new(0.0, 0.0),
            Point2::new(37.0, 0.0),
            false,
        ),
        line(
            "synthetic:test:id#right",
            Point2::new(31.0, 0.0),
            Point2::new(31.0, 19.0),
            false,
        ),
        line(
            "synthetic:test:id#top",
            Point2::new(31.0, 19.0),
            Point2::new(0.0, 19.0),
            false,
        ),
        line(
            "synthetic:test:id#left",
            Point2::new(0.0, 19.0),
            Point2::new(0.0, 0.0),
            false,
        ),
        line(
            "synthetic:test:id#construction",
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
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
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
        .map(|use_| use_.entity.as_str())
        .collect::<Vec<_>>();
    boundary_entities.sort_unstable();
    assert_eq!(
        boundary_entities,
        [
            "synthetic:test:id#bottom",
            "synthetic:test:id#left",
            "synthetic:test:id#right",
            "synthetic:test:id#top"
        ]
    );
}

#[test]
fn historical_point_inside_unique_closed_line_profile_selects_region() {
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
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
        let id = SketchEntityId::mint(format!("synthetic:test:id#line-{ordinal}")).unwrap();
        profile.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity::new(
            id,
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        ));
    }
    let circle_id = SketchEntityId::mint("synthetic:test:id#unrelated-circle").unwrap();
    let profiles = vec![
        profile,
        vec![SketchEntityUse {
            entity: circle_id.clone(),
            reversed: false,
        }],
    ];
    entities.push(SketchEntity::new(
        circle_id,
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(20.0, 20.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    ));
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(10.0, 20.0, 5.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap(),
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(12.0, 21.0, 12.0)], 1.0e-6,),
        Some(SketchProfileRegion::loops(0, Vec::new()).unwrap())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(15.0, 21.0, 12.0)], 1.0e-6,),
        None
    );

    let mut incomplete = sketch.clone();
    let ellipse = SketchEntityId::mint("synthetic:test:id#unsupported-ellipse").unwrap();
    incomplete.profiles.push_single(SketchEntityUse {
        entity: ellipse.clone(),
        reversed: false,
    });
    entities.push(SketchEntity::new(
        ellipse,
        incomplete.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(30.0, 30.0),
            major_angle: Angle::new(0.0).unwrap(),
            major_radius: Length::new(2.0).unwrap(),
            minor_radius: Length::new(1.0).unwrap(),
            bounds: None,
        })
        .unwrap(),
    ));
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
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
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
            let id = SketchEntityId::mint(format!(
                "synthetic:test:id#line-{profile_index}-{edge_index}"
            ))
            .unwrap();
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            entities.push(SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: corners[edge_index],
                    end: corners[(edge_index + 1) % corners.len()],
                })
                .unwrap(),
            ));
        }
        profiles.push(profile);
    }
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap(),
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::loops(0, vec![1]).unwrap())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(3.0, 3.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::loops(1, vec![2]).unwrap())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(5.0, 5.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::loops(2, Vec::new()).unwrap())
    );
    assert_eq!(
        region_containing_points(
            &sketch,
            &entities,
            &[Point3::new(0.0, 5.0, 0.0), Point3::new(2.0, 5.0, 0.0)],
            1.0e-6,
        ),
        Some(SketchProfileRegion::loops(0, vec![1]).unwrap())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(2.0, 5.0, 0.0)], 1.0e-6),
        None
    );
}

#[test]
fn nonperiodic_nurbs_boundary_resolves_atomic_region() {
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let definitions = [
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(10.0, 0.0),
            end: Point2::new(10.0, 10.0),
        })
        .unwrap(),
        SketchGeometry::nurbs(
            cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![
                    Point2::new(10.0, 10.0),
                    Point2::new(5.0, 12.0),
                    Point2::new(0.0, 10.0),
                ],
                Some(vec![1.0, 0.75, 1.0]),
                false,
            )
            .unwrap(),
        ),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 10.0),
            end: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    ];
    let mut entities = Vec::new();
    let outer = definitions
        .into_iter()
        .enumerate()
        .map(|(index, geometry)| {
            let id = SketchEntityId::mint(format!("synthetic:test:id#outer-{index}")).unwrap();
            entities.push(SketchEntity::new(id.clone(), sketch_id.clone(), geometry));
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
            let id = SketchEntityId::mint(format!("synthetic:test:id#inner-{index}")).unwrap();
            entities.push(SketchEntity::new(
                id.clone(),
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: corners[index],
                    end: corners[(index + 1) % corners.len()],
                })
                .unwrap(),
            ));
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
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![outer, inner]).unwrap(),
        native_ref: None,
    };

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(1.0, 1.0, 0.0)], 1.0e-6),
        Some(SketchProfileRegion::loops(0, vec![1]).unwrap())
    );
}

fn coincident_circle_arc_arrangement() -> (Sketch, Vec<SketchEntity>, SketchEntityId, SketchEntityId)
{
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line_id = SketchEntityId::mint("synthetic:test:id#diameter").unwrap();
    let arc_id = SketchEntityId::mint("synthetic:test:id#left-arc").unwrap();
    let circle_id = SketchEntityId::mint("synthetic:test:id#circle").unwrap();
    let entity = |id, geometry| SketchEntity::new(id, sketch_id.clone(), geometry);
    let entities = vec![
        entity(
            line_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, -1.0),
                end: Point2::new(0.0, 1.0),
            })
            .unwrap(),
        ),
        entity(
            arc_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(1.0).unwrap(),
                start_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
                end_angle: Angle::new(3.0 * std::f64::consts::FRAC_PI_2).unwrap(),
            })
            .unwrap(),
        ),
        entity(
            circle_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(1.0).unwrap(),
            })
            .unwrap(),
        ),
    ];
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![
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
        ])
        .unwrap(),
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
    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let corners = [
        Point2::new(-5.0, -5.0),
        Point2::new(5.0, -5.0),
        Point2::new(5.0, 5.0),
        Point2::new(-5.0, 5.0),
    ];
    let mut entities = Vec::new();
    let mut outer = Vec::new();
    for index in 0..corners.len() {
        let id = SketchEntityId::mint(format!("synthetic:test:id#line-{index}")).unwrap();
        outer.push(SketchEntityUse {
            entity: id.clone(),
            reversed: false,
        });
        entities.push(SketchEntity::new(
            id,
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: corners[index],
                end: corners[(index + 1) % corners.len()],
            })
            .unwrap(),
        ));
    }
    let circle = SketchEntityId::mint("synthetic:test:id#circle").unwrap();
    entities.push(SketchEntity::new(
        circle.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        })
        .unwrap(),
    ));
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![
            outer,
            vec![SketchEntityUse {
                entity: circle,
                reversed: false,
            }],
        ])
        .unwrap(),
        native_ref: None,
    };
    let expected = SketchProfileRegion::loops(0, vec![1]).unwrap();

    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(4.0, 0.0, 0.0)], 1.0e-6,),
        Some(expected.clone())
    );
    assert_eq!(
        region_containing_points(&sketch, &entities, &[Point3::new(0.0, 0.0, 0.0)], 1.0e-6,),
        Some(SketchProfileRegion::loops(1, Vec::new()).unwrap())
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
            crate::records::topology::body_recipe::AsmHistoricalEntityKind::Edge,
            7,
            &topology,
        ),
        Some(vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0),])
    );
    topology.point_positions.pop();
    assert_eq!(
        crate::design::geometry::historical_entity_positions(
            crate::records::topology::body_recipe::AsmHistoricalEntityKind::Edge,
            7,
            &topology,
        ),
        None
    );
}

#[test]
fn historical_region_faces_follow_complete_ownership_hierarchy() {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

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
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entity = |geometry| {
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#curve").unwrap(),
            sketch.clone(),
            geometry,
        )
    };
    let arc = entity(
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
    );
    assert!(point_on_sketch_entity(Point2::new(0.0, 2.0), &arc, 1.0e-6));
    assert!(!point_on_sketch_entity(
        Point2::new(-2.0, 0.0),
        &arc,
        1.0e-6
    ));
    let clockwise_arc = entity(
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(point_lies_on_sketch_geometry(
        Point2::new(std::f64::consts::SQRT_2, std::f64::consts::SQRT_2),
        &clockwise_arc.geometry
    ));
    assert!(!point_lies_on_sketch_geometry(
        Point2::new(-2.0, 0.0),
        &clockwise_arc.geometry
    ));

    let ellipse = entity(
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(1.0, -1.0),
            major_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            major_radius: Length::new(4.0).unwrap(),
            minor_radius: Length::new(2.0).unwrap(),
            bounds: Some([
                cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
                cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            ]),
        })
        .unwrap(),
    );
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

    let nurbs = entity(SketchGeometry::nurbs(
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point2::new(1.0, 2.0),
                Point2::new(2.0, 4.0),
                Point2::new(3.0, 2.0),
            ],
            Some(vec![1.0, 0.5, 1.0]),
            false,
        )
        .unwrap(),
    ));
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
    let SketchGeometryDefinition::Nurbs { curve } = nurbs.geometry.definition() else {
        unreachable!()
    };
    let control_points = curve.control_points();
    let weights = curve.weights();
    let interior = cadmpeg_ir::eval::nurbs_pcurve_uv(
        curve.degree(),
        curve.knots(),
        &control_points,
        weights.as_deref(),
        0.375,
    )
    .unwrap();
    assert!(point_on_sketch_entity(interior, &nurbs, 1.0e-9));
}

#[test]
fn unbranched_closed_sketch_components_project_as_ordered_profiles() {
    let sketch = SketchId::mint("f3d:model:sketch#profile").unwrap();
    let line = |id: &str, start: Point2, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let entities = vec![
        line(
            "synthetic:test:id#line-a",
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
        ),
        line(
            "synthetic:test:id#line-b",
            Point2::new(2.0, 2.0),
            Point2::new(2.0, 0.0),
        ),
        line(
            "synthetic:test:id#line-c",
            Point2::new(2.0, 2.0),
            Point2::new(0.0, 2.0),
        ),
        line(
            "synthetic:test:id#line-d",
            Point2::new(0.0, 2.0 + 5.0e-7),
            Point2::new(0.0, 0.0),
        ),
        line(
            "synthetic:test:id#open-line",
            Point2::new(10.0, 0.0),
            Point2::new(11.0, 0.0),
        ),
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#circle").unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(20.0, 20.0),
                radius: Length::new(3.0).unwrap(),
            })
            .unwrap(),
        ),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(
        profiles[0][0].entity,
        SketchEntityId::mint("synthetic:test:id#circle").unwrap()
    );
    assert_eq!(
        profiles[1]
            .iter()
            .map(|entity_use| (entity_use.entity.as_str(), entity_use.reversed))
            .collect::<Vec<_>>(),
        [
            ("synthetic:test:id#line-a", false),
            ("synthetic:test:id#line-b", true),
            ("synthetic:test:id#line-c", false),
            ("synthetic:test:id#line-d", false),
        ]
    );
}

#[test]
fn branched_line_graph_projects_each_bounded_face() {
    let sketch = SketchId::mint("f3d:model:sketch#branched-profile").unwrap();
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        line("synthetic:test:id#bottom-left", (0.0, 0.0), (1.0, 0.0)),
        line("synthetic:test:id#bottom-right", (1.0, 0.0), (2.0, 0.0)),
        line("synthetic:test:id#right", (2.0, 0.0), (2.0, 1.0)),
        line("synthetic:test:id#top-right", (2.0, 1.0), (1.0, 1.0)),
        line("synthetic:test:id#top-left", (1.0, 1.0), (0.0, 1.0)),
        line("synthetic:test:id#left", (0.0, 1.0), (0.0, 0.0)),
        line("synthetic:test:id#divider", (1.0, 0.0), (1.0, 1.0)),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(profiles.len(), 2);
    assert!(profiles.iter().all(|profile| profile.len() == 4));
    assert!(profiles.iter().all(|profile| profile
        .iter()
        .any(|entity_use| entity_use.entity.as_str() == "synthetic:test:id#divider")));
}

#[test]
fn branched_line_graph_with_a_shared_corner_projects_bounded_faces() {
    let sketch = SketchId::mint("f3d:model:sketch#shared-corner-profile").unwrap();
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        line("synthetic:test:id#outer-bottom", (0.0, 0.0), (31.0, 0.0)),
        line("synthetic:test:id#outer-right", (31.0, 0.0), (31.0, 47.0)),
        line("synthetic:test:id#outer-top", (31.0, 47.0), (0.0, 47.0)),
        line("synthetic:test:id#outer-left", (0.0, 47.0), (0.0, 0.0)),
        line("synthetic:test:id#inner-top", (0.0, 47.0), (9.0, 47.0)),
        line("synthetic:test:id#inner-right", (9.0, 47.0), (9.0, 41.0)),
        line("synthetic:test:id#inner-bottom", (9.0, 41.0), (0.0, 41.0)),
        line("synthetic:test:id#inner-left", (0.0, 41.0), (0.0, 47.0)),
    ];

    let profiles = closed_sketch_profiles(&sketch, &entities, 1.0e-6);
    assert_eq!(
        profiles
            .iter()
            .flat_map(|profile| profile
                .iter()
                .map(|entity_use| (entity_use.entity.as_str(), entity_use.reversed)))
            .collect::<Vec<_>>(),
        [
            ("synthetic:test:id#outer-left", false),
            ("synthetic:test:id#outer-bottom", false),
            ("synthetic:test:id#outer-right", false),
            ("synthetic:test:id#outer-top", false),
            ("synthetic:test:id#inner-top", false),
            ("synthetic:test:id#inner-right", false),
            ("synthetic:test:id#inner-bottom", false),
            ("synthetic:test:id#inner-left", false),
        ]
    );
}

#[test]
fn an_arc_shorter_than_twice_the_tolerance_has_no_strict_interior() {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    let tolerance = 0.1;

    // A unit quarter arc is much longer than twice the tolerance, so its
    // midpoint lies strictly inside it and both endpoints do not.
    assert!(angle_strictly_inside_arc(
        FRAC_PI_2 / 2.0,
        0.0,
        FRAC_PI_2,
        1.0,
        tolerance
    ));
    assert!(!angle_strictly_inside_arc(
        0.0, 0.0, FRAC_PI_2, 1.0, tolerance
    ));
    assert!(!angle_strictly_inside_arc(
        FRAC_PI_2, 0.0, FRAC_PI_2, 1.0, tolerance
    ));

    // Exactly twice the tolerance: the interval closes and its own midpoint is
    // outside. The boundary answers false; it does not answer with a capped
    // fraction of the span.
    let end = 2.0 * tolerance;
    assert!(!angle_strictly_inside_arc(
        end / 2.0,
        0.0,
        end,
        1.0,
        tolerance
    ));
    // A hair longer, and the midpoint is strictly inside again.
    let end = 2.0 * tolerance * (1.0 + 1e-9);
    assert!(angle_strictly_inside_arc(
        end / 2.0,
        0.0,
        end,
        1.0,
        tolerance
    ));

    // A tiny radius shortens the arc the same way a tiny angular span does.
    assert!(!angle_strictly_inside_arc(PI, 0.0, TAU, 1e-9, tolerance));

    // A zero tolerance leaves every interior angle strictly inside, and the
    // endpoints outside, with no division by a capped span.
    assert!(angle_strictly_inside_arc(FRAC_PI_2, 0.0, PI, 1.0, 0.0));
    assert!(!angle_strictly_inside_arc(0.0, 0.0, PI, 1.0, 0.0));
}

#[test]
fn numerical_audit_line_circle_intersections_are_scale_invariant() {
    for radius in [1.0e-150, 1.0e-4, 1.0, 1.0e150] {
        let circle = ProfileBoundarySegment::Arc {
            center: Point2::new(0.0, 0.0),
            radius,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        };
        assert!(super::line_arc_intersection_points(
            (
                Point2::new(-radius, 2.0 * radius),
                Point2::new(radius, 2.0 * radius)
            ),
            &circle
        )
        .unwrap()
        .is_empty());
        let tangent = super::line_arc_intersection_points(
            (Point2::new(-radius, radius), Point2::new(radius, radius)),
            &circle,
        )
        .unwrap();
        assert_eq!(tangent, vec![Point2::new(0.0, radius)]);
    }
}

#[test]
fn numerical_followup_profile_speed_bound_retains_common_weights() {
    for weight in [1.0, 1e-200, 1e200] {
        let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            vec![Point2::new(0., 0.), Point2::new(1., 0.)],
            Some(vec![weight; 2]),
            false,
        )
        .unwrap();
        assert_eq!(super::nurbs_speed_bound(&curve), Some(1.0));
    }
}

#[test]
fn numerical_followup_boolean_line_arc_matches_point_intersections() {
    let radius = 1e-4;
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius,
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    let line = ProfileBoundarySegment::Line {
        start: Point2::new(-radius, 2. * radius),
        end: Point2::new(radius, 2. * radius),
    };
    assert!(!super::boundary_segments_intersect(&line, &arc));
    let point = ProfileBoundarySegment::Line {
        start: Point2::new(radius, 0.),
        end: Point2::new(radius, 0.),
    };
    assert!(super::boundary_segments_intersect(&point, &arc));
}

#[test]
fn scaled_planar_intersections_preserve_separation_and_witnesses() {
    for r in [1e-200, 1e-100, 1., 1e200] {
        let a = Point2::new(0., 0.);
        let b = Point2::new(r, 0.);
        assert!(!super::segments_intersect(
            (a, b),
            (Point2::new(0., 2. * r), Point2::new(r, 2. * r))
        ));
        assert!(super::segments_intersect(
            (a, Point2::new(r, r)),
            (Point2::new(0., r), b)
        ));
        assert_eq!(super::point_distance(a, b), r);
        assert_eq!(
            super::point_segment_distance(Point2::new(0.5 * r, r), (a, b)),
            r
        );
        let arc = |x| ProfileBoundarySegment::Arc {
            center: Point2::new(x, 0.),
            radius: r,
            start_angle: 0.,
            end_angle: std::f64::consts::TAU,
        };
        let points = super::arc_intersection_points(&arc(0.), &arc(r)).unwrap();
        assert_eq!(points.len(), 2);
        assert!(super::boundary_segments_intersect(&arc(0.), &arc(r)));
        for point in points {
            assert!((point.u / r - 0.5).abs() <= 4. * f64::EPSILON);
        }
    }
}

#[test]
fn disparate_segment_and_point_scales_preserve_distance() {
    let segment = (Point2::new(0., 0.), Point2::new(1e-200, 0.));
    assert_eq!(
        super::point_segment_distance(Point2::new(0., 1e200), segment),
        1e200
    );
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: 1e-6,
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    assert!(super::line_arc_intersection_points(
        (Point2::new(-1e200, 1.), Point2::new(1e200, 1.)),
        &arc
    )
    .unwrap()
    .is_empty());
}

const LARGE_LINE_TOLERANCE: f64 = 1e-6;
#[test]
fn numerical_0922b_line_arc_crossings() {
    let arc = ProfileBoundarySegment::Arc {
        center: Point2::new(0., 0.),
        radius: 0.001,
        start_angle: 0.,
        end_angle: std::f64::consts::TAU,
    };
    for half in [0.001, 1., 1e4] {
        let points = super::line_arc_intersection_points(
            (Point2::new(-half, 0.), Point2::new(half, 0.)),
            &arc,
        )
        .unwrap();
        println!("Fusion line[-{half},{half}],r=.001 => {points:?}");
        assert_eq!(
            points,
            vec![Point2::new(-0.001, 0.), Point2::new(0.001, 0.)]
        );
    }
}
#[test]
fn numerical_0922b_large_line_crossing() {
    for s in [1., 1e200] {
        let a = ProfileBoundarySegment::Line {
            start: Point2::new(-s, 0.),
            end: Point2::new(s, 0.),
        };
        let b = ProfileBoundarySegment::Line {
            start: Point2::new(0., -s),
            end: Point2::new(0., s),
        };
        let r = analytic_segment_intersections(&a, &b).unwrap();
        println!("Fusion crossing scale{s:e}: {r:?}");
        assert_eq!(r, vec![Point2::new(0., 0.)]);
    }
}
#[test]
fn numerical_0922b_large_line_split() {
    for scale in [1., 1e200] {
        let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0., 0.),
            end: Point2::new(scale, 0.),
        })
        .unwrap();
        let r = super::arrangement_split_parameters(
            &line,
            [0., 1.],
            &[Point2::new(0.5 * scale, 0.)],
            LARGE_LINE_TOLERANCE,
        )
        .unwrap();
        println!("Fusion split scale{scale:e}: {r:?}");
        assert_eq!(r, vec![0., 0.5, 1.]);
    }
}
#[test]
fn numerical_0922b_wide_segment_incidence() {
    for scale in [1., 1e308] {
        let r = super::point_segment_distance(
            Point2::new(0., 0.),
            (Point2::new(-scale, 0.), Point2::new(scale, 0.)),
        );
        println!("Fusion segment[-{scale:e},{scale:e}],origin distance{r:e}");
        assert_eq!(r, 0.);
    }
}

mod predicates;
