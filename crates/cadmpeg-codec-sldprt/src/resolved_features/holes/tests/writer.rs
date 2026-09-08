// SPDX-License-Identifier: Apache-2.0
//! Hole construction and placement semantic-writer round-trips.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_round_trips_typed_simple_blind_hole() {
    use cadmpeg_ir::features::{FeatureDefinition, HoleKind, Length, LinearTermination};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Hole {
            face: None,
            ref placements,
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            },
            diameter: Some(Length(6.35)),
            extent: Some(LinearTermination::Blind {
                length: Length(12.0),
            }),
            ..
        } if placements.is_none()
    ));

    {
        let mut ir = decoded.ir_mut();
        let FeatureDefinition::Hole {
            diameter, extent, ..
        } = &mut ir.model.features[0].definition
        else {
            panic!("typed hole feature");
        };
        *diameter = Some(Length(8.0));
        *extent = Some(LinearTermination::Blind {
            length: Length(16.0),
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
    use cadmpeg_ir::features::{FeatureDefinition, HoleKind, Length, LinearTermination};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            },
            diameter: None,
            extent: Some(LinearTermination::ThroughAll),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::PartialCounterbore {
                    diameter: Some(Length(10.0)),
                    depth: None,
                },
                ..
            },
            diameter: Some(Length(6.0)),
            extent: Some(LinearTermination::ThroughAll),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Hole {
            ref placements,
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Unresolved(None),
                ..
            },
            diameter: Some(Length(5.0)),
            extent: None,
            ..
        } if placements.is_none()
    ));

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
    let FeatureDefinition::Hole { construction, .. } = &mut detached.model.features[2].definition
    else {
        panic!("partial hole");
    };
    let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
        panic!("ordinary hole form");
    };
    *kind = HoleKind::Simple;
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
        FaceSelection, FeatureDefinition, HolePlacement, LinearTermination,
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
        let FeatureDefinition::Hole {
            face,
            placements,
            extent,
            ..
        } = &mut ir.model.features[0].definition
        else {
            panic!("typed hole feature");
        };
        assert_eq!(face, &Some(FaceSelection::Native("face:12".into())));
        assert_eq!(
            placements.as_deref(),
            Some(
                &[HolePlacement::Directed {
                    position: Point3::new(1.0, 2.0, 3.0),
                    direction: Vector3::new(0.0, 0.0, -1.0),
                }][..]
            )
        );

        *face = Some(FaceSelection::Native("face:13".into()));
        *placements = Some(vec![HolePlacement::Directed {
            position: Point3::new(4.0, 5.0, 6.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
        }]);
        *extent = Some(LinearTermination::ThroughAll);
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
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Hole {
            face: Some(FaceSelection::Native(face)),
            placements,
            extent: Some(LinearTermination::ThroughAll),
            ..
        } if face == "face:13"
            && placements.as_deref() == Some(&[HolePlacement::Directed {
                position: Point3::new(4.0, 5.0, 6.0),
                direction: Vector3::new(0.0, 1.0, 0.0),
            }][..])
    ));
}

#[test]
fn semantic_writer_round_trips_counterbore_and_countersink_holes() {
    use cadmpeg_ir::features::{Angle, FeatureDefinition, HoleKind, Length, LinearTermination};

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
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Counterbore {
                    diameter: Length(10.0),
                    depth: Length(4.0),
                },
                ..
            },
            extent: Some(LinearTermination::Blind {
                length: Length(20.0),
            }),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Hole {
            construction: cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Countersink {
                    diameter: Length(9.0),
                    angle: Angle(value),
                },
                ..
            },
            extent: Some(LinearTermination::ThroughAll),
            ..
        } if (*value - 82f64.to_radians()).abs() < 1.0e-12
    ));

    {
        let mut ir = decoded.ir_mut();
        let FeatureDefinition::Hole {
            construction,
            extent,
            ..
        } = &mut ir.model.features[0].definition
        else {
            panic!("counterbore hole");
        };
        let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
            panic!("ordinary hole form");
        };
        *kind = HoleKind::Counterbore {
            diameter: Length(12.0),
            depth: Length(5.0),
        };
        *extent = Some(LinearTermination::ThroughAll);
        let FeatureDefinition::Hole {
            construction,
            extent,
            ..
        } = &mut ir.model.features[1].definition
        else {
            panic!("countersink hole");
        };
        let cadmpeg_ir::features::HoleConstruction::Form { kind, .. } = construction else {
            panic!("ordinary hole form");
        };
        *kind = HoleKind::Countersink {
            diameter: Length(11.0),
            angle: Angle(90f64.to_radians()),
        };
        *extent = Some(LinearTermination::Blind {
            length: Length(25.0),
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
