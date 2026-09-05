// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn recipe_backed_dimension_projects_disjoint_mixed_repeated_distance() {
    let stream = "f3d:A";
    let placement = DesignSketchPlacement {
        frame: crate::records::DesignSketchFrame::new(0, crate::records::DesignSketchFrameForm::ScopeCompact).unwrap(),

        id: format!("{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: crate::records::DesignEntityId::try_from("0_100".to_owned()).expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 11,

        paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),

    };
    let parameter = DesignParameter {
        id: format!("{stream}:design-parameter#20"),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 20,
        source_ordinal: 4,
        source: crate::records::DesignParameterSource::new("Linear Dimension-4".into(), Some(21), Some(crate::records::Located { value: crate::records::DesignParameterDiscriminator::Code0, offset: 0 })).unwrap(),
        expression: "thickness".into(),
        expression_offset: 0,
        source_kind_offset: 0,

        unit: Some(crate::records::RecordedValue { value: "mm".into(), offset: Some(0) }),
        name: "d4".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let owner = DesignParameterOwner {
        id: format!("{stream}:design-parameter-owner#21"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "292".into(),
        record_index: 21,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
        parameter_record_index: 20,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    };
    let companion = DesignParameterCompanion {
        id: format!("{stream}:design-parameter-companion#22"),
        byte_offset: 0,
        class_tag: "408".into(),
        record_index: 22,
        owner_record_index: 21,
        timestamp_micros: std::num::NonZeroU64::new(1).unwrap(),
        timestamp_micros_offset: 0,
        payload_byte_offset: 58,
        payload_byte_length: 200,
        owned_recipe_ids: Vec::new(),
    };
    let recipe = |ordinal, record_index| DesignDimensionRecipeRecord {
        id: format!("{stream}:design-dimension-recipe-record#{record_index}"),
        companion_record_index: 22,
        recipe_ordinal: ordinal,
        recipe_id: format!("{stream}:construction-recipe#{record_index}"),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: "423".into(),
        record_index,
        frame_length: 10,
        prefix_offset: 0,
        prefix_bytes: Vec::new(),
        references: Vec::new(),
        program_offset: 0,
        program: vec![-1],
        matching_edge_operand_ids: Vec::new(),
    };
    let sketch = neutral_sketch_id(&placement);
    let line = |name: &str, start, end| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Line { start, end },
        )
    };
    let point = |name: &str, position| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Point { position },
        )
    };
    let entities = [
        line("first", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        line("second", Point2::new(0.0, 2.0), Point2::new(4.0, 2.0)),
        line("third", Point2::new(10.0, 0.0), Point2::new(10.0, 4.0)),
        line("fourth", Point2::new(12.0, 0.0), Point2::new(12.0, 4.0)),
    ];
    let mut recipe_entities = entities.to_vec();
    recipe_entities.extend([
        point("point-first", Point2::new(20.0, 0.0)),
        point("point-second", Point2::new(20.0, 2.0)),
    ]);
    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(1, 31), recipe(0, 30)],
            points: &[],
            curves: &[],
            entities: &recipe_entities,
        },
        &[],
    );
    let [constraint] = constraints.as_slice() else {
        panic!("expected one recipe-backed dimension")
    };
    let SketchConstraintDefinition::RepeatedDistance {
        measurements,
        parameter: projected_parameter,
        ..
    } = &constraint.definition
    else {
        panic!("expected repeated recipe-backed dimension")
    };
    let expected_parameter = neutral_parameter_id_parts(stream, parameter.record_index).0;
    assert_eq!(&projected_parameter.0, &expected_parameter);
    assert!(matches!(
        measurements.as_slice(),
        [
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Vertical { first, second },
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance { .. },
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance { .. },
        ] if first == &cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("point-first".into()))
            && second == &cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("point-second".into()))
    ));

    let mut radial_parameter = parameter.clone();
    radial_parameter.source = crate::records::DesignParameterSource::new("Radial Dimension-4".into(), radial_parameter.owner_record_index(), radial_parameter.family_discriminator()).unwrap();
    let circle = SketchEntity::new(
        SketchEntityId("radial-circle".into()),
        sketch.clone(),
        SketchGeometry::Circle {
            center: Point2::new(20.0, 20.0),
            radius: Length(2.0),
        },
    );
    let mut radial_entities = entities.to_vec();
    radial_entities.push(circle.clone());
    let radial_constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(0, 30)],
            points: &[],
            curves: &[],
            entities: &radial_entities,
        },
        &[],
    );
    assert!(matches!(
        radial_constraints.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Radius {
                entity,
                parameter: actual_parameter,
            },
            ..
        }] if entity == circle.id()
            && actual_parameter == &neutral_parameter_id_parts(stream, parameter.record_index)
    ));

    let annotation_point = SketchPoint {
        id: format!("{stream}:sketch-point#50"),
        record_index: 50,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        coordinate_offset: 0,
        entity_genesis: None,
        record_form: crate::records::SketchPointRecordForm::version11(
            50,
            crate::records::SketchPointClosure::Selector0State0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(4.5, 0.0),
        depth: 0.0,
        companion: None,
    };
    let annotation_curve = SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#51"),
        record_index: 51,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 51,
        secondary_id: 0,
        geometry: None,
    };
    let annotation_point_entity = SketchEntity::new(
        SketchEntityId("radial-extension-point".into()),
        sketch.clone(),
        SketchGeometry::Point {
            position: annotation_point.coordinates,
        },
    )
    .with_construction(true)
    .with_native_ref(Some(annotation_point.id.clone()));
    let mut annotation_line_entity = entities[0].clone();
    annotation_line_entity.native_ref = Some(annotation_curve.id.clone());
    let annotation_group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#60"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: "292".into(),
        record_index: 60,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                returned: crate::records::Located { value: 50, offset: 0 },
                geometry_record_index: 50,
                geometry_reference_offset: 0,
                role: 2,
                role_offset: 0,
            },
            DesignDimensionLocus {
                returned: crate::records::Located { value: 51, offset: 0 },
                geometry_record_index: 51,
                geometry_reference_offset: 0,
                role: 2,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 1,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        next_class_tag: "300".into(),
        next_record_index: 61,
        next_byte_offset: 100,
    };
    let extension_constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&annotation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: std::slice::from_ref(&annotation_point),
            curves: std::slice::from_ref(&annotation_curve),
            entities: &[
                circle.clone(),
                annotation_point_entity,
                annotation_line_entity,
            ],
        },
        &[],
    );
    assert!(matches!(
        extension_constraints.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Radius {
                entity,
                parameter: actual_parameter,
            },
            ..
        }] if entity == circle.id()
            && actual_parameter == &neutral_parameter_id_parts(stream, parameter.record_index)
    ));

    let curves = [40, 41].map(|record_index| SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#{record_index}"),
        record_index,
        owner_reference: Some(100),
        class_tag: "300".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: u64::from(record_index),
        secondary_id: 0,
        geometry: None,
    });
    let mut entities_with_refs = entities.clone();
    for (entity, curve) in entities_with_refs.iter_mut().zip(&curves) {
        entity.native_ref = Some(curve.id.clone());
    }
    let relation_group = DesignDimensionLocusGroup {
        id: format!("{stream}:design-dimension-locus-group#40"),
        companion_record_index: 22,
        byte_offset: 0,
        class_tag: "292".into(),
        record_index: 40,
        frame_length: 100,
        loci: vec![
            DesignDimensionLocus {
                returned: crate::records::Located { value: 40, offset: 0 },
                geometry_record_index: 40,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
            DesignDimensionLocus {
                returned: crate::records::Located { value: 41, offset: 0 },
                geometry_record_index: 41,
                geometry_reference_offset: 0,
                role: 0,
                role_offset: 0,
            },
        ],
        owner_reference: 100,
        owner_reference_offset: 0,
        owner_role: 0,
        owner_role_offset: 0,
        state: 0,
        state_offset: 0,
        next_class_tag: "300".into(),
        next_record_index: 42,
        next_byte_offset: 100,
    };
    let with_independent_relation = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&relation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(1, 31), recipe(0, 30)],
            points: &[],
            curves: &curves,
            entities: &entities_with_refs,
        },
        &[],
    );
    assert_eq!(with_independent_relation.len(), 2);
    assert!(with_independent_relation.iter().any(|constraint| matches!(
        constraint.definition,
        SketchConstraintDefinition::RepeatedDistance { .. }
    )));
    assert!(with_independent_relation.iter().any(|constraint| matches!(
        constraint.definition,
        SketchConstraintDefinition::Parallel { .. }
    )));
    let mut radial_parameter = parameter.clone();
    radial_parameter.source = crate::records::DesignParameterSource::new("Radial Dimension-2".into(), radial_parameter.owner_record_index(), radial_parameter.family_discriminator()).unwrap();
    let radial_with_independent_relation = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&relation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &curves,
            entities: &entities_with_refs,
        },
        &[],
    );
    assert_eq!(radial_with_independent_relation.len(), 2);
    assert!(radial_with_independent_relation
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Parallel { .. }
        )));
    assert!(radial_with_independent_relation
        .iter()
        .any(|constraint| matches!(
            constraint.definition,
            SketchConstraintDefinition::Native {
                parameter: Some(_),
                ..
            }
        )));
    let without_dimension_frame = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: std::slice::from_ref(&relation_group),
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &curves,
            entities: &entities_with_refs,
        },
        &[],
    );
    assert_eq!(without_dimension_frame.len(), 2);
    assert!(without_dimension_frame.iter().any(|constraint| matches!(
        constraint.definition,
        SketchConstraintDefinition::Native {
            parameter: Some(_),
            ..
        }
    )));

    let mut incompatible_unit = parameter.clone();
    incompatible_unit.unit.as_mut().expect("parameter unit").value = "deg".into();
    let constraints = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&incompatible_unit),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[recipe(1, 31), recipe(0, 30)],
            points: &[],
            curves: &[],
            entities: &entities,
        },
        &[],
    );
    assert!(matches!(
        constraints.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native { operands, .. },
            ..
        }] if operands.len() == 2
    ));

    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: &[],
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Native {
                native_kind,
                native_state: None,
                native_flags: None,
                entities,
                parameter: Some(actual_parameter),
                operands,
                ..
            },
            native_ref: Some(native_ref),
            ..
        }] if native_kind == "Linear Dimension-4"
            && entities.is_empty()
            && actual_parameter.0 == expected_parameter
            && native_ref == &companion.id
            && matches!(operands.as_slice(), [cadmpeg_ir::sketches::SketchNativeOperand {
                native_kind,
                native_field: Some(field),
                native_role: None,
                object_index: 22,
                native_ref: Some(operand_ref),
            }] if native_kind == "dimension_companion"
                && field == "companion_payload"
                && operand_ref == &companion.id)
    ));

    let mut radial_parameter = parameter.clone();
    radial_parameter.source = crate::records::DesignParameterSource::new("Radial Dimension-2".into(), radial_parameter.owner_record_index(), radial_parameter.family_discriminator()).unwrap();
    let radial_entity = SketchEntity::new(
        SketchEntityId("circle".into()),
        sketch.clone(),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length(2.0),
        },
    );
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&radial_parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: std::slice::from_ref(&radial_entity),
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::Radius {
                entity,
                parameter: actual_parameter,
            },
            native_ref: Some(native_ref),
            ..
        }] if entity == radial_entity.id()
            && actual_parameter.0 == expected_parameter
            && native_ref == &companion.id
    ));

    let mut empty_companion = companion.clone();
    empty_companion.payload_byte_length = 0;
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&empty_companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: &[],
        },
        &[],
    );
    assert!(retained.is_empty());

    let line = SketchEntity::new(
        SketchEntityId("measured-line".into()),
        sketch,
        SketchGeometry::Line {
            start: Point2::new(3.0, 4.0),
            end: Point2::new(3.0, 6.0),
        },
    );
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: std::slice::from_ref(&line),
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::DistanceLoci {
                first: cadmpeg_ir::sketches::SketchLocus::Start(first),
                second: cadmpeg_ir::sketches::SketchLocus::End(second),
                parameter: actual_parameter,
            },
            ..
        }] if first == line.id()
            && second == line.id()
            && actual_parameter == &neutral_parameter_id_parts(stream, parameter.record_index)
    ));

    let second_line = SketchEntity::new(
        SketchEntityId("second-measured-line".into()),
        line.sketch.clone(),
        line.geometry.clone(),
    )
    .with_construction(line.construction)
    .with_native_ref(line.native_ref.clone())
    .with_geometry_ref(line.geometry_ref.clone())
    .with_endpoint_refs(line.endpoint_refs.clone());
    let retained = project_dimension_constraints(
        &crate::design::dimensions::DimensionConstraintInputs {
            placements: std::slice::from_ref(&placement),
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            pairs: &[],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
            companions: std::slice::from_ref(&empty_companion),
            recipe_records: &[],
            points: &[],
            curves: &[],
            entities: &[line, second_line],
        },
        &[],
    );
    assert!(matches!(
        retained.as_slice(),
        [cadmpeg_ir::sketches::SketchConstraint {
            definition: SketchConstraintDefinition::RepeatedLength {
                entities,
                parameter,
            },
            ..
        }] if entities.len() == 2
            && parameter == &neutral_parameter_id_parts(stream, radial_parameter.record_index)
    ));
}

