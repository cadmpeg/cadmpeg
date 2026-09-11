// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.

#![allow(clippy::unwrap_used)]

const EPS_SCALAR_ROUND_TRIP: f64 = 1.0e-12;

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

const EPS_PARTIAL_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_REVERSED_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_SYMMETRIC_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_TWO_SIDED_REVOLUTION_FIRST_ANGLE: f64 = 1.0e-12;
const EPS_TWO_SIDED_REVOLUTION_SECOND_ANGLE: f64 = 1.0e-12;

#[test]
fn semantic_writer_round_trips_reference_coordinate_system() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><CoordinateSystem Name="Fixture" Type="ReferenceCoordinateSystem" id="28" Origin="1mm,2mm,3mm" XAxis="1,0,0" YAxis="0,1,0" ZAxis="0,0,1"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::DatumCoordinateSystem { frame } if matches!(frame.origin(), Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }) && matches!(frame.x_axis(), Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            }) && matches!(frame.y_axis(), Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }) && matches!(frame.z_axis(), Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            })
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::DatumCoordinateSystem { frame } = definition else {
                panic!("typed reference coordinate system");
            };
            *frame = cadmpeg_ir::features::FeatureCoordinateFrame::new(
                Point3::new(4.0, 5.0, 6.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap();
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
    assert_eq!(feature.xml_tag, "CoordinateSystem");
    assert_eq!(feature.kind, "ReferenceCoordinateSystem");
    assert_eq!(feature.properties["Origin"], "4mm,5mm,6mm");
    assert_eq!(feature.properties["XAxis"], "0,1,0");
    assert_eq!(feature.properties["YAxis"], "-1,0,0");
    assert_eq!(feature.properties["ZAxis"], "0,0,1");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::DatumCoordinateSystem { frame } if matches!(frame.origin(), Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            }) && matches!(frame.x_axis(), Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }) && matches!(frame.y_axis(), Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            }) && matches!(frame.z_axis(), Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            })
    ));
}

#[test]
fn semantic_writer_round_trips_equation_driven_curve() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><EquationDrivenCurve Name="Spiral" Type="EquationDrivenCurve" id="29" Parameter="t" XEquation="10*cos(t)" YEquation="10*sin(t)" ZEquation="t" Start="0" End="6.283185307179586" Closed="false"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::EquationCurve { curve }
            if curve.parameter() == "t"
            && curve.x_expression() == "10*cos(t)"
            && curve.y_expression() == "10*sin(t)"
            && curve.z_expression() == "t"
            && curve.start() == 0.0
            && (curve.end() - std::f64::consts::TAU).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::EquationCurve { curve } = definition else {
                panic!("typed equation curve");
            };
            *curve = cadmpeg_ir::features::FeatureEquationCurve::new(
                "u".into(),
                "u".into(),
                "u^2".into(),
                "u^3".into(),
                -2.0,
                3.0,
            )
            .unwrap();
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
    assert_eq!(feature.xml_tag, "EquationDrivenCurve");
    assert_eq!(feature.kind, "EquationDrivenCurve");
    assert_eq!(feature.properties["Parameter"], "u");
    assert_eq!(feature.properties["XEquation"], "u");
    assert_eq!(feature.properties["YEquation"], "u^2");
    assert_eq!(feature.properties["ZEquation"], "u^3");
    assert_eq!(feature.properties["Start"], "-2");
    assert_eq!(feature.properties["End"], "3");
    assert_eq!(feature.properties["Closed"], "false");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::EquationCurve { curve }
            if curve.start() == -2.0 && curve.end() == 3.0
            && curve.parameter() == "u"
            && curve.x_expression() == "u"
            && curve.y_expression() == "u^2"
            && curve.z_expression() == "u^3"
    ));
}

