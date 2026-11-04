// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::features::{
    edge_treatments::RadiusSpec, BooleanOp, ExtrudeExtent, FeatureDefinition, FeatureOperation,
    LinearTermination, ParameterValue, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::sketches::{SketchConstraintDefinitionInput, SketchGeometryDefinition};
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::FcstdCodec;

const LENGTH_TOLERANCE: f64 = 1.0e-12;
const VOLUME_TOLERANCE: f64 = 1.0e-6;

#[test]
fn authored_design_history_keeps_sketches_constraints_and_operations() {
    let bytes = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/design_history.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    assert_eq!(model.sketches.len(), 2);
    let sketch = |name: &str| {
        model
            .sketches
            .iter()
            .find(|sketch| sketch.name.as_deref() == Some(name))
            .unwrap()
    };
    let pad = sketch("PadSketch");
    let pocket = sketch("PocketSketch");
    let entities = |sketch: &cadmpeg_ir::sketches::Sketch| {
        model
            .sketch_entities
            .iter()
            .filter(|entity| entity.sketch == sketch.id)
            .collect::<Vec<_>>()
    };
    let rectangle = entities(pad);
    assert_eq!(rectangle.len(), 4);
    for (entity, (start, end)) in rectangle.iter().zip([
        ([0.0, 0.0], [24.0, 0.0]),
        ([24.0, 0.0], [24.0, 16.0]),
        ([24.0, 16.0], [0.0, 16.0]),
        ([0.0, 16.0], [0.0, 0.0]),
    ]) {
        let SketchGeometryDefinition::Line {
            start: actual_start,
            end: actual_end,
        } = entity.geometry.definition()
        else {
            panic!("rectangle side");
        };
        for (actual, expected) in [
            actual_start.get().u,
            actual_start.get().v,
            actual_end.get().u,
            actual_end.get().v,
        ]
        .into_iter()
        .zip(start.into_iter().chain(end))
        {
            assert!((actual - expected).abs() < LENGTH_TOLERANCE);
        }
    }
    let circle = entities(pocket);
    assert_eq!(circle.len(), 1);
    assert!(
        matches!(circle[0].geometry.definition(), SketchGeometryDefinition::Circle { center, radius }
        if (center.get().u - 12.0).abs() < LENGTH_TOLERANCE
        && (center.get().v - 8.0).abs() < LENGTH_TOLERANCE
        && (radius.get() - 3.0).abs() < LENGTH_TOLERANCE)
    );
    let (origin, _, _) = pocket.placement.resolved().unwrap();
    assert!((origin.get().z - 10.0).abs() < LENGTH_TOLERANCE);
    let pad_constraints: Vec<_> = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == pad.id)
        .collect();
    let pocket_constraints: Vec<_> = model
        .sketch_constraints
        .iter()
        .filter(|constraint| constraint.sketch == pocket.id)
        .collect();
    assert_eq!(pad_constraints.len(), 4);
    assert_eq!(pocket_constraints.len(), 1);
    assert!(pad_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Horizontal { entity } if entity == rectangle[0].id())));
    assert!(pad_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Vertical { entity } if entity == rectangle[1].id())));
    let value = |parameter: &cadmpeg_ir::features::ParameterId| {
        let parameter = model
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter)
            .unwrap();
        let Some(ParameterValue::Length(value)) = &parameter.value else {
            panic!("length parameter")
        };
        value.get()
    };
    for (entity, expected) in [(rectangle[0], 24.0), (rectangle[1], 16.0)] {
        assert!(pad_constraints.iter().any(|constraint| matches!(constraint.definition.kind(), SketchConstraintDefinitionInput::Distance { entities, parameter } if entities.as_slice() == [entity.id().clone()] && (value(parameter) - expected).abs() < LENGTH_TOLERANCE)));
    }
    assert!(
        matches!(pocket_constraints[0].definition.kind(), SketchConstraintDefinitionInput::Diameter { entity, parameter } if entity == circle[0].id() && (value(parameter) - 6.0).abs() < LENGTH_TOLERANCE)
    );

    let operation = |name: &str| {
        let definition = model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .evaluation
            .definition();
        match definition {
            FeatureDefinition::Operation(operation)
            | FeatureDefinition::PostProcess { operation, .. } => operation,
        }
    };
    for (name, expected_sketch, expected_op, expected_length) in [
        ("Pad", &pad.id, BooleanOp::Join, 10.0),
        ("Pocket", &pocket.id, BooleanOp::Cut, 6.0),
    ] {
        assert!(
            matches!(operation(name), FeatureOperation::Extrude { profile: ProfileRef::Planar(PlanarProfileRef::Sketch(sketch)), op, extent: ExtrudeExtent::OneSided { side }, .. } if sketch == expected_sketch && *op == expected_op && matches!(side.termination, LinearTermination::Blind { length } if (length.get() - expected_length).abs() < LENGTH_TOLERANCE)),
            "{name}"
        );
    }
    let measures =
        cadmpeg_measure::test_support::round_trip::measured("design_history", decoded.ir())
            .unwrap();
    for (name, expected) in [
        ("Pad", 24.0 * 16.0 * 10.0),
        (
            "Pocket",
            24.0 * 16.0 * 10.0 - std::f64::consts::PI * 3.0 * 3.0 * 6.0,
        ),
    ] {
        let feature = model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap();
        let [body] = feature.evaluation.outputs().as_slice() else {
            panic!(
                "{name}: expected one result body, got {:?}",
                feature.evaluation.outputs()
            )
        };
        let body = measures
            .bodies()
            .iter()
            .find(|row| row.body() == body.as_str())
            .unwrap();
        let volume = body
            .volume()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (volume - expected).abs() < VOLUME_TOLERANCE,
            "{name}: volume {volume}"
        );
    }
    assert!(
        matches!(operation("Fillet"), FeatureOperation::Fillet { groups } if matches!(groups.as_slice(), [group] if matches!(group.radius, RadiusSpec::Constant { radius } if (radius.get() - 1.0).abs() < LENGTH_TOLERANCE)))
    );
}