#[test]
fn recipe_dimension_requires_one_axis_aligned_point_pair() {
    let sketch = SketchId("sketch".into());
    let point = |name: &str, u, v| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let parameter = cadmpeg_ir::features::ParameterId("parameter".into());
    let mut entities = vec![
        point("first", -30.0, 2.0),
        point("second", -30.0, 0.0),
        point("unrelated", 10.0, 10.0),
    ];
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(
            &entities,
            &sketch,
            2.0,
            &parameter,
            0.0,
        ).as_slice(),
        [SketchConstraintDefinition::VerticalDistance { first, second, parameter: actual }]
            if *first == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("first".into()))
                && *second == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("second".into()))
                && *actual == parameter
    ));
    entities.push(point("ambiguous", 10.0, 8.0));
    let candidates = crate::design::dimensions::recipe_linear_dimension_candidates(
        &entities, &sketch, 2.0, &parameter, 0.0,
    );
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        crate::design::dimensions::recipe_dimension_candidate_entities(&candidates),
        [
            SketchEntityId("first".into()),
            SketchEntityId("second".into()),
            SketchEntityId("unrelated".into()),
            SketchEntityId("ambiguous".into()),
        ]
    );
}

#[test]
fn recipe_dimension_resolves_one_parallel_line_pair() {
    let sketch = SketchId("sketch".into());
    let line = |name: &str, start, end| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Line { start, end },
        )
    };
    let mut entities = vec![
        line("first", Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
        line("second", Point2::new(1.0, 2.0), Point2::new(5.0, 2.0)),
        line("unrelated", Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
    ];
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(
            &entities,
            &sketch,
            2.0,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            0.0,
        ).as_slice(),
        [SketchConstraintDefinition::Distance { entities, .. }]
            if entities.as_slice() == [SketchEntityId("first".into()), SketchEntityId("second".into())]
    ));
    let point = |name: &str, position| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Point { position },
        )
    };
    let mut entities_with_endpoints = entities.clone();
    entities_with_endpoints.extend([
        point("first-start", Point2::new(0.0, 0.0)),
        point("first-end", Point2::new(4.0, 0.0)),
        point("second-start", Point2::new(1.0, 2.0)),
        point("second-end", Point2::new(5.0, 2.0)),
    ]);
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(
            &entities_with_endpoints,
            &sketch,
            2.0,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            0.0,
        ).as_slice(),
        [SketchConstraintDefinition::Distance { entities, .. }]
            if entities.as_slice() == [SketchEntityId("first".into()), SketchEntityId("second".into())]
    ));

    let parameter = DesignParameter {
        id: "f3d:A:design-parameter#1".into(),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 1,
        source_ordinal: 1,
        source: crate::records::DesignParameterSource::new("Linear Dimension-2".into(), Some(2), Some(crate::records::Located { value: crate::records::DesignParameterDiscriminator::Code0, offset: 0 })).unwrap(),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind_offset: 0,

        unit: Some(crate::records::RecordedValue { value: "mm".into(), offset: Some(0) }),
        name: "d1".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    assert!(matches!(
        crate::design::dimensions::unique_parallel_line_dimension_definition(
            &entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            0.0,
        ),
        Some(SketchConstraintDefinition::Distance {
            entities,
            ..
        }) if entities.as_slice()
            == [SketchEntityId("first".into()), SketchEntityId("second".into())]
    ));

    let fragment = line(
        "second-fragment",
        Point2::new(7.0, 2.0),
        Point2::new(9.0, 2.0),
    );
    let mut fragmented_entities = entities.clone();
    fragmented_entities.push(fragment);
    fragmented_entities.push(line(
        "disjoint-first",
        Point2::new(20.0, 0.0),
        Point2::new(20.0, 1.0),
    ));
    fragmented_entities.push(line(
        "disjoint-second",
        Point2::new(22.0, 3.0),
        Point2::new(22.0, 4.0),
    ));
    assert!(matches!(
        crate::design::dimensions::owner_scoped_parallel_line_set_dimension_definition(
            &fragmented_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::ParallelLineSetDistance {
            first,
            second,
            ..
        }) if first == vec![SketchEntityId("first".into())]
            && second == vec![
                SketchEntityId("second".into()),
                SketchEntityId("second-fragment".into()),
            ]
    ));

    let point = SketchEntity::new(
        SketchEntityId("point".into()),
        sketch.clone(),
        SketchGeometry::Point {
            position: Point2::new(0.0, 2.0),
        },
    );
    let mut point_entities = entities.clone();
    point_entities.push(point);
    assert!(matches!(
        crate::design::dimensions::unique_point_line_dimension_definition(
            &point_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            1.0e-6,
        ),
        Some(SketchConstraintDefinition::Distance {
            entities,
            ..
        }) if entities.as_slice()
            == [SketchEntityId("point".into()), SketchEntityId("first".into())]
    ));

    entities.push(line("third", Point2::new(-1.0, 4.0), Point2::new(3.0, 4.0)));
    assert!(
        crate::design::dimensions::unique_parallel_line_dimension_definition(
            &entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            0.0,
        )
        .is_none()
    );
    point_entities.push(entities.last().expect("third line").clone());
    assert!(
        crate::design::dimensions::unique_point_line_dimension_definition(
            &point_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            1.0e-6,
        )
        .is_none()
    );
}