#[test]
fn semantic_writer_round_trips_helix() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{FeatureDefinition, HelixShape},
        scalar::NonZeroLength,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Helix Name="Coil" Type="HelixSpiral" id="30" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1" Clockwise="true" Taper="none"><Dimension Name="Radius">4mm</Dimension><Dimension Name="Pitch">-2mm</Dimension><Dimension Name="Revolutions">3.5</Dimension></Helix></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Helix {
            axis_origin: checked_geometry_1,
            axis_direction: checked_geometry_2,
            radius: actual_radius,
            shape: HelixShape::Cylindrical {
                pitch,
            },
            revolutions,
            clockwise: true,
            ..
        } if (revolutions.get() == 3.5 && (pitch.get() == -2.0) && actual_radius.get() == 4.0) && matches!(checked_geometry_1.get(), Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }) && matches!(checked_geometry_2.get(), Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            })
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Helix {
                axis_origin,
                axis_direction,
                radius,
                shape: HelixShape::Cylindrical { pitch },
                revolutions,
                clockwise,
                ..
            } = definition
            else {
                panic!("typed helix");
            };
            *axis_origin =
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap();
            *axis_direction =
                cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
            *radius = cadmpeg_ir::scalar::PositiveLength::new(7.0).unwrap();
            *pitch = NonZeroLength::new(8.0).unwrap();
            *revolutions = cadmpeg_ir::scalar::PositiveReal::new(9.25).unwrap();
            *clockwise = false;
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
    assert_eq!(feature.xml_tag, "Helix");
    assert_eq!(feature.kind, "HelixSpiral");
    assert_eq!(feature.properties["AxisOrigin"], "4mm,5mm,6mm");
    assert_eq!(feature.properties["AxisDirection"], "0,1,0");
    assert_eq!(feature.properties["Clockwise"], "false");
    assert_eq!(feature.properties["Taper"], "none");
    assert_eq!(feature.parameters["Radius"], "7mm");
    assert_eq!(feature.parameters["Pitch"], "8mm");
    assert_eq!(feature.parameters["Revolutions"], "9.25");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Helix {
            axis_origin: checked_geometry_1,
            axis_direction: checked_geometry_2,
            radius: actual_radius,
            shape: HelixShape::Cylindrical { pitch },
            revolutions,
            clockwise: false,
            ..
        } if (revolutions.get() == 9.25 && (pitch.get() == 8.0) && actual_radius.get() == 7.0) && matches!(checked_geometry_1.get(), Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            }) && matches!(checked_geometry_2.get(), Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            })
    ));
}

#[test]
fn semantic_writer_round_trips_slash_named_helix() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{FeatureDefinition, HelixShape},
        scalar::NonZeroLength,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Coil" Type="Helix/Spiral" id="30" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"><Dimension Name="Radius">4mm</Dimension><Dimension Name="Pitch">2mm</Dimension><Dimension Name="Revolutions">3.5</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moHelix_c", "Coil", 30)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Helix {
            radius: actual_radius,
            shape: HelixShape::Cylindrical { pitch },
            revolutions,
            ..
        } if revolutions.get() == 3.5 && (pitch.get() == 2.0) && actual_radius.get() == 4.0
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Helix {
                axis_origin,
                axis_direction,
                radius,
                shape: HelixShape::Cylindrical { pitch },
                revolutions,
                clockwise,
                ..
            } = definition
            else {
                panic!("typed helix");
            };
            *axis_origin =
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap();
            *axis_direction =
                cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
            *radius = cadmpeg_ir::scalar::PositiveLength::new(7.0).unwrap();
            *pitch = NonZeroLength::new(8.0).unwrap();
            *revolutions = cadmpeg_ir::scalar::PositiveReal::new(9.25).unwrap();
            *clockwise = true;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Helix/Spiral");
    assert_eq!(native.parameters["Radius"], "7mm");
    assert_eq!(native.parameters["Pitch"], "8mm");
    assert_eq!(native.parameters["Revolutions"], "9.25");
    assert_eq!(native.properties["AxisOrigin"], "4mm,5mm,6mm");
    assert_eq!(native.properties["AxisDirection"], "0,1,0");
    assert_eq!(native.properties["Clockwise"], "true");
}

#[test]
fn semantic_writer_round_trips_native_axis_helix() {
    use cadmpeg_ir::{
        features::FeatureDefinition,
        scalar::{Angle, Length},
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        r#"<Keywords><Feature Name="Helix/Spiral1" Type="Helix/Spiral" id="30"><Dimension Name="D3">3200</Dimension><Dimension Name="D4">12800</Dimension><Dimension Name="D5">0.25</Dimension><Dimension Name="D7">0°</Dimension></Feature></Keywords>"#
            .as_bytes(),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moHelix_c", "Helix/Spiral1", 30)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let feature = &decoded.ir().model.features[0];
    let native_ref = feature.native_ref.as_deref().unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::HelixNativeAxis {
            axis_native_ref,
            axial_rise: actual_axial_rise,
            pitch: actual_pitch,
            revolutions,
            start_angle: Angle::ZERO,
            clockwise: false,
        } if revolutions.get() == 0.25 && (axis_native_ref == native_ref) && actual_axial_rise.get() == 3200.0 && actual_pitch.get() == 12800.0
    ));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
    let findings = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::HelixNativeAxis {
                axial_rise,
                pitch,
                revolutions,
                start_angle,
                clockwise,
                ..
            } = definition
            else {
                panic!("typed native-axis helix");
            };
            *axial_rise = Length::new(4000.0).unwrap();
            *pitch = Length::new(16000.0).unwrap();
            *revolutions = cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap();
            *start_angle = Angle::new(std::f64::consts::FRAC_PI_2).unwrap();
            *clockwise = true;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Helix/Spiral");
    assert_eq!(native.parameters["D3"], "4000");
    assert_eq!(native.parameters["D4"], "16000");
    assert_eq!(native.parameters["D5"], "0.5");
    assert_eq!(native.parameters["D7"], "90°");
    assert_eq!(native.properties["Clockwise"], "true");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::HelixNativeAxis {
            axial_rise: actual_axial_rise,
            pitch: actual_pitch,
            revolutions,
            start_angle: value,
            clockwise: true,
            ..
        } if revolutions.get() == 0.5 && ((value.get() - std::f64::consts::FRAC_PI_2).abs() < EPS_SCALAR_ROUND_TRIP) && actual_axial_rise.get() == 4000.0 && actual_pitch.get() == 16000.0
    ));
}

