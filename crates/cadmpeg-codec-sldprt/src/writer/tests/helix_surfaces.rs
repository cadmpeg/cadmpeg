// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            x_axis: Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
            y_axis: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            z_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::DatumCoordinateSystem {
            origin,
            x_axis,
            y_axis,
            z_axis,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed reference coordinate system");
        };
        *origin = Point3::new(4.0, 5.0, 6.0);
        *x_axis = Vector3::new(0.0, 1.0, 0.0);
        *y_axis = Vector3::new(-1.0, 0.0, 0.0);
        *z_axis = Vector3::new(0.0, 0.0, 1.0);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::DatumCoordinateSystem {
            origin: Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            },
            x_axis: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            y_axis: Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            z_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        } if parameter == "t"
            && x_expression == "10*cos(t)"
            && y_expression == "10*sin(t)"
            && z_expression == "t"
            && *start == 0.0
            && (*end - std::f64::consts::TAU).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start,
            end,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed equation curve");
        };
        *parameter = "u".into();
        *x_expression = "u".into();
        *y_expression = "u^2".into();
        *z_expression = "u^3".into();
        *start = -2.0;
        *end = 3.0;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::EquationCurve {
            parameter,
            x_expression,
            y_expression,
            z_expression,
            start: -2.0,
            end: 3.0,
        } if parameter == "u"
            && x_expression == "u"
            && y_expression == "u^2"
            && z_expression == "u^3"
    ));
}

#[test]
fn semantic_writer_round_trips_helix() {
    use cadmpeg_ir::features::{FeatureDefinition, Length};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Helix Name="Coil" Type="HelixSpiral" id="30" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1" Clockwise="true" Taper="none"><Dimension Name="Radius">4mm</Dimension><Dimension Name="Pitch">-2mm</Dimension><Dimension Name="Revolutions">3.5</Dimension></Helix></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Helix {
            axis_origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            axis_direction: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            radius: Length(4.0),
            pitch: Length(-2.0),
            revolutions: 3.5,
            clockwise: true,
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Helix {
            axis_origin,
            axis_direction,
            radius,
            pitch,
            revolutions,
            clockwise,
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed helix");
        };
        *axis_origin = Point3::new(4.0, 5.0, 6.0);
        *axis_direction = Vector3::new(0.0, 1.0, 0.0);
        *radius = Length(7.0);
        *pitch = Length(8.0);
        *revolutions = 9.25;
        *clockwise = false;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Helix {
            axis_origin: Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            },
            axis_direction: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            radius: Length(7.0),
            pitch: Length(8.0),
            revolutions: 9.25,
            clockwise: false,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_slash_named_helix() {
    use cadmpeg_ir::features::{FeatureDefinition, Length};
    use cadmpeg_ir::math::{Point3, Vector3};

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Helix {
            radius: Length(4.0),
            pitch: Length(2.0),
            revolutions: 3.5,
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Helix {
            axis_origin,
            axis_direction,
            radius,
            pitch,
            revolutions,
            clockwise,
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed helix");
        };
        *axis_origin = Point3::new(4.0, 5.0, 6.0);
        *axis_direction = Vector3::new(0.0, 1.0, 0.0);
        *radius = Length(7.0);
        *pitch = Length(8.0);
        *revolutions = 9.25;
        *clockwise = true;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
    use cadmpeg_ir::features::{Angle, FeatureDefinition, Length};

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = &decoded.ir().model.features[0];
    let native_ref = feature.native_ref.as_deref().unwrap();
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::HelixNativeAxis {
            axis_native_ref,
            axial_rise: Length(3200.0),
            pitch: Length(12800.0),
            revolutions: 0.25,
            start_angle: Angle(0.0),
            clockwise: false,
        } if axis_native_ref == native_ref
    ));
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.message
            == "1 typed feature(s) retain native or unresolved required operation operands."
    }));
    let findings = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).findings;
    assert!(findings.is_empty(), "{findings:#?}");

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::HelixNativeAxis {
            axial_rise,
            pitch,
            revolutions,
            start_angle,
            clockwise,
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed native-axis helix");
        };
        *axial_rise = Length(4000.0);
        *pitch = Length(16000.0);
        *revolutions = 0.5;
        *start_angle = Angle(std::f64::consts::FRAC_PI_2);
        *clockwise = true;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::HelixNativeAxis {
            axial_rise: Length(4000.0),
            pitch: Length(16000.0),
            revolutions: 0.5,
            start_angle: Angle(value),
            clockwise: true,
            ..
        } if (value - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
}