#[test]
fn recipe_dimension_resolves_unique_axis_aligned_extension_point() {
    let sketch = SketchId("sketch".into());
    let parameter = cadmpeg_ir::features::ParameterId("parameter".into());
    let point = |name: &str, u, v| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let entities = vec![
        SketchEntity::new(
            SketchEntityId("carrier".into()),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(2.0, 0.0),
                end: Point2::new(0.0, 0.0),
            },
        ),
        point("carrier-start", 2.0, 0.0),
        point("carrier-end", 0.0, 0.0),
        point("extension", 4.0, 0.0),
        point("off-carrier-horizontal", 2.0, 3.0),
        point("off-carrier-vertical", 4.0, 2.0),
    ];
    let candidates = crate::design::dimensions::recipe_linear_dimension_candidates(
        &entities, &sketch, 2.0, &parameter, 0.0,
    );
    assert!(candidates.len() > 2);
    assert!(matches!(
        crate::design::dimensions::recipe_extension_point_dimension(
            &candidates,
            &entities,
            &sketch,
        ),
        Some(SketchConstraintDefinition::HorizontalDistance { first, second, parameter: actual })
            if first == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("carrier-start".into()))
                && second == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId("extension".into()))
                && actual == parameter
    ));

    let mut ambiguous = entities;
    ambiguous.extend([
        point("second-carrier-start", 12.0, 5.0),
        point("second-extension", 14.0, 5.0),
        SketchEntity::new(
            SketchEntityId("second-carrier".into()),
            sketch.clone(),
            SketchGeometry::Line {
                start: Point2::new(12.0, 5.0),
                end: Point2::new(10.0, 5.0),
            },
        ),
    ]);
    let candidates = crate::design::dimensions::recipe_linear_dimension_candidates(
        &ambiguous, &sketch, 2.0, &parameter, 0.0,
    );
    assert!(crate::design::dimensions::recipe_extension_point_dimension(
        &candidates,
        &ambiguous,
        &sketch,
    )
    .is_none());
}