#[test]
fn semantic_writer_rejects_embedded_helix_geometry_edits() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        r#"<Keywords><Feature Name="Helix/Spiral1" Type="Helix/Spiral" id="30"><Dimension Name="D3">3200</Dimension><Dimension Name="D4">12800</Dimension><Dimension Name="D5">0.25</Dimension><Dimension Name="D7">0°</Dimension></Feature></Keywords>"#
            .as_bytes(),
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moHelix_c", "Helix/Spiral1", 30)]),
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    {
        let mut ir_edit = decoded.ir_mut();
        update_sldprt_native(&mut ir_edit, |native| {
            let description = b"boundary_polyline mesh";
            let schema = b"SCH_3201255_32001_13006";
            let mut stream = b"PS\0\0".to_vec();
            stream.extend((description.len() as u16).to_be_bytes());
            stream.extend(description);
            stream.push(schema.len() as u8);
            stream.extend(schema);
            stream.extend([0xff, 0xff, 0xff, 0xff, 0x00, 0x22]);
            stream.extend((65u32 * 3).to_be_bytes());
            stream.extend([0x00, 0x22]);
            for index in 0..=64 {
                let t = f64::from(index) / 64.0;
                let angle = std::f64::consts::FRAC_PI_2 * t;
                for value in [
                    10.0 + 3.5 * angle.cos(),
                    20.0 - 3.2 * t,
                    30.0 + 3.5 * angle.sin(),
                ] {
                    stream.extend(value.to_be_bytes());
                }
            }
            native.feature_input_lanes[0].native_payload.extend(stream);
        });
        let native = sldprt_native(&ir_edit);
        crate::resolved_features::holes::project_helix_axes(
            &mut ir_edit.model.features,
            &native.feature_histories,
            &native.feature_input_lanes,
        )
        .unwrap();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Helix { radius, .. } = definition else {
                panic!("embedded helix geometry");
            };
            *radius = cadmpeg_ir::scalar::PositiveLength::new(9.0).unwrap();
        });
    }

    let error = crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("changes embedded helix geometry"),
        "{error}"
    );
}

