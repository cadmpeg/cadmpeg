// SPDX-License-Identifier: Apache-2.0
//! Hole construction and placement semantic-writer round-trips.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

#[test]
fn semantic_writer_round_trips_typed_simple_blind_hole() {
    use cadmpeg_ir::features::{
        holes::HoleKind, FeatureDefinition, FeatureOperation, LinearTermination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Hole Name="Drill" Type="Hole" id="15"><Dimension Name="Diameter">0.25in</Dimension><Dimension Name="Depth">12mm</Dimension></Hole></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            face: None,
            ref placements,
            shape,
            extent: Some(LinearTermination::Blind {
                length: actual_length,
            }),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            }, Some(actual_diameter),) if (placements.is_none()) && decoded.ir().dimension_value(&actual_diameter).unwrap().get() == 6.35 && decoded.ir().dimension_value(&actual_length).unwrap().get() == 12.0)));

    {
        let mut ir = decoded.ir_mut();
        let owner = ir.model.features[0].id.clone();
        ir.set_dimension(
            &owner,
            "/shape/diameter",
            cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
        )
        .unwrap();
        ir.set_dimension(
            &owner,
            "/extent/length",
            cadmpeg_ir::scalar::NonZeroLength::new(16.0).unwrap(),
        )
        .unwrap();
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Diameter"], "8mm");
    assert_eq!(feature.parameters["Depth"], "16mm");
}

#[test]
fn semantic_writer_retains_partial_native_hole_construction() {
    use cadmpeg_ir::features::{
        holes::HoleKind, FeatureDefinition, FeatureOperation, LinearTermination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Hole Name="Unknown diameter" Type="Hole" id="61" EndCondition="ThroughAll"><Dimension Name="Diameter">NaNmm</Dimension></Hole>
            <Hole Name="Partial counterbore" Type="Hole" id="62" EndCondition="ThroughAll"><Dimension Name="Diameter">6mm</Dimension><Dimension Name="CounterboreDiameter">10mm</Dimension><Dimension Name="CounterboreDepth">NaNmm</Dimension></Hole>
            <Hole Name="Conflicting entry" Type="Hole" id="63" EndCondition="Future" Position="invalid" Direction="0,0,0"><Dimension Name="Diameter">5mm</Dimension><Dimension Name="CounterboreDiameter">11mm</Dimension><Dimension Name="CounterboreDepth">3mm</Dimension><Dimension Name="CountersinkDiameter">9mm</Dimension><Dimension Name="CountersinkAngle">82deg</Dimension></Hole>
        </Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            }, None,))));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::PartialCounterbore(
                    cadmpeg_ir::features::holes::PartialPair::First(actual_diameter),
                ),
                ..
            }, Some(actual_diameter_2),) if decoded.ir().dimension_value(&actual_diameter).unwrap().get() == 10.0 && decoded.ir().dimension_value(&actual_diameter_2).unwrap().get() == 6.0)));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            ref placements,
            shape,
            extent: None,
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::Unresolved(None),
                ..
            }, Some(actual_diameter),) if (placements.is_none()) && decoded.ir().dimension_value(&actual_diameter).unwrap().get() == 5.0)));

    for index in 0..2 {
        let mut detached = decoded.ir().clone();
        detached.model.features[index].native_ref = None;
        crate::test_support::assert_synthesis_fallback(&detached, decoded.source_fidelity());
    }
    let mut detached = decoded.ir().clone();
    detached.model.features[2].native_ref = None;
    let updated_detached_evaluation = &mut detached.model.features[2].evaluation;
    let mut updated_detached_definition = updated_detached_evaluation.definition().clone();
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, .. }) =
        &mut updated_detached_definition
    else {
        panic!("partial hole");
    };
    let mut edited_construction = shape.construction().clone();
    let construction = &mut edited_construction;
    let cadmpeg_ir::features::holes::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::Simple;

    *shape = cadmpeg_ir::features::holes::HoleShape::new(
        edited_construction,
        shape.exit_kind().clone(),
        shape.diameter().cloned(),
    )
    .unwrap();
    updated_detached_evaluation.set_definition(updated_detached_definition);
    crate::test_support::assert_synthesis_fallback(&detached, decoded.source_fidelity());

    for (index, feature) in decoded.ir_mut().model.features.iter_mut().enumerate() {
        feature.name = Some(format!("Renamed hole {}", index + 1));
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].parameters["Diameter"], "NaNmm");
    assert_eq!(native[1].parameters["CounterboreDepth"], "NaNmm");
    assert_eq!(native[2].properties["EndCondition"], "Future");
    assert_eq!(native[2].properties["Position"], "invalid");
    assert_eq!(native[2].properties["Direction"], "0,0,0");
    assert_eq!(native[2].parameters["CounterboreDiameter"], "11mm");
    assert_eq!(native[2].parameters["CountersinkDiameter"], "9mm");
}