#[test]
fn concentric_circle_dimensions_require_disjoint_matching_pairs() {
    let sketch = SketchId("sketch".into());
    let parameter = DesignParameter {
        id: "f3d:A:design-parameter#1".into(),
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 1,
        source_ordinal: 1,
        source: crate::records::DesignParameterSource::new("Linear Dimension-2".into(), Some(2), Some(crate::records::Located { value: crate::records::DesignParameterDiscriminator::Code0, offset: 0 })).unwrap(),
        expression: "2 mm".into(),
        expression_offset: 0,
        source_kind_offset: 0,

        unit: Some(crate::records::RecordedValue { value: "mm".into(), offset: Some(0) }),
        name: "d1".into(),
        name_offset: 0,
        evaluated_value: 0.2,
        evaluated_value_offset: 0,
    };
    let circle = |name: &str, center, radius| {
        SketchEntity::new(
            SketchEntityId(name.into()),
            sketch.clone(),
            SketchGeometry::Circle {
                center,
                radius: Length(radius),
            },
        )
    };
    let mut circles = vec![
        circle("outer-a", Point2::new(0.0, 0.0), 5.0),
        circle("inner-a", Point2::new(0.0, 0.0), 3.0),
        circle("outer-b", Point2::new(20.0, 0.0), 8.0),
        circle("inner-b", Point2::new(20.0, 0.0), 6.0),
    ];
    let definition = crate::design::dimensions::concentric_circle_dimension_definition(
        &circles,
        &sketch,
        &parameter,
        &cadmpeg_ir::features::ParameterId("parameter".into()),
        0.0,
    )
    .expect("two disjoint concentric pairs");
    assert!(matches!(
        definition,
        SketchConstraintDefinition::RepeatedDistance {
            measurements,
            ..
        } if measurements == vec![
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("outer-a".into())
                ),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("inner-a".into())
                ),
            },
            cadmpeg_ir::sketches::SketchDistanceMeasurement::Distance {
                first: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("outer-b".into())
                ),
                second: cadmpeg_ir::sketches::SketchLocus::Entity(
                    SketchEntityId("inner-b".into())
                ),
            },
        ]
    ));

    circles.push(circle("overlap", Point2::new(0.0, 0.0), 1.0));
    assert!(
        crate::design::dimensions::concentric_circle_dimension_definition(
            &circles,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId("parameter".into()),
            0.0,
        )
        .is_none()
    );
}

#[test]
fn expression_dependency_audit_counts_only_unprojected_same_stream_names() {
    let parameter = |stream: &str, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated parameter");
        parameter.id = format!("f3d:{stream}:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let native = vec![
        parameter("A", 1, "1 mm", "Width"),
        parameter("A", 2, "Width + External", "Half"),
        parameter("B", 1, "1 mm", "External"),
    ];
    let (_, mut projected) = project_parameter_design(&native, &[], &[], &[], &[], &[], &[], &[]);
    assert_eq!(
        unresolved_parameter_expression_dependency_count(&native, &projected),
        0
    );

    projected
        .iter_mut()
        .find(|parameter| parameter.name == "Half")
        .expect("Half parameter")
        .dependencies
        .clear();
    assert_eq!(
        unresolved_parameter_expression_dependency_count(&native, &projected),
        1
    );
}