#[test]
fn semantic_writer_round_trips_wrap() {
    use cadmpeg_ir::{
        features::{FaceSelection, FeatureDefinition, ProfileRef, WrapMode},
        scalar::Length,
    };

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><Wrap Name="Mark" Type="Wrap" id="31" Profile="{face}" Face="{face}" Mode="Emboss" Method="Spline"><Dimension Name="Depth">2mm</Dimension></Wrap></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(), FeatureDefinition::Wrap {
            profile,
            face: FaceSelection::Resolved { faces: targets, native },
            mode: WrapMode::Emboss { depth: actual_depth },
        } if matches!((profile.as_ref(),), (ProfileRef::Faces(faces),) if (faces == std::slice::from_ref(&face_id) && targets == std::slice::from_ref(&face_id) && native == &face) && actual_depth.get() == 2.0)));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Wrap {
                profile,
                face,
                mode,
            } = definition
            else {
                panic!("typed wrap");
            };
            *profile = (ProfileRef::Faces(vec![face_id.clone()]))
                .try_into()
                .unwrap();
            *face = FaceSelection::Faces(vec![face_id.clone()]);
            *mode = WrapMode::Deboss {
                depth: Length::new(3.5).unwrap(),
            };
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Profile"], face_id.as_str());
    assert_eq!(native.properties["Face"], face_id.as_str());
    assert_eq!(native.properties["Mode"], "Deboss");
    assert_eq!(native.properties["Method"], "Spline");
    assert_eq!(native.parameters["Depth"], "3.5mm");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Wrap {
            mode: WrapMode::Deboss { depth: actual_depth },
            ..
        } if actual_depth.get() == 3.5
    ));

    let scribed = regenerated;
    let mut scribed = cadmpeg_test_support::EditableDecodeResult::from(scribed);
    {
        let mut ir_edit = scribed.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::Wrap { mode, .. } = definition else {
                panic!("typed wrap");
            };
            *mode = WrapMode::Scribe;
        });
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        scribed.ir(),
        scribed.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let scribed = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(scribed.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Mode"], "Scribe");
    assert!(!native.parameters.contains_key("Depth"));
    assert!(matches!(
        scribed.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Wrap {
            mode: WrapMode::Scribe,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_move_copy_body() {
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::{
        features::{AxisAngle, BodySelection, FeatureDefinition},
        scalar::Angle,
    };

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = base.ir().model.bodies[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><MoveBody Name="Copy" Type="MoveCopyBody" id="32" Bodies="{body}" Translation="1mm,2mm,3mm" RotationOrigin="4mm,5mm,6mm" RotationAxis="0,0,1" Copies="2" Frame="model"><Dimension Name="Rotation">90deg</Dimension></MoveBody></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let body_id = decoded.ir().model.bodies[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Resolved { bodies, native },
            translation: geometry_1,
            rotation: Some(AxisAngle {
                origin: geometry_2,
                direction: geometry_3,
                angle,
            }),
            copies: 2,
        } if ( bodies == std::slice::from_ref(&body_id) && native == &body
            && (angle.get() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12) && matches!(geometry_1.get(), Vector3 { x: 1.0, y: 2.0, z: 3.0 }) && matches!(geometry_2.get(), Point3 { x: 4.0, y: 5.0, z: 6.0 }) && matches!(geometry_3.get(), Vector3 { x: 0.0, y: 0.0, z: 1.0 })
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::MoveBody {
                bodies,
                translation,
                rotation,
                copies,
            } = definition
            else {
                panic!("typed body motion");
            };
            *bodies = BodySelection::Bodies(vec![body_id.clone()]);
            *translation =
                cadmpeg_ir::features::FiniteVector3::new(Vector3::new(-7.0, 8.0, 9.0)).unwrap();
            *rotation = Some(AxisAngle {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(10.0, 11.0, 12.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    0.0, 1.0, 0.0,
                ))
                .unwrap(),
                angle: Angle::new(0.25).unwrap(),
            });
            *copies = 3;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Bodies"], body_id.as_str());
    assert_eq!(native.properties["Translation"], "-7mm,8mm,9mm");
    assert_eq!(native.properties["RotationOrigin"], "10mm,11mm,12mm");
    assert_eq!(native.properties["RotationAxis"], "0,1,0");
    assert_eq!(native.properties["Copies"], "3");
    assert_eq!(native.properties["Frame"], "model");
    assert_eq!(native.parameters["Rotation"], "0.25rad");

    let translated = regenerated;
    let mut translated = cadmpeg_test_support::EditableDecodeResult::from(translated);
    {
        let mut ir_edit = translated.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::MoveBody {
                rotation, copies, ..
            } = definition
            else {
                panic!("typed body motion");
            };
            *rotation = None;
            *copies = 0;
        });
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        translated.ir(),
        translated.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let translated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(translated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Copies"], "0");
    assert!(!native.properties.contains_key("RotationOrigin"));
    assert!(!native.properties.contains_key("RotationAxis"));
    assert!(!native.parameters.contains_key("Rotation"));
    assert!(matches!(
        translated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::MoveBody {
            rotation: None,
            copies: 0,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_offset_surface() {
    use cadmpeg_ir::{
        features::{FaceSelection, FeatureDefinition},
        scalar::Length,
    };

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><OffsetSurface Name="Offset" Type="OffsetSurface" id="33" Faces="{face}" Knit="true"><Dimension Name="Distance">2mm</Dimension></OffsetSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(actual_distance),
        } if (faces == std::slice::from_ref(&face_id) && native == &face) && actual_distance.get() == 2.0
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::OffsetSurface { faces, distance } = definition else {
                panic!("typed offset surface");
            };
            *faces = FaceSelection::Faces(vec![face_id.clone()]);
            *distance = Some(Length::new(-3.5).unwrap());
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.as_str());
    assert_eq!(native.properties["Knit"], "true");
    assert_eq!(native.parameters["Distance"], "-3.5mm");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::OffsetSurface {
            distance: Some(actual_distance),
            ..
        } if actual_distance.get() == -3.5
    ));
}

#[test]
fn semantic_writer_round_trips_knit_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><KnitSurface Name="Knit" Type="Knit" id="34" Faces="{face}" MergeEntities="false" CreateSolid="false" CheckGeometry="true"><Dimension Name="GapTolerance">0.01mm</Dimension></KnitSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Resolved { faces, native },
            merge_entities: Some(false),
            create_solid: Some(false),
            gap_tolerance: Some(actual_gap_tolerance),
        } if (faces == std::slice::from_ref(&face_id) && native == &face) && actual_gap_tolerance.get() == 0.01
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::KnitSurface {
                faces,
                merge_entities,
                create_solid,
                gap_tolerance,
            } = definition
            else {
                panic!("typed knit surface");
            };
            *faces = FaceSelection::Faces(vec![face_id.clone()]);
            *merge_entities = Some(true);
            *create_solid = Some(true);
            *gap_tolerance = None;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.as_str());
    assert_eq!(native.properties["MergeEntities"], "true");
    assert_eq!(native.properties["CreateSolid"], "true");
    assert_eq!(native.properties["CheckGeometry"], "true");
    assert!(!native.parameters.contains_key("GapTolerance"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::KnitSurface {
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: None,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_cut_with_surface() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = base.ir().model.bodies[0].id.as_str().to_owned();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><CutWithSurface Name="Cut" Type="SurfaceCut" id="35" Targets="{body}" Tools="{face}" Reverse="false" ConsumeTool="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let body_id = decoded.ir().model.bodies[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::CutWithSurface {
            targets: BodySelection::Resolved { bodies, native: body_native },
            tools: FaceSelection::Resolved { faces, native: face_native },
            reverse: Some(false),
        } if bodies == std::slice::from_ref(&body_id) && body_native == &body
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::CutWithSurface {
                targets,
                tools,
                reverse,
            } = definition
            else {
                panic!("typed surface cut");
            };
            *targets = BodySelection::Bodies(vec![body_id.clone()]);
            *tools = FaceSelection::Faces(vec![face_id.clone()]);
            *reverse = Some(true);
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Targets"], body_id.as_str());
    assert_eq!(native.properties["Tools"], face_id.as_str());
    assert_eq!(native.properties["Reverse"], "true");
    assert_eq!(native.properties["ConsumeTool"], "false");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::CutWithSurface {
            reverse: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_preserves_missing_cut_with_surface_side_flag() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = base.ir().model.bodies[0].id.as_str().to_owned();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><CutWithSurface Name="Cut" Type="SurfaceCut" id="35" Targets="{body}" Tools="{face}" ConsumeTool="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::CutWithSurface {
            targets: BodySelection::Resolved { .. },
            tools: FaceSelection::Resolved { .. },
            reverse: None,
        }
    ));

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
    assert!(!native.properties.contains_key("Reverse"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::CutWithSurface { reverse: None, .. }
    ));
}

#[test]
fn semantic_writer_round_trips_filled_surface() {
    filled_surface_round_trip(cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
        cadmpeg_ir::features::SurfaceContinuity::Curvature,
    ));
}

#[test]
fn semantic_writer_accepts_all_equal_per_boundary_continuity() {
    for conditions in [
        vec![cadmpeg_ir::features::SurfaceContinuity::Curvature],
        vec![cadmpeg_ir::features::SurfaceContinuity::Curvature; 2],
    ] {
        filled_surface_round_trip(
            cadmpeg_ir::features::FilledSurfaceContinuityState::per_boundary(conditions),
        );
    }
}

fn filled_surface_round_trip(
    edited_continuity: cadmpeg_ir::features::FilledSurfaceContinuityState,
) {
    use cadmpeg_ir::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, SurfaceContinuity,
    };

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir().model.edges[0].id.as_str().to_owned();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><FilledSurface Name="Fill" Type="FillSurface" id="36" Boundary="{edge}" SupportFaces="{face}" Continuity="Tangent" MergeResult="false" Optimize="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Resolved { edges, native: edge_native }),
            support_faces: FaceSelection::Resolved { faces, native: face_native },
            continuity,
            merge_result: Some(false),
        } if continuity.uniform_value() == Some(SurfaceContinuity::Tangent)
            && edges == std::slice::from_ref(&edge_id) && edge_native == &edge
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::FilledSurface {
                boundary,
                support_faces,
                continuity,
                merge_result,
            } = definition
            else {
                panic!("typed filled surface");
            };
            *boundary = cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(vec![
                edge_id.clone(),
            ]));
            *support_faces = FaceSelection::Faces(vec![face_id.clone()]);
            *continuity = edited_continuity;
            *merge_result = Some(true);
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Boundary"], edge_id.as_str());
    assert_eq!(native.properties["SupportFaces"], face_id.as_str());
    assert_eq!(native.properties["Continuity"], "Curvature");
    assert_eq!(native.properties["MergeResult"], "true");
    assert_eq!(native.properties["Optimize"], "true");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::FilledSurface {
            ref continuity,
            merge_result: Some(true),
            ..
        } if continuity.uniform_value() == Some(SurfaceContinuity::Curvature)
    ));
}

