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

/// A write that keeps the source's history and generates the sketches, so
/// leaves out a driven parameter, writes a parameter whose expression reads it with its value,
/// leaves out a reader with no value, and names both; the part reads back
/// with no expression that reads a parameter it does not hold.
#[test]
fn a_kept_history_writes_a_reader_of_a_driven_parameter_with_its_value() {
    let mut source = crate::test_support::container::sldprt_with_body(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Equations" Type="EquationDriven" id="7"><Dimension Name="Input">2</Dimension><Dimension Name="Result">Input + 1</Dimension><Dimension Name="Spare">Input * 3</Dimension></Feature></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let id_of = |ir: &cadmpeg_ir::CadIr, name: &str| {
        ir.model
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("{name} in {:?}", ir.model.parameters))
            .id
            .clone()
    };
    let input = id_of(decoded.ir(), "Input");
    let result = id_of(decoded.ir(), "Result");
    let spare = id_of(decoded.ir(), "Spare");
    {
        // Decode records the sketch baselines; a document without them that
        // keeps the history baseline is one whose sketches the write
        // generates, which leaves driven parameters out.
        let digests = &mut decoded.source_fidelity_mut().codec_digests;
        for baseline in [
            "sldprt_neutral_sketch_local_sha256",
            "sldprt_native_sketch_sha256",
            "sldprt_neutral_sketch_constraint_local_sha256",
        ] {
            digests.remove(baseline);
        }
        assert!(digests.contains_key("sldprt_native_history_sha256"));
        let mut ir = decoded.ir_mut();
        // The design states `Input` driven (D11).
        ir.design
            .parameters
            .retain(|parameter| parameter.id != input);
        ir.design
            .parameters
            .push(cadmpeg_ir::design::parameter::DesignParameter {
                id: input.clone(),
                scope: cadmpeg_ir::ids::ProductDefinitionId::mint("synthetic:test:id#part")
                    .unwrap(),
                owner: None,
                name: Some(cadmpeg_core::nonblank_literal!("Input")),
                kind: cadmpeg_ir::design::units::ValueKind::Integer,
                unit: None,
                definition: cadmpeg_ir::design::parameter::ParameterDefinition::Driven,
                value: None,
                display: None,
                tolerance: None,
                decimals: None,
                comment: None,
                native_options: std::collections::BTreeMap::new(),
                source_settings: std::collections::BTreeMap::new(),
                native_ref: None,
            });
        for parameter in &mut ir.model.parameters {
            if parameter.id == result {
                parameter.expression = "Input + 2".into();
                parameter.value = Some(ParameterValue::Integer(4));
            } else if parameter.id == spare {
                parameter.value = None;
            }
        }
    }
    let plan = crate::test_support::plan_fixture_with_native_envelope(
        cadmpeg_ir::codec::write::EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
        cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
    )
    .unwrap();
    let losses = plan.report().losses.clone();
    let named = |code: crate::loss::SldprtLossCode, id: &cadmpeg_ir::features::ParameterId| {
        losses
            .iter()
            .any(|loss| loss.code == code.kind() && loss.message.ends_with(id.as_str()))
    };
    assert!(
        named(
            crate::loss::SldprtLossCode::WriterParameterExpressionsOmitted,
            &result
        ),
        "{losses:#?}"
    );
    assert!(
        named(crate::loss::SldprtLossCode::WriterFeaturesOmitted, &spare),
        "{losses:#?}"
    );
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded).unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let parameters = &regenerated.ir().model.parameters;
    let names = parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Result"], "{parameters:?}");
    assert_eq!(parameters[0].expression, "4");
    assert!(parameters[0].dependencies.is_empty());
}
