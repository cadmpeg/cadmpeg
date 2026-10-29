// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::{PmDcFeaturePropertyKind as Kind, PmDcFeatureReferenceFamily as Family};
use crate::test_support::sheet_metal::{list, property, reference, Fixture};
use cadmpeg_ir::features::{FeatureContent, FeatureEvaluation, GeneratedEdgeRef, PlanarProfileRef};

#[test]
fn paired_inputs_select_their_own_complete_independent_rows() {
    let mut fixture = Fixture::new();
    fixture.properties.extend([
        property(
            11,
            &Kind::References {
                family: Family::SheetMetalAttachmentSelections,
                items: list(&[51]),
            },
        ),
        property(
            51,
            &Kind::SheetMetal {
                definition: super::super::SheetMetalProperty::EdgeSelection {
                    edges: reference(52),
                },
            },
        ),
        property(
            52,
            &Kind::References {
                family: Family::EdgeCollection,
                items: list(&[53]),
            },
        ),
        property(
            53,
            &Kind::EdgeItem {
                index_references: crate::pmdc::PmDcU32List::new(
                    2,
                    Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                    vec![61],
                )
                .expect("indexes"),
                index_reference_value: -1,
                value: 0,
            },
        ),
    ]);
    let values = operands::plate(
        &fixture.plate,
        &fixture.style,
        |r| fixture.property(r),
        Fixture::length,
        Fixture::direction,
        cadmpeg_ir::units::UnitVector3::Z_AXIS,
        cadmpeg_ir::units::UnitVector3::X_AXIS,
    )
    .expect("operands");
    let selection = EdgeSelection::generated(
        vec![GeneratedEdgeRef {
            feature: cadmpeg_ir::features::FeatureId::mint("test:design:feature#base")
                .expect("producer"),
            local_id: cadmpeg_ir::features::SelectionReference::new("cap".to_owned())
                .expect("name"),
        }],
        fixture.property(reference(11)).expect("selection").id(),
    )
    .expect("generated");
    let plate_id = crate::feature::history_feature_id(&fixture.plate.identity).expect("plate");
    let bend_id = crate::feature::history_feature_id(&fixture.bend.identity).expect("bend");
    let make_feature = |id, ordinal, native_ref, operation| Feature {
        id,
        ordinal,
        native_ref: Some(native_ref),
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
    };
    let plate = make_feature(
        plate_id.clone(),
        0,
        fixture.plate.id(),
        FeatureOperation::SheetMetalAttachedPlate {
            profile: PlanarProfileRef::Sketch(
                cadmpeg_ir::sketches::SketchId::mint("test:design:sketch#wall").expect("sketch"),
            ),
            thickness: values.thickness,
            side: values.side,
            attachment: selection.clone(),
        },
    );
    let bend = make_feature(
        bend_id.clone(),
        1,
        fixture.bend.id(),
        FeatureOperation::SheetMetalBend {
            plate: plate_id.clone(),
            attachment: selection,
            inside_radius: values.inside_radius,
            relief: values.relief,
            transition: values.transition,
        },
    );
    let sample = crate::feature::edge_selection::geometry::EdgeGeometry {
        start: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            0.0, 0.0, 0.0,
        ))
        .expect("start"),
        end: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(8.0, 0.0, 0.0))
            .expect("end"),
        interior: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            4.0, 0.0, 0.0,
        ))
        .expect("middle"),
        tangent: cadmpeg_ir::units::UnitVector3::X_AXIS,
    };
    let inventory = FeatureInventory {
        features: vec![fixture.plate, fixture.bend],
        properties: fixture.properties,
        edge_inputs: crate::feature::edge_selection::EdgeInventory {
            generated_sources: None,
            generated_sources_complete: false,
            names: std::collections::HashMap::default(),
            geometry: std::collections::HashMap::from([(
                ("fixture".to_owned(), 61),
                sample.clone(),
            )]),
        },
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("context");
    let mut features = vec![plate, bend];
    let inputs = project(&ctx, &mut features, &inventory).expect("inputs");
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].input_of, plate_id);
    assert_eq!(inputs[1].input_of, bend_id);
    assert_eq!(inputs[0].edge_geometry, inputs[1].edge_geometry);
    assert_eq!(inputs[0].edge_geometry[0].interior, sample.interior);
    let attachment = |feature: &Feature| match feature.evaluation.definition() {
        FeatureDefinition::Operation(
            FeatureOperation::SheetMetalAttachedPlate { attachment, .. }
            | FeatureOperation::SheetMetalBend { attachment, .. },
        ) => attachment.clone(),
        _ => panic!("feature"),
    };
    assert_ne!(attachment(&features[0]), attachment(&features[1]));
    assert!(
        matches!(attachment(&features[0]),EdgeSelection::Historical { state,.. } if state==inputs[0].id)
    );
    assert!(
        matches!(attachment(&features[1]), EdgeSelection::Historical { state, .. } if state == inputs[1].id)
    );
}