#[test]
fn semantic_writer_round_trips_trim_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, PathRef, TrimRegion};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir().model.edges[0].id.as_str().to_owned();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><TrimSurface Name="Trim" Type="SurfaceTrim" id="37" Faces="{face}" Tool="{edge}" Keep="Inside" Split="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Resolved { faces, native },
            tool: PathRef::Edges(edges),
            keep: TrimRegion::Inside,
            ..
        } if faces == std::slice::from_ref(&face_id) && native == &face && edges == std::slice::from_ref(&edge_id)
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::TrimSurface {
                faces, tool, keep, ..
            } = definition
            else {
                panic!("typed trim surface");
            };
            *faces = FaceSelection::Faces(vec![face_id.clone()]);
            *tool = PathRef::Edges(vec![edge_id.clone()]);
            *keep = TrimRegion::Outside;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.as_str());
    assert_eq!(native.properties["Tool"], edge_id.as_str());
    assert_eq!(native.properties["Keep"], "Outside");
    assert_eq!(native.properties["Split"], "false");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::TrimSurface {
            keep: TrimRegion::Outside,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_extend_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, SurfaceExtension};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><ExtendSurface Name="Extend" Type="SurfaceExtend" id="38" Faces="{face}" Method="Natural" CornerMode="Merge"><Dimension Name="Distance">2mm</Dimension></ExtendSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(actual_distance),
            method: SurfaceExtension::Natural,
        } if (faces == std::slice::from_ref(&face_id) && native == &face) && actual_distance.get() == 2.0
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::ExtendSurface {
                faces,
                distance,
                method,
            } = definition
            else {
                panic!("typed extended surface");
            };
            *faces = FaceSelection::Faces(vec![face_id.clone()]);
            *distance = Some(cadmpeg_ir::scalar::PositiveLength::new(4.5).unwrap());
            *method = SurfaceExtension::Linear;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.as_str());
    assert_eq!(native.properties["Method"], "Linear");
    assert_eq!(native.properties["CornerMode"], "Merge");
    assert_eq!(native.parameters["Distance"], "4.5mm");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::ExtendSurface {
            distance: Some(actual_distance),
            method: SurfaceExtension::Linear,
            ..
        } if actual_distance.get() == 4.5
    ));
}

