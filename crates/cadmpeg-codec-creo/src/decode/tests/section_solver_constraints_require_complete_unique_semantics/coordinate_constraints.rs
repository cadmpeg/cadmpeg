// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchCoordinateAxis, SketchEntityId, SketchId, SketchLocus};
use super::super::{section_skamp_constraints};
use crate::decode::sketch::coordinates::{resolved_section_points};
use super::fixtures::{base_definition};
use crate::feature::definitions::test_support::{replace_points};
use crate::feature::definitions::{VariableType, ScalarLane};

#[test]
fn section_solver_same_coordinates_and_point_coincidence_preserve_semantics() {
    let definition = base_definition();
    let constraints =
        section_skamp_constraints(&definition, &SketchId::mint("creo:model:sketch#917").expect("valid test fixture"));
    let mut fixed_y_coordinate = definition.clone();
    fixed_y_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .kind = 30;
    fixed_y_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .flags = 99;
    assert_eq!(
        section_skamp_constraints(
            &fixed_y_coordinate,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[14]
            .0
            .definition,
        constraints[14].0.definition
    );
    let mut propagated_same_coordinate = definition.clone();
    propagated_same_coordinate
        .variables
        .as_mut()
        .expect("variables")
        .rows
        .iter_mut()
        .find(|row| row.key == 5 && row.variable_type == VariableType::V)
        .expect("second same-coordinate point")
        .value = ScalarLane::Undefined;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &propagated_same_coordinate)).expect("test section solve").get(&5),
        Some(&[3.0, 2.0])
    );
    let mut unsolved_same_coordinate = definition.clone();
    replace_points(unsolved_same_coordinate.variables.as_mut().expect("variables"), Vec::new());
    assert_eq!(
        section_skamp_constraints(
            &unsolved_same_coordinate,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[14]
            .0
            .definition,
        constraints[14].0.definition
    );
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .kind = 31;
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .flags = 0;
    assert_eq!(*(section_skamp_constraints(
            &unsolved_same_coordinate,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[14]
            .0
            .definition).kind(),
        SketchConstraintDefinitionInput::SameCoordinate { relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:12".to_string()
            ).expect("valid test fixture")),SketchLocus::Start(SketchEntityId::mint(
                "creo:featdefs:sketch_entity#917:15".to_string()
            ).expect("valid test fixture")),SketchCoordinateAxis::U).expect("valid test fixture") }
    );
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .kind = 17;
    unsolved_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .flags = 0;
    assert!(matches!(
        section_skamp_constraints(
            &unsolved_same_coordinate,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[14]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut conflicting_same_coordinate = definition.clone();
    conflicting_same_coordinate
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[14]
        .flags = 1;
    assert!(matches!(
        section_skamp_constraints(
            &conflicting_same_coordinate,
            &SketchId::mint("creo:model:sketch#917").expect("valid test fixture")
        )[14]
            .0
            .definition.kind(),
        SketchConstraintDefinitionInput::Native { .. }
    ));
    let mut point_coincidence_definition = definition.clone();
    replace_points(point_coincidence_definition
        .variables
        .as_mut()
        .expect("variables"), vec![
        crate::feature::definitions::FeatureSectionPoint {
            point_id: 4,
            u: Some(1.0),
            v: Some(2.0),
        },
        crate::feature::definitions::FeatureSectionPoint {
            point_id: 6,
            u: None,
            v: Some(2.0),
        },
    ]);
    let point_segment = point_coincidence_definition
        .segments
        .as_mut()
        .expect("segments");
    point_segment.declared_count = 6;
    point_segment.rows.insert(crate::feature::segment_rows::SegmentRow::Ordinary(crate::feature::definitions::FeatureSegment {
        kind: crate::feature::definitions::FeatureSegmentKind::Point(6),
        directions: [None; 3],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 17,
        body: Vec::new(),
        offset: 45,
    }));
    point_coincidence_definition
        .relations
        .as_mut()
        .expect("relations")
        .skamps.as_mut().expect("skamp table").rows_mut()[8]
        .items = vec![
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 14,
            sense: 0,
        },
        crate::feature::definitions::FeatureSkampItem {
            entity_id: 17,
            sense: 0,
        },
    ];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| resolved_section_points(ctx, &point_coincidence_definition)).expect("test section solve").get(&6),
        Some(&[1.0, 2.0])
    );
}

