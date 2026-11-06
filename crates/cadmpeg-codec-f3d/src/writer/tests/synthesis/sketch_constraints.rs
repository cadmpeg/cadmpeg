// SPDX-License-Identifier: Apache-2.0
//! Source-less sketch constraint synthesis.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::F3dCodec;

#[test]
fn source_less_native_point_relation_preserves_reference_runs() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        NativeOperandField, Sketch, SketchConstraint, SketchConstraintDefinitionInput,
        SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchNativeOperand,
        SketchPlacement, SketchProfiles,
    };

    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch = "authored:model:sketch#1".try_into().unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch,
        name: Some("Sketch_1".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
    for (id, x) in [
        ("authored:model:sketch-entity#1", 0.0),
        ("authored:model:sketch-entity#2", 10.0),
    ] {
        ir.model.sketch_entities.push(SketchEntity::new(
            id.try_into().unwrap(),
            ir.model.sketches[0].id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(x, 0.0),
            })
            .unwrap(),
        ));
    }
    let first = ir.model.sketch_entities[0].id().clone();
    let second = ir.model.sketch_entities[1].id().clone();
    let operands = [
        ("member", 100),
        ("member", 200),
        ("return", 200),
        ("return", 100),
    ]
    .into_iter()
    .map(|(field, index)| SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new("point").unwrap(),
        field: Some(NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::new(field).unwrap(),
            role: None,
        }),
        object_index: Some(index),
        native_ref: None,
    })
    .collect::<Vec<_>>();
    ir.model.sketch_constraints.push(SketchConstraint {
        id: "authored:model:sketch-constraint#1".try_into().unwrap(),
        sketch: ir.model.sketches[0].id.clone(),
        definition: SketchConstraintDefinitionInput::Native {
            native_kind: cadmpeg_core::text::NonBlankString::new("parallel").unwrap(),
            native_state: Some(0x10),
            native_flags: None,
            native_properties: Default::default(),
            entities: vec![first.clone(), second.clone(), second, first],
            parameter: None,
            operands: operands.clone(),
        }
        .try_into()
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let [constraint] = decoded.ir().model.sketch_constraints.as_slice() else {
        panic!("expected one native relation")
    };
    let SketchConstraintDefinitionInput::Native {
        native_kind,
        native_state,
        entities,
        operands: actual,
        ..
    } = constraint.definition.kind()
    else {
        panic!("expected a native relation")
    };
    assert_eq!(native_kind.as_str(), "parallel");
    assert_eq!(*native_state, Some(0x10));
    assert_eq!(actual, &operands);
    assert_eq!(entities.len(), 4);
    assert_eq!(entities[0], entities[3]);
    assert_eq!(entities[1], entities[2]);
    assert_ne!(entities[0], entities[1]);
}

#[test]
fn source_less_sketch_horizontal_constraint_survives_synthesis() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchEntity, SketchGeometry,
        SketchGeometryDefinition, SketchPlacement, SketchProfiles,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let sketch_id = "authored:model:sketch#1".try_into().unwrap();
    ir.model.sketches.push(Sketch {
        id: sketch_id,
        name: Some("Sketch_1".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: SketchProfiles::default(),
        native_ref: None,
    });
    let entity = "authored:model:sketch-entity#1".try_into().unwrap();
    ir.model.sketch_entities.push(SketchEntity::new(
        entity,
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    ));
    ir.model.sketch_constraints.push(SketchConstraint {
        id: "authored:model:sketch-constraint#1".try_into().unwrap(),
        sketch: ir.model.sketches[0].id.clone(),
        definition: SketchConstraintDefinitionInput::Horizontal {
            entity: ir.model.sketch_entities[0].id().clone(),
        }
        .try_into()
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("sketch constraint")));
    plan.write_to(&mut bytes).unwrap();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketch_constraints.len(), 1);
    assert!(
        matches!(decoded.ir().model.sketch_constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { entity } if entity == decoded.ir().model.sketch_entities[0].id())
    );
    let mut underflow_placement = ir.clone();
    underflow_placement.model.sketches[0].placement = SketchPlacement::try_resolved(
        Point3::new(f64::from_bits(1), 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let (output, report) =
        super::super::source_less_analytic::synthesize_document(&underflow_placement, None);
    assert!(output.ir().model.sketches.is_empty());
    assert!(report.losses.iter().any(|loss| {
        loss.message
            .contains(underflow_placement.model.sketches[0].id.as_str())
    }));
    let mut unsupported = ir.model.sketch_constraints[0].clone();
    unsupported.id = "authored:model:sketch-constraint#2".try_into().unwrap();
    unsupported.name = Some("named relation".into());
    ir.model.sketch_constraints.push(unsupported);
    let mut omitted_entity = SketchEntity::new(
        "authored:model:sketch-entity#omitted".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(20.0, 0.0),
            end: Point2::new(30.0, 0.0),
        })
        .unwrap(),
    );
    omitted_entity.geometry_ref = Some("f3d:unwritable:curve#1".into());
    let mut dependent = ir.model.sketch_constraints[0].clone();
    dependent.id = "authored:model:sketch-constraint#dependent"
        .try_into()
        .unwrap();
    dependent.definition = SketchConstraintDefinitionInput::Horizontal {
        entity: omitted_entity.id().clone(),
    }
    .try_into()
    .unwrap();
    ir.model.sketch_entities.push(omitted_entity);
    ir.model.sketch_entities.push(SketchEntity::new(
        "authored:model:sketch-entity#underflow".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(f64::from_bits(1), 0.0),
        })
        .unwrap(),
    ));
    ir.model.sketch_constraints.push(dependent);
    let mut partial_bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| { loss.message.contains("authored:model:sketch-constraint#2") }));
    for id in [
        "authored:model:sketch-entity#omitted",
        "authored:model:sketch-entity#underflow",
        "authored:model:sketch-constraint#dependent",
    ] {
        assert!(
            plan.report()
                .losses
                .iter()
                .any(|loss| loss.message.contains(id)),
            "missing loss for {id}"
        );
    }
    plan.write_to(&mut partial_bytes).unwrap();
    let partial = F3dCodec
        .decode(&mut Cursor::new(partial_bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(partial.ir().model.sketches.len(), 1);
    assert_eq!(partial.ir().model.sketch_entities.len(), 1);
    assert_eq!(partial.ir().model.sketch_constraints.len(), 1);
    assert!(
        matches!(partial.ir().model.sketch_constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { entity }
            if entity == partial.ir().model.sketch_entities[0].id())
    );
    let mut edited = decoded.ir().clone();
    edited.model.sketch_entities[0].geometry =
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 10.0),
        })
        .unwrap();
    edited.model.sketch_constraints[0].definition = SketchConstraintDefinitionInput::Vertical {
        entity: edited.model.sketch_entities[0].id().clone(),
    }
    .try_into()
    .unwrap();
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketch_constraints.len(), 1);
    assert!(
        matches!(decoded.ir().model.sketch_constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Vertical { entity } if entity == decoded.ir().model.sketch_entities[0].id())
    );
    ir.model.sketch_constraints.truncate(1);
    ir.model.sketch_entities.truncate(1);
    for (ordinal, start, end) in [
        (2, Point2::new(20.0, 0.0), Point2::new(30.0, 0.0)),
        (3, Point2::new(40.0, 0.0), Point2::new(40.0, 10.0)),
    ] {
        ir.model.sketch_entities.push(SketchEntity::new(
            format!("authored:model:sketch-entity#{ordinal}")
                .try_into()
                .unwrap(),
            ir.model.sketches[0].id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        ));
    }
    let first = ir.model.sketch_entities[0].id().clone();
    let second = ir.model.sketch_entities[1].id().clone();
    let third = ir.model.sketch_entities[2].id().clone();
    for (ordinal, definition) in [
        (
            2,
            SketchConstraintDefinitionInput::Parallel {
                first: first.clone(),
                second: second.clone(),
            },
        ),
        (
            3,
            SketchConstraintDefinitionInput::Collinear {
                first: first.clone(),
                second: second.clone(),
            },
        ),
        (
            4,
            SketchConstraintDefinitionInput::Equal {
                first: first.clone(),
                second: second.clone(),
            },
        ),
        (
            5,
            SketchConstraintDefinitionInput::Perpendicular {
                first,
                second: third,
            },
        ),
    ] {
        let mut constraint = ir.model.sketch_constraints[0].clone();
        constraint.id = format!("authored:model:sketch-constraint#{ordinal}")
            .try_into()
            .unwrap();
        constraint.definition = definition.try_into().unwrap();
        ir.model.sketch_constraints.push(constraint);
    }
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("sketch constraint")));
    plan.write_to(&mut bytes).unwrap();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let constraints = &decoded.ir().model.sketch_constraints;
    let entities = &decoded.ir().model.sketch_entities;
    assert_eq!(entities.len(), 3);
    assert_eq!(constraints.len(), 5);
    assert!(constraints.iter().any(|c| matches!(
        c.definition.kind(),
        SketchConstraintDefinitionInput::Parallel { first, second }
            if first == entities[0].id() && second == entities[1].id()
    )));
    assert!(constraints.iter().any(|c| matches!(
        c.definition.kind(),
        SketchConstraintDefinitionInput::Collinear { first, second }
            if first == entities[0].id() && second == entities[1].id()
    )));
    assert!(constraints.iter().any(|c| matches!(
        c.definition.kind(),
        SketchConstraintDefinitionInput::Equal { first, second }
            if first == entities[0].id() && second == entities[1].id()
    )));
    assert!(constraints.iter().any(|c| matches!(
        c.definition.kind(),
        SketchConstraintDefinitionInput::Perpendicular { first, second }
            if first == entities[0].id() && second == entities[2].id()
    )));
    ir.model.sketch_entities[0].construction = true;
    let mut construction_circle = SketchEntity::new(
        "authored:model:sketch-entity#circle".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(5.0, 5.0),
            radius: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
        })
        .unwrap(),
    );
    construction_circle.construction = true;
    ir.model.sketch_entities.push(construction_circle);
    let mut second_circle = SketchEntity::new(
        "authored:model:sketch-entity#circle2".try_into().unwrap(),
        ir.model.sketches[0].id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(5.0, 5.0),
            radius: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
        })
        .unwrap(),
    );
    second_circle.construction = true;
    ir.model.sketch_entities.push(second_circle);
    let line = ir.model.sketch_entities[0].id().clone();
    let circle = ir.model.sketch_entities[3].id().clone();
    let circle2 = ir.model.sketch_entities[4].id().clone();
    for (ordinal, definition) in [
        (
            6,
            SketchConstraintDefinitionInput::Tangent {
                first: line,
                second: circle.clone(),
            },
        ),
        (
            7,
            SketchConstraintDefinitionInput::Concentric {
                first: circle.clone(),
                second: circle2.clone(),
            },
        ),
        (
            8,
            SketchConstraintDefinitionInput::Equal {
                first: circle.clone(),
                second: circle2.clone(),
            },
        ),
        (
            9,
            SketchConstraintDefinitionInput::Curvature {
                first: circle,
                second: circle2,
            },
        ),
    ] {
        let mut constraint = ir.model.sketch_constraints[0].clone();
        constraint.id = format!("authored:model:sketch-constraint#{ordinal}")
            .try_into()
            .unwrap();
        constraint.definition = definition.try_into().unwrap();
        ir.model.sketch_constraints.push(constraint);
    }
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(!plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("sketch entity")));
    plan.write_to(&mut bytes).unwrap();
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let entities = &decoded.ir().model.sketch_entities;
    assert_eq!(entities.len(), 5);
    let line = entities
        .iter()
        .find(|entity| entity.geometry == ir.model.sketch_entities[0].geometry)
        .unwrap();
    assert!(line.construction);
    assert_eq!(
        entities.iter().filter(|entity| entity.construction).count(),
        3
    );
    assert!(decoded.ir().model.sketches[0].profiles.is_empty());
    let constraints = &decoded.ir().model.sketch_constraints;
    assert_eq!(constraints.len(), 9);
    let (first_circle, second_circle) = constraints
        .iter()
        .find_map(|constraint| match constraint.definition.kind() {
            SketchConstraintDefinitionInput::Concentric { first, second } => Some((first, second)),
            _ => None,
        })
        .unwrap();
    assert_ne!(first_circle, second_circle);
    for id in [first_circle, second_circle] {
        let entity = entities.iter().find(|entity| entity.id() == id).unwrap();
        assert!(entity.construction);
        assert_eq!(entity.geometry, ir.model.sketch_entities[3].geometry);
    }
    assert!(constraints.iter().any(|constraint| matches!(constraint.definition.kind(),
        SketchConstraintDefinitionInput::Tangent { first, second } if first == line.id() && second == first_circle)));
    assert!(constraints.iter().any(|constraint| matches!(constraint.definition.kind(),
        SketchConstraintDefinitionInput::Equal { first, second } if first == first_circle && second == second_circle)));
    assert!(constraints.iter().any(|constraint| matches!(constraint.definition.kind(),
        SketchConstraintDefinitionInput::Curvature { first, second } if first == first_circle && second == second_circle)));
}