#[test]
fn semantic_writer_rejects_embedded_helix_geometry_edits() {
    use cadmpeg_ir::features::{FeatureDefinition, Length};

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
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
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
        );
        let FeatureDefinition::Helix { radius, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("embedded helix geometry");
        };
        *radius = Length(9.0);
    }

    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
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
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ProfileRef, WrapMode};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><Wrap Name="Mark" Type="Wrap" id="31" Profile="{face}" Face="{face}" Mode="Emboss" Method="Spline"><Dimension Name="Depth">2mm</Dimension></Wrap></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Wrap {
            profile: ProfileRef::Faces(faces),
            face: FaceSelection::Resolved { faces: targets, native },
            mode: WrapMode::Emboss,
            depth: Some(Length(2.0)),
        } if faces == std::slice::from_ref(&face_id) && targets == std::slice::from_ref(&face_id) && native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Wrap {
            profile,
            face,
            mode,
            depth,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed wrap");
        };
        *profile = ProfileRef::Faces(vec![face_id.clone()]);
        *face = FaceSelection::Faces(vec![face_id.clone()]);
        *mode = WrapMode::Deboss;
        *depth = Some(Length(3.5));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Profile"], face_id.0);
    assert_eq!(native.properties["Face"], face_id.0);
    assert_eq!(native.properties["Mode"], "Deboss");
    assert_eq!(native.properties["Method"], "Spline");
    assert_eq!(native.parameters["Depth"], "3.5mm");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Wrap {
            mode: WrapMode::Deboss,
            depth: Some(Length(3.5)),
            ..
        }
    ));

    let mut scribed = regenerated;
    {
        let mut ir_edit = scribed.ir_mut();
        let FeatureDefinition::Wrap { mode, depth, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed wrap");
        };
        *mode = WrapMode::Scribe;
        *depth = None;
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(scribed.ir(), scribed.source_fidelity(), &mut encoded)
        .unwrap();
    let scribed = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(scribed.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Mode"], "Scribe");
    assert!(!native.parameters.contains_key("Depth"));
    assert!(matches!(
        scribed.ir().model.features[0].definition,
        FeatureDefinition::Wrap {
            mode: WrapMode::Scribe,
            depth: None,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_move_copy_body() {
    use cadmpeg_ir::features::{Angle, AxisAngle, BodySelection, FeatureDefinition};
    use cadmpeg_ir::math::{Point3, Vector3};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let body = base.ir().model.bodies[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><MoveBody Name="Copy" Type="MoveCopyBody" id="32" Bodies="{body}" Translation="1mm,2mm,3mm" RotationOrigin="4mm,5mm,6mm" RotationAxis="0,0,1" Copies="2" Frame="model"><Dimension Name="Rotation">90deg</Dimension></MoveBody></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let body_id = decoded.ir().model.bodies[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Resolved { bodies, native },
            translation: Vector3 { x: 1.0, y: 2.0, z: 3.0 },
            rotation: Some(AxisAngle {
                origin: Point3 { x: 4.0, y: 5.0, z: 6.0 },
                direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                angle: Angle(angle),
            }),
            copies: 2,
        } if bodies == std::slice::from_ref(&body_id) && native == &body
            && (*angle - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::MoveBody {
            bodies,
            translation,
            rotation,
            copies,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed body motion");
        };
        *bodies = BodySelection::Bodies(vec![body_id.clone()]);
        *translation = Vector3::new(-7.0, 8.0, 9.0);
        *rotation = Some(AxisAngle {
            origin: Point3::new(10.0, 11.0, 12.0),
            direction: Vector3::new(0.0, 1.0, 0.0),
            angle: Angle(0.25),
        });
        *copies = 3;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Bodies"], body_id.0);
    assert_eq!(native.properties["Translation"], "-7mm,8mm,9mm");
    assert_eq!(native.properties["RotationOrigin"], "10mm,11mm,12mm");
    assert_eq!(native.properties["RotationAxis"], "0,1,0");
    assert_eq!(native.properties["Copies"], "3");
    assert_eq!(native.properties["Frame"], "model");
    assert_eq!(native.parameters["Rotation"], "0.25rad");

    let mut translated = regenerated;
    {
        let mut ir_edit = translated.ir_mut();
        let FeatureDefinition::MoveBody {
            rotation, copies, ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed body motion");
        };
        *rotation = None;
        *copies = 0;
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
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
        translated.ir().model.features[0].definition,
        FeatureDefinition::MoveBody {
            rotation: None,
            copies: 0,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_offset_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><OffsetSurface Name="Offset" Type="OffsetSurface" id="33" Faces="{face}" Knit="true"><Dimension Name="Distance">2mm</Dimension></OffsetSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(Length(2.0)),
        } if faces == std::slice::from_ref(&face_id) && native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::OffsetSurface { faces, distance } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed offset surface");
        };
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
        *distance = Some(Length(-3.5));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["Knit"], "true");
    assert_eq!(native.parameters["Distance"], "-3.5mm");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::OffsetSurface {
            distance: Some(Length(-3.5)),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_knit_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><KnitSurface Name="Knit" Type="Knit" id="34" Faces="{face}" MergeEntities="false" CreateSolid="false" CheckGeometry="true"><Dimension Name="GapTolerance">0.01mm</Dimension></KnitSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::KnitSurface {
            faces: FaceSelection::Resolved { faces, native },
            merge_entities: Some(false),
            create_solid: Some(false),
            gap_tolerance: Some(Length(0.01)),
        } if faces == std::slice::from_ref(&face_id) && native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed knit surface");
        };
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
        *merge_entities = Some(true);
        *create_solid = Some(true);
        *gap_tolerance = None;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["MergeEntities"], "true");
    assert_eq!(native.properties["CreateSolid"], "true");
    assert_eq!(native.properties["CheckGeometry"], "true");
    assert!(!native.parameters.contains_key("GapTolerance"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
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
    let body = base.ir().model.bodies[0].id.0.clone();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><CutWithSurface Name="Cut" Type="SurfaceCut" id="35" Targets="{body}" Tools="{face}" Reverse="false" ConsumeTool="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let body_id = decoded.ir().model.bodies[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::CutWithSurface {
            targets: BodySelection::Resolved { bodies, native: body_native },
            tools: FaceSelection::Resolved { faces, native: face_native },
            reverse: Some(false),
        } if bodies == std::slice::from_ref(&body_id) && body_native == &body
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::CutWithSurface {
            targets,
            tools,
            reverse,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed surface cut");
        };
        *targets = BodySelection::Bodies(vec![body_id.clone()]);
        *tools = FaceSelection::Faces(vec![face_id.clone()]);
        *reverse = Some(true);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Targets"], body_id.0);
    assert_eq!(native.properties["Tools"], face_id.0);
    assert_eq!(native.properties["Reverse"], "true");
    assert_eq!(native.properties["ConsumeTool"], "false");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
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
    let body = base.ir().model.bodies[0].id.0.clone();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><CutWithSurface Name="Cut" Type="SurfaceCut" id="35" Targets="{body}" Tools="{face}" ConsumeTool="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::CutWithSurface {
            targets: BodySelection::Resolved { .. },
            tools: FaceSelection::Resolved { .. },
            reverse: None,
        }
    ));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert!(!native.properties.contains_key("Reverse"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::CutWithSurface { reverse: None, .. }
    ));
}

#[test]
fn semantic_writer_round_trips_filled_surface() {
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
    let edge = base.ir().model.edges[0].id.0.clone();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><FilledSurface Name="Fill" Type="FillSurface" id="36" Boundary="{edge}" SupportFaces="{face}" Continuity="Tangent" MergeResult="false" Optimize="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Resolved { edges, native: edge_native }),
            support_faces: FaceSelection::Resolved { faces, native: face_native },
            continuity: Some(SurfaceContinuity::Tangent),
            boundary_continuities,
            merge_result: Some(false),
        } if boundary_continuities.is_empty()
            && edges == std::slice::from_ref(&edge_id) && edge_native == &edge
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::FilledSurface {
            boundary,
            support_faces,
            continuity,
            boundary_continuities,
            merge_result,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed filled surface");
        };
        *boundary = cadmpeg_ir::features::SurfaceBoundary::Edges(EdgeSelection::Edges(vec![
            edge_id.clone(),
        ]));
        *support_faces = FaceSelection::Faces(vec![face_id.clone()]);
        *continuity = Some(SurfaceContinuity::Curvature);
        boundary_continuities.clear();
        *merge_result = Some(true);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Boundary"], edge_id.0);
    assert_eq!(native.properties["SupportFaces"], face_id.0);
    assert_eq!(native.properties["Continuity"], "Curvature");
    assert_eq!(native.properties["MergeResult"], "true");
    assert_eq!(native.properties["Optimize"], "true");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::FilledSurface {
            continuity: Some(SurfaceContinuity::Curvature),
            merge_result: Some(true),
            ..
        }
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
    let edge = base.ir().model.edges[0].id.0.clone();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><TrimSurface Name="Trim" Type="SurfaceTrim" id="37" Faces="{face}" Tool="{edge}" Keep="Inside" Split="false"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Resolved { faces, native },
            tool: PathRef::Edges(edges),
            keep: TrimRegion::Inside,
        } if faces == std::slice::from_ref(&face_id) && native == &face && edges == std::slice::from_ref(&edge_id)
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::TrimSurface { faces, tool, keep } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed trim surface");
        };
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
        *tool = PathRef::Edges(vec![edge_id.clone()]);
        *keep = TrimRegion::Outside;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["Tool"], edge_id.0);
    assert_eq!(native.properties["Keep"], "Outside");
    assert_eq!(native.properties["Split"], "false");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::TrimSurface {
            keep: TrimRegion::Outside,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_extend_surface() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, SurfaceExtension};

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><ExtendSurface Name="Extend" Type="SurfaceExtend" id="38" Faces="{face}" Method="Natural" CornerMode="Merge"><Dimension Name="Distance">2mm</Dimension></ExtendSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Resolved { faces, native },
            distance: Some(Length(2.0)),
            method: SurfaceExtension::Natural,
        } if faces == std::slice::from_ref(&face_id) && native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::ExtendSurface {
            faces,
            distance,
            method,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed extended surface");
        };
        *faces = FaceSelection::Faces(vec![face_id.clone()]);
        *distance = Some(Length(4.5));
        *method = SurfaceExtension::Linear;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Faces"], face_id.0);
    assert_eq!(native.properties["Method"], "Linear");
    assert_eq!(native.properties["CornerMode"], "Merge");
    assert_eq!(native.parameters["Distance"], "4.5mm");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::ExtendSurface {
            distance: Some(Length(4.5)),
            method: SurfaceExtension::Linear,
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_all_ruled_surface_modes() {
    use cadmpeg_ir::features::{
        EdgeSelection, FaceSelection, FeatureDefinition, Length, RuledSurfaceMode,
    };
    use cadmpeg_ir::math::Vector3;

    let base_bytes = sldprt_with_body(&triangle_body());
    let base = SldprtCodec
        .decode(
            &mut Cursor::new(base_bytes.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let edge = base.ir().model.edges[0].id.0.clone();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><RuledSurface Name="Ruled" Type="SurfaceRuled" id="39" Edges="{edge}" SupportFaces="{face}" Mode="Direction" Direction="0,0,1" Trim="true"><Dimension Name="Distance">2mm</Dimension></RuledSurface></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::RuledSurface {
            edges: EdgeSelection::Resolved { edges, native: edge_native },
            support_faces: FaceSelection::Resolved { faces, native: face_native },
            mode: RuledSurfaceMode::Direction {
                direction: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                distance: Length(2.0),
            },
            ..
        } if edges == std::slice::from_ref(&edge_id) && edge_native == &edge
            && faces == std::slice::from_ref(&face_id) && face_native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::RuledSurface {
            edges,
            support_faces,
            mode,
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed ruled surface");
        };
        *edges = EdgeSelection::Edges(vec![edge_id.clone()]);
        *support_faces = FaceSelection::Faces(vec![face_id.clone()]);
        *mode = RuledSurfaceMode::Normal {
            distance: Length(3.0),
        };
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let mut regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Mode"], "Normal");
    assert!(!native.properties.contains_key("Direction"));
    assert_eq!(native.properties["Trim"], "true");
    assert_eq!(native.parameters["Distance"], "3mm");

    {
        let mut ir_edit = regenerated.ir_mut();
        let FeatureDefinition::RuledSurface { mode, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed ruled surface");
        };
        *mode = RuledSurfaceMode::Tangent {
            distance: Length(4.0),
        };
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            regenerated.ir(),
            regenerated.source_fidelity(),
            &mut encoded,
        )
        .unwrap();
    let tangent = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        tangent.ir().model.features[0].definition,
        FeatureDefinition::RuledSurface {
            mode: RuledSurfaceMode::Tangent {
                distance: Length(4.0)
            },
            ..
        }
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
    let edge = base.ir().model.edges[0].id.0.clone();
    let face = base.ir().model.faces[0].id.0.clone();
    let xml = format!(
        r#"<Keywords><ProjectedCurve Name="Projection" Type="ProjectionCurve" id="40" Source="{edge}" TargetFaces="{face}" Direction="0,0,1" Bidirectional="false" Simplify="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let edge_id = decoded.ir().model.edges[0].id.clone();
    let face_id = decoded.ir().model.faces[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::ProjectedCurve {
            source: PathRef::Edges(edges),
            target_faces: FaceSelection::Resolved { faces, native },
            direction: cadmpeg_ir::features::CurveProjectionDirection::Vector(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            bidirectional: Some(false),
        } if edges == std::slice::from_ref(&edge_id) && faces == std::slice::from_ref(&face_id) && native == &face
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::ProjectedCurve {
            source,
            target_faces,
            direction,
            bidirectional,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed projected curve");
        };
        *source = PathRef::Edges(vec![edge_id.clone()]);
        *target_faces = FaceSelection::Faces(vec![face_id.clone()]);
        *direction = cadmpeg_ir::features::CurveProjectionDirection::State(
            cadmpeg_ir::features::CurveProjectionDirectionState::TargetNormal,
        );
        *bidirectional = Some(true);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.properties["Source"], edge_id.0);
    assert_eq!(native.properties["TargetFaces"], face_id.0);
    assert_eq!(native.properties["Bidirectional"], "true");
    assert_eq!(native.properties["Simplify"], "true");
    assert!(!native.properties.contains_key("Direction"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
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
    let first = base.ir().model.edges[0].id.0.clone();
    let second = base.ir().model.edges[1].id.0.clone();
    let xml = format!(
        r#"<Keywords><CompositeCurve Name="Chain" Type="CompositeCurve" id="41" Segments="{first};{second}" Closed="false" Simplify="true"/></Keywords>"#
    );
    let mut source = base_bytes;
    source.extend(make_block(0x42, "Contents/Keywords", xml.as_bytes()));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let first_id = decoded.ir().model.edges[0].id.clone();
    let second_id = decoded.ir().model.edges[1].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::CompositeCurve { segments, closed: false }
            if segments == &vec![
                PathRef::Edges(vec![first_id.clone()]),
                PathRef::Edges(vec![second_id.clone()]),
            ]
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::CompositeCurve { segments, closed } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed composite curve");
        };
        *segments = vec![
            PathRef::Edges(vec![second_id.clone()]),
            PathRef::Edges(vec![first_id.clone()]),
        ];
        *closed = true;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(
        native.properties["Segments"],
        format!("{};{}", second_id.0, first_id.0)
    );
    assert_eq!(native.properties["Closed"], "true");
    assert_eq!(native.properties["Simplify"], "true");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::CompositeCurve { segments, closed: true }
            if segments == &vec![
                PathRef::Edges(vec![second_id]),
                PathRef::Edges(vec![first_id]),
            ]
    ));
}

#[test]
fn semantic_writer_round_trips_typed_revolution() {
    use cadmpeg_ir::features::{Angle, BooleanOp, FeatureDefinition, RevolveExtent, Termination};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Revolve Name="Turn" Type="Revolve" id="17" AxisOrigin="10mm,20mm,30mm" AxisDirection="0,1,0" Operation="Join"><Dimension Name="Angle">180deg</Dimension></Revolve></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(cadmpeg_ir::features::RevolutionAxis {
                    origin: Point3 { x: 10.0, y: 20.0, z: 30.0 },
                    direction: Vector3 { x: 0.0, y: 1.0, z: 0.0 },
                }),
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(value) },
                }),
                ..
            },
            op: BooleanOp::Join,
        } if (*value - std::f64::consts::PI).abs() < 1e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Revolve { construction, op } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed revolution feature");
        };
        let Some(axis) = construction.axis.as_mut() else {
            panic!("resolved revolution axis");
        };
        axis.origin = Point3::new(1.0, 2.0, 3.0);
        axis.direction = Vector3::new(0.0, 0.0, 1.0);
        construction.extent = Some(RevolveExtent::OneSided {
            termination: Termination::Angle {
                angle: Angle(std::f64::consts::FRAC_PI_2),
            },
        });
        *op = BooleanOp::Cut;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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

    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: None,
                axis: Some(cadmpeg_ir::features::RevolutionAxis {
                    origin: Point3 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0
                    },
                    direction: Vector3 {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0
                    },
                }),
                extent: None,
                ..
            },
            op: BooleanOp::Unresolved,
        }
    ));
    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, decoded.source_fidelity(), &mut Vec::new())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("unresolved revolution construction"));
    decoded.ir_mut().model.features[0].name = Some("Renamed turn".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(_),
                profile: None,
                extent: None,
                ..
            },
            op: BooleanOp::Unresolved,
        }
    ));
}