#[test]
fn semantic_writer_round_trips_all_ruled_surface_modes() {
    use cadmpeg_ir::features::{EdgeSelection, FaceSelection, FeatureDefinition, RuledSurfaceMode};
    use cadmpeg_ir::math::Vector3;

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir().model.edges[0].id.as_str().to_owned();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><RuledSurface Name="Ruled" Type="SurfaceRuled" id="39" Edges="{edge}" SupportFaces="{face}" Mode="Direction" Direction="0,0,1" Trim="true"><Dimension Name="Distance">2mm</Dimension></RuledSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::RuledSurface {
            edges: EdgeSelection::Resolved { edges, native: edge_native },
            support_faces: FaceSelection::Resolved { faces, native: face_native },
            mode: RuledSurfaceMode::Direction {
                direction: geometry_1,
                distance: actual_distance,
            },
            ..
        } if ( (edges == std::slice::from_ref(&edge_id) && edge_native == &edge
            && faces == std::slice::from_ref(&face_id) && face_native == &face) && actual_distance.get() == 2.0) && matches!(geometry_1.get(), Vector3 { x: 0.0, y: 0.0, z: 1.0 })
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::RuledSurface {
                edges,
                support_faces,
                mode,
                ..
            } = definition
            else {
                panic!("typed ruled surface");
            };
            *edges = EdgeSelection::Edges(vec![edge_id.clone()]);
            *support_faces = FaceSelection::Faces(vec![face_id.clone()]);
            *mode = RuledSurfaceMode::Normal {
                distance: cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap(),
            };
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
    let mut regenerated = cadmpeg_test_support::EditableDecodeResult::from(regenerated);
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Mode"], "Normal");
    assert!(!native.properties.contains_key("Direction"));
    assert_eq!(native.properties["Trim"], "true");
    assert_eq!(native.parameters["Distance"], "3mm");

    {
        let mut ir_edit = regenerated.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::RuledSurface { mode, .. } = definition else {
                panic!("typed ruled surface");
            };
            *mode = RuledSurfaceMode::Tangent {
                distance: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
            };
        });
    }
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(
        regenerated.ir(),
        regenerated.source_fidelity(),
        &mut encoded,
    )
    .unwrap();
    let tangent = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        tangent.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::RuledSurface {
            mode: RuledSurfaceMode::Tangent {
                distance: actual_distance
            },
            ..
        } if actual_distance.get() == 4.0
    ));
}

