// SPDX-License-Identifier: Apache-2.0
//! A driven (reference) dimension through a full write that generates the
//! sketch lanes, read back.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId,
    SketchLocus,
};

use crate::loss::SldprtLossCode;
use crate::SldprtCodec;

/// A sketch feature whose sketch holds a circle profile and three points:
/// the driving dimension `D1` measures 4 mm between two of them, and the
/// driven dimension `D2` measures 5 mm between two others. `D2`'s record
/// states the literal `6mm`, as the record of a Fusion driven dimension can
/// state a value its geometry no longer measures.
fn sketch_with_a_driven_dimension() -> cadmpeg_ir::CadIr {
    let mut ir = crate::test_support::ir::source_less_cube();
    let sketch_id = SketchId::mint("synthetic:test:sketch#driven").unwrap();
    let entity = |key: &str, definition: SketchGeometryDefinition| {
        let id = SketchEntityId::mint(format!("synthetic:test:sketch-entity#{key}")).unwrap();
        (
            id.clone(),
            SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::try_from(definition).unwrap(),
            ),
        )
    };
    let point = |key: &str, u: f64, v: f64| {
        entity(
            key,
            SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            },
        )
    };
    let (circle, circle_entity) = entity(
        "circle",
        SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        },
    );
    let (origin, origin_entity) = point("origin", 30.0, 0.0);
    let (along, along_entity) = point("along", 34.0, 0.0);
    let (up, up_entity) = point("up", 30.0, 5.0);
    ir.model
        .sketch_entities
        .extend([circle_entity, origin_entity, along_entity, up_entity]);
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: Some("Driven".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: circle,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    });
    let feature = FeatureId::mint("synthetic:test:feature#driven").unwrap();
    ir.model.features.push(Feature {
        product_definition: None,
        id: feature.clone(),
        ordinal: 0,
        name: Some("Driven".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id.clone())),
            }),
        ),
        native_ref: None,
    });
    for (ordinal, key, name, millimetres, first, second, driving) in [
        (0, "driving", "D1", 4.0, &origin, &along, None),
        (1, "driven", "D2", 6.0, &origin, &up, Some(false)),
    ] {
        let parameter = ParameterId::mint(format!("synthetic:test:parameter#{key}")).unwrap();
        ir.model.parameters.push(DesignParameter {
            product_definition: None,
            id: parameter.clone(),
            owner: Some(feature.clone()),
            ordinal,
            name: name.into(),
            expression: format!("{millimetres}mm"),
            display: None,
            value: Some(ParameterValue::Length(Length::new(millimetres).unwrap())),
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId::mint(format!("synthetic:test:constraint#{key}")).unwrap(),
            sketch: sketch_id.clone(),
            definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
                SketchConstraintDefinitionInput::DistanceLoci {
                    first: SketchLocus::Entity(first.clone()),
                    second: SketchLocus::Entity(second.clone()),
                    parameter,
                },
            )
            .unwrap(),
            name: None,
            driving,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }
    ir.model
        .sketch_entities
        .sort_by(|first, second| first.id().cmp(second.id()));
    ir.model
        .parameters
        .sort_by(|first, second| first.id.cmp(&second.id));
    ir.model
        .sketch_constraints
        .sort_by(|first, second| first.id.cmp(&second.id));
    ir
}

/// The write omits the driven dimension and its parameter, and charges
/// both, so no editable parameter stands for it in the written part; the
/// driving dimension's parameter is written and read back.
#[test]
fn no_editable_parameter_stands_for_a_driven_dimension_in_a_written_part() {
    let ir = sketch_with_a_driven_dimension();
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        EncodeInput::new(&ir, None),
        TargetRequest::Inherit,
    )
    .unwrap();
    let charged = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::WriterDrivenDimensionsOmitted.kind())
        .map(|loss| loss.message.clone())
        .collect::<Vec<_>>();
    for prefix in [
        "1 driven (reference) sketch dimension record(s)",
        "1 driven (reference) dimension parameter record(s)",
    ] {
        assert!(
            charged.iter().any(|message| message.starts_with(prefix)),
            "{prefix}: {charged:?}"
        );
    }
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let decoded = decoded.ir();
    assert!(
        decoded.model.parameters.iter().any(|parameter| {
            parameter.name == "D1"
                && parameter.value == Some(ParameterValue::Length(Length::new(4.0).unwrap()))
        }),
        "{:?}",
        decoded.model.parameters
    );
    assert!(
        !decoded
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.name == "D2"),
        "{:?}",
        decoded.model.parameters
    );
    assert!(
        !decoded.design.parameters.iter().any(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some("D2")
                && !matches!(
                    parameter.definition,
                    cadmpeg_ir::design::parameter::ParameterDefinition::Driven
                )
        }),
        "{:?}",
        decoded.design.parameters
    );
}