#[test]
fn semantic_writer_round_trips_all_revolution_extents() {
    use cadmpeg_ir::features::{
        Angle, BooleanOp, FeatureDefinition, ProfileRef, RevolveExtent, Termination,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="TurnProfile" Type="Sketch" id="40"/><Revolve Name="One" Type="Revolve" id="41" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,0,1" EndCondition="OneSided" Operation="Join"><Dimension Name="Angle">90deg</Dimension></Revolve><Revolve Name="Sym" Type="Revolve" id="42" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="0,1,0" EndCondition="Symmetric" Operation="NewBody"><Dimension Name="Angle">180deg</Dimension></Revolve><Revolve Name="Two" Type="Revolve" id="43" Profile="40" AxisOrigin="0mm,0mm,0mm" AxisDirection="1,0,0" EndCondition="TwoSided" Operation="Cut"><Dimension Name="Angle">30deg</Dimension><Dimension Name="Angle2">60deg</Dimension></Revolve></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let profile_feature = decoded.ir().model.features[0].id.clone();
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: Some(ProfileRef::Feature(profile)),
                extent: Some(RevolveExtent::OneSided {
                    termination: Termination::Angle { angle: Angle(value) },
                }),
                ..
            },
            op: BooleanOp::Join,
        } if profile == &profile_feature && (*value - 90f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::Symmetric {
                    termination: Termination::Angle { angle: Angle(value) },
                }),
                ..
            },
            op: BooleanOp::NewBody,
        } if (value - std::f64::consts::PI).abs() < 1e-12
    ));
    assert!(matches!(
        decoded.ir().model.features[3].definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(RevolveExtent::TwoSided {
                    first: Termination::Angle { angle: Angle(first) },
                    second: Termination::Angle { angle: Angle(second) },
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (first - 30f64.to_radians()).abs() < 1e-12
            && (second - 60f64.to_radians()).abs() < 1e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Revolve { construction, op } =
            &mut ir_edit.model.features[3].definition
        else {
            panic!("typed revolution");
        };
        construction.extent = Some(RevolveExtent::OneSided {
            termination: Termination::Angle { angle: Angle(0.75) },
        });
        *op = BooleanOp::Intersect;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
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