#[test]
fn semantic_writer_round_trips_projected_curve() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, PathRef};
    use cadmpeg_ir::math::Vector3;

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir().model.edges[0].id.as_str().to_owned();
    let face = base.ir().model.faces[0].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><ProjectedCurve Name="Projection" Type="ProjectionCurve" id="40" Source="{edge}" TargetFaces="{face}" Direction="0,0,1" Bidirectional="false" Simplify="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::ProjectedCurve {
            source: PathRef::Edges(edges),
            target_faces: FaceSelection::Resolved { faces, native },
            direction: cadmpeg_ir::features::CurveProjectionDirection::Vector(checked_geometry_1),
            bidirectional: Some(false),
        } if (edges == std::slice::from_ref(&edge_id) && faces == std::slice::from_ref(&face_id) && native == &face) && matches!(checked_geometry_1.get(), Vector3 { x: 0.0, y: 0.0, z: 1.0 })
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } = definition
            else {
                panic!("typed projected curve");
            };
            *source = PathRef::Edges(vec![edge_id.clone()]);
            *target_faces = FaceSelection::Faces(vec![face_id.clone()]);
            *direction = cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
            );
            *bidirectional = Some(true);
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Source"], edge_id.as_str());
    assert_eq!(native.properties["TargetFaces"], face_id.as_str());
    assert_eq!(native.properties["Bidirectional"], "true");
    assert_eq!(native.properties["Simplify"], "true");
    assert!(!native.properties.contains_key("Direction"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::ProjectedCurve {
            direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal
            ),
            bidirectional: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_ordered_composite_curve() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let first = base.ir().model.edges[0].id.as_str().to_owned();
    let second = base.ir().model.edges[1].id.as_str().to_owned();
    let xml = format!(
        r#"<Keywords><CompositeCurve Name="Chain" Type="CompositeCurve" id="41" Segments="{first};{second}" Closed="false" Simplify="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let first_id = decoded.ir().model.edges[0].id.clone();
    let second_id = decoded.ir().model.edges[1].id.clone();
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::CompositeCurve { segments, closed: false }
            if segments.as_slice() == [
                PathRef::Edges(vec![first_id.clone()]),
                PathRef::Edges(vec![second_id.clone()]),
            ]
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        ir_edit.model.features[0].evaluation.edit(|definition, _| {
            let FeatureDefinition::CompositeCurve { segments, closed } = definition else {
                panic!("typed composite curve");
            };
            *segments = vec![
                PathRef::Edges(vec![second_id.clone()]),
                PathRef::Edges(vec![first_id.clone()]),
            ]
            .try_into()
            .unwrap();
            *closed = true;
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
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(
        native.properties["Segments"],
        format!("{};{}", second_id.as_str(), first_id.as_str())
    );
    assert_eq!(native.properties["Closed"], "true");
    assert_eq!(native.properties["Simplify"], "true");
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::CompositeCurve { segments, closed: true }
            if segments.as_slice() == [
                PathRef::Edges(vec![second_id]),
                PathRef::Edges(vec![first_id]),
            ]
    ));
}

#[test]
fn semantic_writer_round_trips_typed_revolution() {
    use cadmpeg_ir::features::{AngularTermination, BooleanOp, FeatureDefinition, RevolveExtent};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Turn" Type="Revolve" id="17" AxisOrigin="10mm,20mm,30mm" AxisDirection="0,1,0" Operation="Join"><Dimension Name="Angle">180deg</Dimension></Revolve></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Revolve {
            construction,
            op: BooleanOp::Join,
        } if construction.profile().is_none()
            && construction.axis().is_some_and(|axis| matches!(axis,
                cadmpeg_ir::features::RevolutionAxis {
                    origin: geometry_1,
                    direction: geometry_2,
                    ..
                } if matches!(geometry_1.get(), Point3 { x: 10.0, y: 20.0, z: 30.0 }) && matches!(geometry_2.get(), Vector3 { x: 0.0, y: 1.0, z: 0.0 })))
            && matches!(construction.extent(), Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value },
                }) if (value.get() - std::f64::consts::PI).abs() < EPS_PARTIAL_REVOLUTION_ANGLE)
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[0].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Revolve { construction, op } = &mut updated_ir_edit_definition
        else {
            panic!("typed revolution feature");
        };
        let Some(axis) = construction.axis_mut() else {
            panic!("resolved revolution axis");
        };
        axis.origin = cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap();
        axis.direction =
            cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap();
        construction.set_extent(Some(RevolveExtent::OneSided {
            termination: AngularTermination::Angle {
                angle: cadmpeg_ir::scalar::PositiveAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            },
        }));
        *op = BooleanOp::Cut;
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
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
    assert_eq!(feature.properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(feature.properties["AxisDirection"], "0,0,1");
    assert_eq!(feature.properties["Operation"], "Cut");
    assert_eq!(
        feature.parameters["Angle"],
        format!("{}rad", std::f64::consts::FRAC_PI_2)
    );
}

