// SPDX-License-Identifier: Apache-2.0

use super::super::{declared_solver_rows, section_skamp_constraints, synchronize_skamp_count};
use super::fixtures::base_definition;
use crate::decode::sketch::coordinates::resolved_section_points;
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;
use crate::feature::definitions::test_support::{append_points, with_points};
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus,
};
use std::collections::BTreeMap;

#[test]
fn section_solver_projected_arc_and_mixed_constraints() {
    let definition = base_definition();
    let mut point_row_symmetry = definition.clone();
    point_row_symmetry.variables = Some(with_points(
        crate::feature::definitions::FeatureVariableTable {
            declared_count: 0,
            entity_ref: None,
            rows: Vec::new(),
            offset: 90,
        },
        vec![
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 4,
                u: Some(1.0),
                v: Some(1.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 6,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 7,
                u: None,
                v: None,
            },
        ],
    ));
    let segments = point_row_symmetry.segments.as_mut().expect("segments");
    segments.rows.edit_points(|rows| {
        *rows = vec![
            crate::feature::definitions::FeaturePointSegment {
                point_id: 4,
                external_id: 18,
                offset: 91,
            },
            crate::feature::definitions::FeaturePointSegment {
                point_id: 6,
                external_id: 19,
                offset: 92,
            },
            crate::feature::definitions::FeaturePointSegment {
                point_id: 7,
                external_id: 20,
                offset: 93,
            },
        ];
    });
    segments.declared_count = 8;
    let relations = point_row_symmetry.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut relations.skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 94,
            kind: 14,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 18,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 19,
                    sense: 4,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 20,
                    sense: 4,
                },
            ],
            offset: 94,
        }];
    relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &point_row_symmetry
        ))
        .expect("test section solve")
        .get(&7),
        Some(&[2.0, 2.0])
    );
    let mut circle_center_symmetry = point_row_symmetry.clone();
    append_points(
        circle_center_symmetry
            .variables
            .as_mut()
            .expect("variables"),
        vec![crate::feature::definitions::FeatureSectionPoint {
            point_id: 8,
            u: Some(0.0),
            v: Some(0.0),
        }],
    );
    let segments = circle_center_symmetry.segments.as_mut().expect("segments");
    segments.rows.edit_circles(|rows| {
        *rows = vec![crate::feature::definitions::FeatureCircleSegment {
            center_id: 8,
            radius_ref: 101,
            external_id: 21,
            offset: 95,
        }];
    });
    segments.declared_count = 9;
    circle_center_symmetry
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[1]
        .entity_id = 21;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &circle_center_symmetry
        ))
        .expect("test section solve")
        .get(&7),
        Some(&[2.0, 2.0])
    );
    let mut projected_copy = definition.clone();
    projected_copy.trim_entities = Some(crate::feature::definitions::FeatureTrimEntityTable {
        declared_count: None,
        entity_ref: None,
        entry_ref: None,
        buckets: Vec::new(),
        rows: vec![crate::feature::definitions::FeatureTrimEntity {
            external_id: 13,
            mode: Some(0),
            vertices: [2, 3],
            kind: crate::feature::definitions::TrimEntityKind::Line,
            offset: 84,
        }],
        solved_external_ids: vec![13],
        offset: 83,
    });
    *declared_solver_rows(&mut projected_copy.relations.as_mut().expect("relations").skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 19,
            kind: 37,
            flags: 98_304,
            status: 34,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
            ],
            offset: 85,
        }];
    synchronize_skamp_count(&mut projected_copy);
    assert_eq!(
        *(section_skamp_constraints(
            &projected_copy,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::ProjectedCopy {
            source: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
            result: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
        }
    );
    let mismatched_projected_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("line")
                    .expect("nonempty source identity"),
            ),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            SketchGeometry::native(
                cadmpeg_core::text::NonBlankString::try_from("arc")
                    .expect("nonempty source identity"),
            ),
        ),
    ]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(
            ctx,
            &projected_copy,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&mismatched_projected_geometry),
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    projected_copy
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items
        .swap(0, 1);
    assert!(matches!(
        section_skamp_constraints(
            &projected_copy,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    for (kind, angle) in [
        (10, std::f64::consts::FRAC_PI_2),
        (11, std::f64::consts::PI),
    ] {
        let mut fixed_arc = definition.clone();
        let relations = fixed_arc.relations.as_mut().expect("relations");
        *declared_solver_rows(&mut relations.skamps) =
            vec![crate::feature::definitions::FeatureSkamp {
                id: 18,
                kind,
                flags: 0,
                status: 35,
                items: vec![crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                }],
                offset: 83,
            }];
        synchronize_skamp_count(&mut fixed_arc);
        assert_eq!(
            *(section_skamp_constraints(
                &fixed_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition)
                .kind(),
            SketchConstraintDefinitionInput::ArcAngle {
                entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture"),
                angle: cadmpeg_ir::scalar::PositiveAngle::new(angle)
                    .expect("positive angle fixture"),
            }
        );
        fixed_arc
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .status = 34;
        assert!(matches!(
            section_skamp_constraints(
                &fixed_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition
            .kind(),
            SketchConstraintDefinitionInput::ArcAngle { .. }
        ));
        fixed_arc
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .items[0] = crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 0,
        };
        assert!(matches!(
            section_skamp_constraints(
                &fixed_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition
            .kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ));
        fixed_arc
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .items[0] = crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        };
        assert!(matches!(
            section_skamp_constraints(
                &fixed_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition
            .kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ));
    }
    for kind in [12, 13] {
        let mut oriented_arc = definition.clone();
        let relations = oriented_arc.relations.as_mut().expect("relations");
        *declared_solver_rows(&mut relations.skamps) =
            vec![crate::feature::definitions::FeatureSkamp {
                id: 18,
                kind,
                flags: 0,
                status: 35,
                items: vec![crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                }],
                offset: 83,
            }];
        synchronize_skamp_count(&mut oriented_arc);
        let expected_entity =
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture");
        let axis = if kind == 12 {
            SketchCoordinateAxis::V
        } else {
            SketchCoordinateAxis::U
        };
        let expected = SketchConstraintDefinitionInput::SameCoordinate {
            relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                SketchLocus::Start(expected_entity.clone()),
                SketchLocus::End(expected_entity),
                axis,
            )
            .expect("valid test fixture"),
        };
        assert_eq!(
            *(section_skamp_constraints(
                &oriented_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition)
                .kind(),
            expected
        );
        oriented_arc
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .status = 34;
        assert!(matches!(
            section_skamp_constraints(
                &oriented_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition
            .kind(),
            SketchConstraintDefinitionInput::SameCoordinate { .. }
        ));
        oriented_arc
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .items[0] = crate::feature::definitions::FeatureSkampItem {
            entity_id: 12,
            sense: 0,
        };
        assert!(matches!(
            section_skamp_constraints(
                &oriented_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition
            .kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ));
        oriented_arc
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .items[0] = crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 2,
        };
        assert!(matches!(
            section_skamp_constraints(
                &oriented_arc,
                &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
            )[0]
            .0
            .definition
            .kind(),
            SketchConstraintDefinitionInput::Native { .. }
        ));
    }
    let mut type_33_bounded_curve = definition.clone();
    type_33_bounded_curve
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_bounded_curves(|rows| {
            *rows = vec![crate::feature::definitions::FeatureBoundedCurveSegment {
                directions: [None; 3],
                point_ids: [1, 2],
                center_id: None,
                arc_orientation: Some(1),
                vertical_horizontal: Some(0),
                radius_ref: None,
                radius2_ref: None,
                external_id: 18,
                offset: 86,
            }];
        });
    type_33_bounded_curve
        .segments
        .as_mut()
        .expect("segments")
        .declared_count = 6;
    *declared_solver_rows(
        &mut type_33_bounded_curve
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![crate::feature::definitions::FeatureSkamp {
        id: 20,
        kind: 33,
        flags: 34,
        status: 35,
        items: vec![crate::feature::definitions::FeatureSkampItem {
            entity_id: 18,
            sense: 10,
        }],
        offset: 87,
    }];
    synchronize_skamp_count(&mut type_33_bounded_curve);
    let type_33_constraints = section_skamp_constraints(
        &type_33_bounded_curve,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    assert!(matches!(
        type_33_constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Fixed { entity }
            if entity == &SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:18".to_string()
            ).expect("valid test fixture")
    ));
    assert_eq!(type_33_constraints[0].0.active, Some(true));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .status = 34;
    let inactive_type_33 = section_skamp_constraints(
        &type_33_bounded_curve,
        &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
    );
    assert!(matches!(
        inactive_type_33[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Fixed { .. }
    ));
    assert_eq!(inactive_type_33[0].0.active, Some(false));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .flags = 0;
    assert!(matches!(
        section_skamp_constraints(
            &type_33_bounded_curve,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native {
            native_flags: Some(0),
            ..
        }
    ));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .flags = 34;
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[0]
        .sense = 0;
    assert!(matches!(
        section_skamp_constraints(
            &type_33_bounded_curve,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    type_33_bounded_curve
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[0]
        .sense = 10;
    let mut missing_bounded_curve = type_33_bounded_curve.clone();
    missing_bounded_curve
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_bounded_curves(Vec::clear);
    assert!(matches!(
        section_skamp_constraints(
            &missing_bounded_curve,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let emitted_type_33_geometry = BTreeMap::from([(
        SketchEntityId::mint("creo:featdefs:sketch_entity#917:18".to_string())
            .expect("valid test fixture"),
        SketchGeometry::native(
            cadmpeg_core::text::NonBlankString::try_from("bounded_curve")
                .expect("nonempty source identity"),
        ),
    )]);
    let emitted_type_33_constraints = crate::decode::with_test_decode_ctx(|ctx| {
        section_skamp_constraints_for_geometry(
            ctx,
            &type_33_bounded_curve,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&emitted_type_33_geometry),
        )
    })
    .expect("test section solve");
    assert!(matches!(
        emitted_type_33_constraints[0].0.definition.kind(),
        SketchConstraintDefinitionInput::Fixed { entity }
            if entity == &SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:18".to_string()
            ).expect("valid test fixture")
    ));

    let mut type_33_line = definition.clone();
    *declared_solver_rows(&mut type_33_line.relations.as_mut().expect("relations").skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 20,
            kind: 33,
            flags: 393_216,
            status: 35,
            items: vec![crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 10,
            }],
            offset: 88,
        }];
    synchronize_skamp_count(&mut type_33_line);
    assert!(matches!(
        section_skamp_constraints(
            &type_33_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native {
            native_flags: Some(393_216),
            ..
        }
    ));

    type_33_line
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .flags = 0;
    assert!(matches!(
        section_skamp_constraints(
            &type_33_line,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native {
            native_flags: Some(0),
            ..
        }
    ));
    let mut mixed_tangent = definition.clone();
    let mixed_tangent_relations = mixed_tangent.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut mixed_tangent_relations.skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 18,
            kind: 4,
            flags: 0,
            status: 34,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 3,
                },
            ],
            offset: 83,
        }];
    mixed_tangent_relations
        .skamps
        .as_mut()
        .expect("skamp table")
        .header_mut()
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        *(section_skamp_constraints(
            &mixed_tangent,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::TangentLoci {
            first: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
        }
    );
    mixed_tangent
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items
        .reverse();
    assert_eq!(
        *(section_skamp_constraints(
            &mixed_tangent,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::TangentLoci {
            first: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                    .expect("valid test fixture")
            ),
            second: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
        }
    );
    mixed_tangent
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[0] = crate::feature::definitions::FeatureSkampItem {
        entity_id: 15,
        sense: 2,
    };
    assert!(matches!(
        section_skamp_constraints(
            &mixed_tangent,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut mixed_perpendicular = definition.clone();
    *declared_solver_rows(
        &mut mixed_perpendicular
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![crate::feature::definitions::FeatureSkamp {
        id: 19,
        kind: 5,
        flags: 0,
        status: 34,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 13,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
        ],
        offset: 84,
    }];
    synchronize_skamp_count(&mut mixed_perpendicular);
    assert_eq!(
        *(section_skamp_constraints(
            &mixed_perpendicular,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Perpendicular {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    mixed_perpendicular
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[0]
        .sense = 2;
    assert!(matches!(
        section_skamp_constraints(
            &mixed_perpendicular,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition
        .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    *declared_solver_rows(
        &mut mixed_perpendicular
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    ) = vec![
        crate::feature::definitions::FeatureSkamp {
            id: 18,
            kind: 0,
            flags: 0,
            status: 35,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 99,
                    sense: 2,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 14,
                    sense: 0,
                },
            ],
            offset: 83,
        },
        crate::feature::definitions::FeatureSkamp {
            id: 19,
            kind: 5,
            flags: 0,
            status: 34,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 99,
                    sense: 0,
                },
            ],
            offset: 84,
        },
    ];
    synchronize_skamp_count(&mut mixed_perpendicular);
    assert_eq!(
        *(section_skamp_constraints(
            &mixed_perpendicular,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[1]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Perpendicular {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                .expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:99".to_string())
                .expect("valid test fixture"),
        }
    );
}
