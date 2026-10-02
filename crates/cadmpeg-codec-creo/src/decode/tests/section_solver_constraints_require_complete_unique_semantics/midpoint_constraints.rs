// SPDX-License-Identifier: Apache-2.0

use super::super::{declared_solver_rows, section_skamp_constraints, synchronize_skamp_count};
use super::fixtures::base_definition;
use crate::decode::sketch::coordinates::{resolved_section_coordinates, resolved_section_points};
use crate::decode::sketch_transfer::loci::section_skamp_is_circular;
use crate::decode::sketch_transfer::skamp_constraints::section_skamp_constraints_for_geometry;
use crate::feature::definitions::test_support::replace_points;
use crate::feature::definitions::{ScalarLane, VariableType};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId, SketchLocus,
};
use std::collections::BTreeMap;

const EPS_ARC_MIDPOINT: f64 = 1e-12;

#[test]
fn section_solver_midpoints_preserve_saved_geometry_and_coordinate_constraints() {
    let definition = base_definition();
    let mut collinear_definition = definition.clone();
    let collinear_relations = collinear_definition.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut collinear_relations.skamps) =
        vec![crate::feature::definitions::FeatureSkamp {
            id: 9,
            kind: 9,
            flags: 0,
            status: 1,
            items: vec![
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 12,
                    sense: 0,
                },
                crate::feature::definitions::FeatureSkampItem {
                    entity_id: 15,
                    sense: 0,
                },
            ],
            offset: 83,
        }];
    collinear_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        *(section_skamp_constraints(
            &collinear_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Collinear {
            first: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
            second: SketchEntityId::mint("creo:featdefs:sketch_entity#917:15".to_string())
                .expect("valid test fixture"),
        }
    );
    let mut midpoint_definition = definition.clone();
    let midpoint = crate::feature::definitions::FeatureSkamp {
        id: 35,
        kind: 35,
        flags: 0,
        status: 1,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 14,
                sense: 0,
            },
        ],
        offset: 83,
    };
    let midpoint_relations = midpoint_definition.relations.as_mut().expect("relations");
    *declared_solver_rows(&mut midpoint_relations.skamps) = vec![midpoint];
    midpoint_relations
        .skamps
        .as_mut()
        .and_then(|table| table.header_mut())
        .expect("skamp header")
        .declared_count = 1;
    assert_eq!(
        *(section_skamp_constraints(
            &midpoint_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Entity(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:14".to_string())
                    .expect("valid test fixture")
            ),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    replace_points(
        midpoint_definition.variables.as_mut().expect("variables"),
        vec![
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 1,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 2,
                u: Some(4.0),
                v: Some(2.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 4,
                u: None,
                v: None,
            },
        ],
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &midpoint_definition
        ))
        .expect("test section solve"),
        BTreeMap::from([(1, [0.0, 0.0]), (2, [4.0, 2.0]), (4, [2.0, 1.0])])
    );
    let mut saved_line_midpoint = midpoint_definition.clone();
    saved_line_midpoint.order_table = Some(crate::feature::definitions::FeatureOrderTable {
        declared_count: 1,
        has_prototype: false,
        entity_ref: None,
        rows: vec![crate::feature::definitions::FeatureOrderRow {
            external_id: 99,
            internal_id: 20,
            bitmask: 1,
            offset: 95,
        }],
        offset: 94,
    });
    saved_line_midpoint.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
        entities: vec![crate::feature::definitions::FeatureSavedEntity::Line(
            crate::feature::definitions::FeatureSavedLine {
                entity_id: 20,
                references: Vec::new(),
                attributes: Vec::new(),
                endpoints: [
                    [Some(0.0), Some(0.0), Some(0.0)],
                    [Some(4.0), Some(2.0), Some(0.0)],
                ],
                body: Vec::new(),
                offset: 96,
            },
        )],
        offset: 96,
    });
    saved_line_midpoint
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[0]
        .entity_id = 99;
    synchronize_skamp_count(&mut saved_line_midpoint);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &saved_line_midpoint
        ))
        .expect("test section solve")
        .get(&4),
        Some(&[2.0, 1.0])
    );
    let mut arc_midpoint_definition = midpoint_definition.clone();
    replace_points(
        arc_midpoint_definition
            .variables
            .as_mut()
            .expect("variables"),
        vec![
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 2,
                u: Some(1.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 3,
                u: Some(0.0),
                v: Some(1.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 4,
                u: Some(0.0),
                v: Some(0.0),
            },
            crate::feature::definitions::FeatureSectionPoint {
                point_id: 8,
                u: None,
                v: None,
            },
        ],
    );
    arc_midpoint_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            rows.iter_mut()
                .find(|segment| segment.external_id == 13)
                .expect("arc")
                .arc_orientation = Some(0);
        });
    arc_midpoint_definition
        .segments
        .as_mut()
        .expect("segments")
        .rows
        .edit_ordinary(|rows| {
            rows.iter_mut()
                .find(|segment| segment.external_id == 14)
                .expect("point")
                .kind = crate::feature::definitions::FeatureSegmentKind::Point(8);
        });
    let midpoint_skamps = &mut arc_midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut();
    midpoint_skamps[0].items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 13,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 14,
            sense: 0,
        },
    ];
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
            ctx,
            &arc_midpoint_definition
        ))
        .expect("test section solve")
        .get(&8)
        .is_some_and(|[u, v]| {
            let expected = -std::f64::consts::FRAC_1_SQRT_2;
            (u - expected).abs() <= EPS_ARC_MIDPOINT && (v - expected).abs() <= EPS_ARC_MIDPOINT
        })
    );
    for (kind, expected_coordinate) in [(12, 1), (13, 0)] {
        let mut arc_alignment_definition = midpoint_definition.clone();
        replace_points(
            arc_alignment_definition
                .variables
                .as_mut()
                .expect("variables"),
            vec![
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 2,
                    u: Some(1.0),
                    v: Some(2.0),
                },
                crate::feature::definitions::FeatureSectionPoint {
                    point_id: 3,
                    u: if expected_coordinate == 0 {
                        None
                    } else {
                        Some(3.0)
                    },
                    v: if expected_coordinate == 1 {
                        None
                    } else {
                        Some(3.0)
                    },
                },
            ],
        );
        {
            let alignment_skamps = &mut arc_alignment_definition
                .relations
                .as_mut()
                .expect("relations")
                .skamps
                .as_mut()
                .expect("skamp table")
                .rows_mut();
            alignment_skamps[0] = crate::feature::definitions::FeatureSkamp {
                id: 12,
                kind,
                flags: 0,
                status: 1,
                items: vec![crate::feature::definitions::FeatureSkampItem {
                    entity_id: 13,
                    sense: 0,
                }],
                offset: 84,
            };
        }
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(
                ctx,
                &arc_alignment_definition
            ))
            .expect("test section solve")
            .get(&3),
            Some(&[
                if expected_coordinate == 0 { 1.0 } else { 3.0 },
                if expected_coordinate == 1 { 2.0 } else { 3.0 },
            ])
        );
        arc_alignment_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps
            .as_mut()
            .expect("skamp table")
            .rows_mut()[0]
            .status = 0;
        let unresolved = crate::decode::with_test_decode_ctx(|ctx| {
            resolved_section_points(ctx, &arc_alignment_definition)
        })
        .expect("test section solve");
        assert!(!unresolved.contains_key(&3));
    }
    midpoint_definition
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .iter_mut()
        .find(|row| row.key == 4 && row.variable_type == VariableType::U)
        .expect("midpoint point")
        .value = ScalarLane::Value(3.0);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_coordinates(
            ctx,
            &midpoint_definition
        ))
        .expect("test section solve")
        .get(&4),
        Some(&[Some(3.0), Some(1.0)])
    );
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[1] = crate::feature::definitions::FeatureSkampItem {
        entity_id: 13,
        sense: 0,
    };
    declared_solver_rows(
        &mut midpoint_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .push(crate::feature::definitions::FeatureSkamp {
        id: 36,
        kind: 3,
        flags: 0,
        status: 0,
        items: vec![
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 12,
                sense: 0,
            },
            crate::feature::definitions::FeatureSkampItem {
                entity_id: 13,
                sense: 4,
            },
        ],
        offset: 84,
    });
    synchronize_skamp_count(&mut midpoint_definition);
    assert!(section_skamp_is_circular(
        &midpoint_definition,
        &midpoint_definition
            .relations
            .as_ref()
            .expect("relations")
            .skamps()[0]
            .items[1],
    ));
    assert_eq!(
        *(section_skamp_constraints(
            &midpoint_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::Center(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
    declared_solver_rows(
        &mut midpoint_definition
            .relations
            .as_mut()
            .expect("relations")
            .skamps,
    )
    .truncate(1);
    synchronize_skamp_count(&mut midpoint_definition);
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[1] = crate::feature::definitions::FeatureSkampItem {
        entity_id: 14,
        sense: 0,
    };
    let midpoint_geometry = BTreeMap::from([
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(2.0, 0.0),
            })
            .expect("valid test fixture"),
        ),
        (
            SketchEntityId::mint("creo:featdefs:sketch_entity#917:14".to_string())
                .expect("valid test fixture"),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(1.0, 0.0),
            })
            .expect("valid test fixture"),
        ),
    ]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(
            ctx,
            &midpoint_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&midpoint_geometry),
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Midpoint { .. }
    ));
    let unresolved_midpoint_geometry = BTreeMap::from([(
        SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
            .expect("valid test fixture"),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .expect("valid test fixture"),
    )]);
    assert!(matches!(
        crate::decode::with_test_decode_ctx(|ctx| section_skamp_constraints_for_geometry(
            ctx,
            &midpoint_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"),
            Some(&unresolved_midpoint_geometry),
        ))
        .expect("test section solve")[0]
            .0
            .definition
            .kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    midpoint_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps
        .as_mut()
        .expect("skamp table")
        .rows_mut()[0]
        .items[1] = crate::feature::definitions::FeatureSkampItem {
        entity_id: 13,
        sense: 2,
    };
    assert_eq!(
        *(section_skamp_constraints(
            &midpoint_definition,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[0]
        .0
        .definition)
            .kind(),
        SketchConstraintDefinitionInput::Midpoint {
            point: SketchLocus::End(
                SketchEntityId::mint("creo:featdefs:sketch_entity#917:13".to_string())
                    .expect("valid test fixture")
            ),
            entity: SketchEntityId::mint("creo:featdefs:sketch_entity#917:12".to_string())
                .expect("valid test fixture"),
        }
    );
}
