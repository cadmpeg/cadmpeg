// SPDX-License-Identifier: Apache-2.0
//! Nonfinite and invalid feature-dimension decode degradation tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_degrades_nonfinite_feature_dimensions() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Extrude" Type="BossExtrude" id="1"><Dimension Name="Depth">NaNmm</Dimension></Extrusion>
            <Fillet Name="Fillet" Type="Fillet" id="2"><Dimension Name="Radius">infmm</Dimension></Fillet>
            <Shell Name="Shell" Type="Shell" id="3" Outward="false"><Dimension Name="Thickness">NaNmm</Dimension></Shell>
            <Dome Name="Dome" Type="Dome" id="4" Faces="face:1" Elliptical="false" Reverse="false"><Dimension Name="Height">infmm</Dimension></Dome>
            <Revolve Name="Revolve" Type="Revolve" id="5" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">NaNrad</Dimension></Revolve>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features.len(), 5);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                },
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            thickness: None,
            outward: Some(false),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[3].definition,
        FeatureDefinition::Dome {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            height: None,
            elliptical: Some(false),
            reverse: Some(false),
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(_),
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
}

#[test]
fn decode_degrades_nonpositive_feature_dimensions() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Extrusion Name="Extrude" Type="BossExtrude" id="1"><Dimension Name="Depth">0mm</Dimension></Extrusion>
            <Fillet Name="Fillet" Type="Fillet" id="2"><Dimension Name="Radius">-1mm</Dimension></Fillet>
            <Shell Name="Shell" Type="Shell" id="3" Outward="false"><Dimension Name="Thickness">0mm</Dimension></Shell>
            <Dome Name="Dome" Type="Dome" id="4" Faces="face:1" Elliptical="false" Reverse="false"><Dimension Name="Height">-2mm</Dimension></Dome>
            <Hole Name="Hole" Type="Hole" id="5"><Dimension Name="Diameter">0mm</Dimension><Dimension Name="Depth">5mm</Dimension></Hole>
            <Chamfer Name="Chamfer" Type="Chamfer" id="6"><Dimension Name="Distance">-3mm</Dimension></Chamfer>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features.len(), 6);
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::Termination::Unresolved,
                    ..
                },
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Fillet {
            ref groups,
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::FilletGroup {
            radius: cadmpeg_ir::features::RadiusSpec::Unresolved {
                form: Some(cadmpeg_ir::features::RadiusForm::Constant),
            },
            ..
        }])
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            thickness: None,
            outward: Some(false),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[3].definition,
        FeatureDefinition::Dome {
            faces: cadmpeg_ir::features::FaceSelection::Native(_),
            height: None,
            elliptical: Some(false),
            reverse: Some(false),
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Hole {
            kind: cadmpeg_ir::features::HoleKind::Simple,
            diameter: None,
            extent: Some(cadmpeg_ir::features::Termination::Blind {
                length: cadmpeg_ir::features::Length(5.0),
            }),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[5].definition,
        FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::Distance),
            },
            ..
        }])
    ));
}

#[test]
fn decode_retains_invalid_feature_directions_and_angles_as_native() {
    use cadmpeg_ir::features::{FeatureDefinition, PatternForm, PatternKind};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="NativeSeed" id="1"/>
            <Pattern Name="Pattern" Type="LinearPattern" id="2" Seeds="1" Direction="0,0,0"><Dimension Name="Spacing">2mm</Dimension><Dimension Name="Count">2</Dimension></Pattern>
            <MoveFace Name="Move" Type="MoveFace" id="3" Faces="face:1" Mode="Translate" Direction="0,0,0"><Dimension Name="Distance">2mm</Dimension></MoveFace>
            <Chamfer Name="Chamfer" Type="Chamfer" id="4"><Dimension Name="Distance">2mm</Dimension><Dimension Name="Angle">180deg</Dimension></Chamfer>
            <Revolve Name="Revolve" Type="Revolve" id="5" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" Operation="Join"><Dimension Name="Angle">-1deg</Dimension></Revolve>
            <Sweep Name="Sweep" Type="Sweep" id="6" Profile="1" Path="1" Operation="Join"><Dimension Name="Scale">inf</Dimension></Sweep>
            <Rib Name="Rib" Type="Rib" id="7" Profile="1" Direction="0,0,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib>
        </Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features.len(), 7);
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Pattern {
            pattern: PatternKind::Unresolved {
                form: Some(PatternForm::Linear),
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[4].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(_),
                extent: None,
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[6].definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(_),
                direction: None,
                thickness: Some(cadmpeg_ir::features::Length(2.0)),
                side: Some(cadmpeg_ir::features::RibSide::OneSided),
                draft: cadmpeg_ir::features::RibDraft::None,
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Chamfer {
            ref groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            spec: cadmpeg_ir::features::ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::ChamferForm::DistanceAngle),
            },
            ..
        }])
    ));
    for index in [2, 5] {
        assert!(matches!(
            decoded.ir().model.features[index].definition,
            FeatureDefinition::Native { .. }
        ));
    }
}
