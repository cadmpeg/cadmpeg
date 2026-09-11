// SPDX-License-Identifier: Apache-2.0
//! Hole construction and placement semantic-writer round-trips.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_round_trips_typed_simple_blind_hole() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, HoleKind, LinearTermination};

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
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            }, Some(actual_diameter),) if (placements.is_none()) && actual_diameter.get() == 6.35 && actual_length.get() == 12.0)));

    {
        let mut ir = decoded.ir_mut();
        ir.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
                definition
            else {
                panic!("typed hole feature");
            };
            shape
                .try_edit(|_, _, diameter| {
                    *diameter = Some(cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap());
                })
                .unwrap();
            *extent = Some(LinearTermination::Blind {
                length: cadmpeg_ir::scalar::NonZeroLength::new(16.0).unwrap(),
            });
        });
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
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Diameter"], "8mm");
    assert_eq!(feature.parameters["Depth"], "16mm");
}

#[test]
fn semantic_writer_retains_partial_native_hole_construction() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, HoleKind, LinearTermination};

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
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            }, None,))));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,

            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::PartialCounterbore(
                    cadmpeg_ir::features::PartialPair::First(actual_diameter),
                ),
                ..
            }, Some(actual_diameter_2),) if actual_diameter.get() == 10.0 && actual_diameter_2.get() == 6.0)));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            ref placements,
            shape,

            extent: None,
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Unresolved(None),
                ..
            }, Some(actual_diameter),) if (placements.is_none()) && actual_diameter.get() == 5.0)));

    for (index, message) in [
        (0, "unresolved hole diameter"),
        (1, "unresolved hole entry construction"),
    ] {
        let mut detached = decoded.ir().clone();
        detached.model.features[index].native_ref = None;
        let error = crate::test_support::plan_inherited_write(
            &detached,
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(message));
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
    let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::Simple;

    *shape = cadmpeg_ir::features::HoleShape::new(
        edited_construction,
        *shape.exit_kind(),
        shape.diameter(),
    )
    .unwrap();
    updated_detached_evaluation.set_definition(updated_detached_definition);
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("unresolved hole termination"));

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
        FaceSelection, FeatureDefinition, FeatureOperation, HolePlacement, LinearTermination,
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
    crate::test_support::plan_inherited_write(
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
    assert!(!native.parameters.contains_key("Depth"));
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
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, HoleKind, LinearTermination};

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
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Counterbore {
                    diameter: actual_diameter,
                    depth: actual_depth,
                },
                ..
            },) if actual_diameter.get() == 10.0 && actual_depth.get() == 4.0 && actual_length.get() == 20.0)));
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::ThroughAll {}),
            ..
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Countersink {
                    diameter: actual_diameter,
                    angle: value,
                },
                ..
            },) if ((value.get() - 82f64.to_radians()).abs() < 1.0e-12) && actual_diameter.get() == 9.0)));

    {
        let mut ir = decoded.ir_mut();
        let updated_ir_evaluation = &mut ir.model.features[0].evaluation;
        let mut updated_ir_definition = updated_ir_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
            &mut updated_ir_definition
        else {
            panic!("counterbore hole");
        };
        let mut edited_construction = shape.construction().clone();
        let construction = &mut edited_construction;
        let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
            panic!("ordinary hole form");
        };
        *kind = HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(12.0).unwrap(),
            depth: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
        };
        *extent = Some(LinearTermination::ThroughAll {});

        *shape = cadmpeg_ir::features::HoleShape::new(
            edited_construction,
            *shape.exit_kind(),
            shape.diameter(),
        )
        .unwrap();
        updated_ir_evaluation.set_definition(updated_ir_definition);
        let updated_ir_evaluation = &mut ir.model.features[1].evaluation;
        let mut updated_ir_definition = updated_ir_evaluation.definition().clone();
        let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
            &mut updated_ir_definition
        else {
            panic!("countersink hole");
        };
        let mut edited_construction = shape.construction().clone();
        let construction = &mut edited_construction;
        let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
            panic!("ordinary hole form");
        };
        *kind = HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(11.0).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(90f64.to_radians()).unwrap(),
        };
        *extent = Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(25.0).unwrap(),
        });

        *shape = cadmpeg_ir::features::HoleShape::new(
            edited_construction,
            *shape.exit_kind(),
            shape.diameter(),
        )
        .unwrap();
        updated_ir_evaluation.set_definition(updated_ir_definition);
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
    let features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(features[0].properties["EndCondition"], "ThroughAll");
    assert!(!features[0].parameters.contains_key("Depth"));
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