#[test]
fn semantic_writer_retains_partial_native_revolution_construction() {
    use cadmpeg_ir::features::{BooleanOp, FeatureDefinition};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Unknown turn" Type="Revolve" id="17" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    assert!(matches!(
        decoded.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Revolve {
            construction,
            op: BooleanOp::Unresolved,
        } if construction.profile().is_none()
            && construction.axis().is_some_and(|axis| matches!(axis,
                cadmpeg_ir::features::RevolutionAxis {
                    origin: geometry_1,
                    direction: geometry_2,
                    ..
                } if matches!(geometry_1.get(), Point3 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0
                    }) && matches!(geometry_2.get(), Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0
                    })))
            && construction.extent().is_none()
    ));
    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    let error = crate::test_support::plan_inherited_write(
        &detached,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("unresolved revolution construction"));
    decoded.ir_mut().model.features[0].name = Some("Renamed turn".into());

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
    assert_eq!(native.name, "Renamed turn");
    assert_eq!(native.properties["AxisOrigin"], "1mm,2mm,3mm");
    assert_eq!(native.properties["AxisDirection"], "0,0,1");
    assert!(!native.properties.contains_key("Profile"));
    assert!(!native.properties.contains_key("Operation"));
    assert!(!native.parameters.contains_key("Angle"));
    assert!(matches!(
        regenerated.ir().model.features[0].evaluation.definition(),
        FeatureDefinition::Revolve {
            ref construction,
            op: BooleanOp::Unresolved,
        } if construction.axis().is_some()
            && construction.profile().is_none()
            && construction.extent().is_none()
    ));
}

#[test]
fn semantic_writer_round_trips_all_revolution_extents() {
    use cadmpeg_ir::features::{
        AngularTermination, BooleanOp, FeatureDefinition, ProfileRef, RevolveExtent,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="TurnProfile" Type="Sketch" id="40"/><Revolve Name="One" Type="Revolve" id="41" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" EndCondition="OneSided" Operation="Join"><Dimension Name="Angle">90deg</Dimension></Revolve><Revolve Name="Sym" Type="Revolve" id="42" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,1,0" EndCondition="Symmetric" Operation="NewBody"><Dimension Name="Angle">180deg</Dimension></Revolve><Revolve Name="Two" Type="Revolve" id="43" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="1,0,0" EndCondition="TwoSided" Operation="Cut"><Dimension Name="Angle">30deg</Dimension><Dimension Name="Angle2">60deg</Dimension></Revolve></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut decoded = cadmpeg_test_support::EditableDecodeResult::from(decoded);
    let profile_feature = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        decoded.ir().model.features[1].evaluation.definition(),
        FeatureDefinition::Revolve {
            construction,
            op: BooleanOp::Join,
        } if matches!(construction.profile().map(AsRef::as_ref), Some(ProfileRef::Feature(profile)) if profile == &profile_feature)
            && matches!(construction.extent(), Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value },
                }) if (value.get() - 90f64.to_radians()).abs() < EPS_REVERSED_REVOLUTION_ANGLE)
    ));
    assert!(matches!(
        decoded.ir().model.features[2].evaluation.definition(),
        FeatureDefinition::Revolve {
            ref construction,
            op: BooleanOp::NewBody,
        } if matches!(construction.extent(), Some(RevolveExtent::Symmetric {
                    termination: AngularTermination::Angle { angle: value },
                }) if (value.get() - std::f64::consts::PI).abs() < EPS_SYMMETRIC_REVOLUTION_ANGLE)
    ));
    assert!(matches!(
        decoded.ir().model.features[3].evaluation.definition(),
        FeatureDefinition::Revolve {
            ref construction,
            op: BooleanOp::Cut,
        } if matches!(construction.extent(), Some(RevolveExtent::TwoSided {
                    first: AngularTermination::Angle { angle: first },
                    second: AngularTermination::Angle { angle: second },
                }) if (first.get() - 30f64.to_radians()).abs()
                    < EPS_TWO_SIDED_REVOLUTION_FIRST_ANGLE
                    && (second.get() - 60f64.to_radians()).abs()
                        < EPS_TWO_SIDED_REVOLUTION_SECOND_ANGLE)
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let updated_ir_edit_evaluation = &mut ir_edit.model.features[3].evaluation;
        let mut updated_ir_edit_definition = updated_ir_edit_evaluation.definition().clone();
        let FeatureDefinition::Revolve { construction, op } = &mut updated_ir_edit_definition
        else {
            panic!("typed revolution");
        };
        construction.set_extent(Some(RevolveExtent::OneSided {
            termination: AngularTermination::Angle {
                angle: cadmpeg_ir::scalar::PositiveAngle::new(0.75).unwrap(),
            },
        }));
        *op = BooleanOp::Intersect;
        updated_ir_edit_evaluation.set_definition(updated_ir_edit_definition);
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
    assert_eq!(native[3].properties["EndCondition"], "OneSided");
    assert_eq!(native[3].properties["Operation"], "Intersect");
    assert_eq!(native[3].properties["Profile"], "40");
    assert_eq!(native[3].parameters["Angle"], "0.75rad");
    assert!(!native[3].parameters.contains_key("Angle2"));
}