#[test]
fn semantic_writer_round_trips_hole_placement() {
    use cadmpeg_ir::features::{
        holes::HolePlacement, FaceSelection, FeatureDefinition, FeatureOperation, LinearTermination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Hole Name="Placed" Type="Hole" id="28" Face="face:12" Position="1mm,2mm,3mm" Direction="0,0,-1" EndCondition="Blind"><Dimension Name="Diameter">6mm</Dimension><Dimension Name="Depth">10mm</Dimension></Hole></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let depth_id = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap()
        .id
        .clone();
    {
        let mut ir = decoded.ir_mut();
        let updated_ir_evaluation = &mut ir.model.features[0].evaluation;
        let mut updated_ir_definition = updated_ir_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Hole {
            face,
            placements,
            extent,
            ..
        }) = &mut updated_ir_definition
        else {
            panic!("typed hole feature");
        };
        assert_eq!(face, &Some(FaceSelection::Native("face:12".into())));
        assert_eq!(
            placements.as_deref(),
            Some(
                &[HolePlacement::Directed {
                    position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                        .unwrap(),
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, -1.0
                    ))
                    .unwrap(),
                }][..]
            )
        );

        *face = Some(FaceSelection::Native("face:13".into()));
        *placements = Some(vec![HolePlacement::Directed {
            position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0))
                .unwrap(),
        }]);
        *extent = Some(LinearTermination::ThroughAll {});
        updated_ir_evaluation.set_definition(updated_ir_definition);
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Face"], "face:13");
    assert_eq!(native.properties["Position"], "4mm,5mm,6mm");
    assert_eq!(native.properties["Direction"], "0,1,0");
    assert_eq!(native.properties["EndCondition"], "ThroughAll");
    // D1: explicit ThroughAll removes the operation use, not its independent parameter.
    assert!(native.parameters.contains_key("Depth"));
    let depth = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id == depth_id)
        .expect("same independent depth parameter");
    assert_eq!(depth.name, "Depth");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(10.0).unwrap()
        ))
    );
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Hole {
            face: Some(FaceSelection::Native(face)),
            placements,
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if face == "face:13"
            && placements.as_deref() == Some(&[HolePlacement::Directed {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap(),
            }][..])
    ));
}

#[test]
fn semantic_writer_round_trips_counterbore_and_countersink_holes() {
    use cadmpeg_ir::features::{
        holes::HoleKind, FeatureDefinition, FeatureOperation, LinearTermination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Hole Name="Counterbore" Type="Hole" id="51" EndCondition="Blind"><Dimension Name="Diameter">6mm</Dimension><Dimension Name="Depth">20mm</Dimension><Dimension Name="CounterboreDiameter">10mm</Dimension><Dimension Name="CounterboreDepth">4mm</Dimension></Hole>
            <Hole Name="Countersink" Type="Hole" id="52" EndCondition="ThroughAll"><Dimension Name="Diameter">5mm</Dimension><Dimension Name="CountersinkDiameter">9mm</Dimension><Dimension Name="CountersinkAngle">82deg</Dimension></Hole>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::Blind {
                length: actual_length,
            }),
            ..
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::Counterbore {
                    diameter: actual_diameter,
                    depth: actual_depth,
                },
                ..
            },) if decoded.ir().dimension_value(&actual_diameter).unwrap().get() == 10.0 && decoded.ir().dimension_value(&actual_depth).unwrap().get() == 4.0 && decoded.ir().dimension_value(&actual_length).unwrap().get() == 20.0)));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::Countersink {
                    diameter: actual_diameter,
                    angle: value,
                },
                ..
            },) if ((decoded.ir().dimension_value(&value).unwrap().get() - 82f64.to_radians()).abs() < 1.0e-12) && decoded.ir().dimension_value(&actual_diameter).unwrap().get() == 9.0)));

    let depth_id = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .unwrap()
        .id
        .clone();
    {
        let mut ir = decoded.ir_mut();
        for (index, path, value) in [
            (0, "/shape/construction/kind/diameter", 12.0),
            (0, "/shape/construction/kind/depth", 5.0),
            (1, "/shape/construction/kind/diameter", 11.0),
        ] {
            let owner = ir.model.features[index].id.clone();
            ir.set_dimension(
                &owner,
                path,
                cadmpeg_ir::scalar::PositiveLength::new(value).unwrap(),
            )
            .unwrap();
        }
        let owner = ir.model.features[1].id.clone();
        ir.set_dimension(
            &owner,
            "/shape/construction/kind/angle",
            cadmpeg_ir::scalar::InteriorAngle::new(90f64.to_radians()).unwrap(),
        )
        .unwrap();
        let updated_ir_evaluation = &mut ir.model.features[0].evaluation;
        let mut updated_ir_definition = updated_ir_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, .. }) =
            &mut updated_ir_definition
        else {
            panic!("counterbore hole");
        };
        *extent = Some(LinearTermination::ThroughAll {});

        updated_ir_evaluation.set_definition(updated_ir_definition);
        let updated_ir_evaluation = &mut ir.model.features[1].evaluation;
        let mut updated_ir_definition = updated_ir_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Hole { extent, .. }) =
            &mut updated_ir_definition
        else {
            panic!("countersink hole");
        };
        *extent = Some(LinearTermination::Blind {
            length: cadmpeg_ir::Dimension::Literal(
                cadmpeg_ir::scalar::NonZeroLength::new(25.0).unwrap(),
            ),
        });

        updated_ir_evaluation.set_definition(updated_ir_definition);
    }

    let mut encoded = Vec::new();
    crate::test_support::serialize_edited_history(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(features[0].properties["EndCondition"], "ThroughAll");
    // D1: ThroughAll removes use while the independent original20mm parameter survives.
    assert!(features[0].parameters.contains_key("Depth"));
    let depth = regenerated
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.id == depth_id)
        .expect("same independent counterbore depth parameter");
    assert_eq!(depth.name, "Depth");
    assert_eq!(
        depth.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(20.0).unwrap()
        ))
    );
    assert!(matches!(
        regenerated.ir().model.features[0].definition().operation(),
        FeatureOperation::Hole {
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }
    ));
    assert_eq!(features[0].parameters["CounterboreDiameter"], "12mm");
    assert_eq!(features[0].parameters["CounterboreDepth"], "5mm");
    assert_eq!(features[1].properties["EndCondition"], "Blind");
    assert_eq!(features[1].parameters["Depth"], "25mm");
    assert_eq!(features[1].parameters["CountersinkDiameter"], "11mm");
    assert_eq!(
        features[1].parameters["CountersinkAngle"],
        format!("{}rad", 90f64.to_radians())
    );
}
