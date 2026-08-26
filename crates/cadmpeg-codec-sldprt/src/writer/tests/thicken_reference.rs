// SPDX-License-Identifier: Apache-2.0
//! Semantic writer tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn semantic_writer_round_trips_typed_shell() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Shell Name="Thin" Type="Shell" id="14" RemovedFaces="face:4" Outward="false"><Dimension Name="Thickness">0.08in</Dimension></Shell></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Native(selection),
            thickness: Some(Length(value)),
            outward: Some(false),
            ..
        } if selection == "face:4" && (*value - 2.032).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Shell {
            removed_faces,
            thickness,
            outward,
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed shell feature");
        };
        *thickness = Some(Length(3.0));
        *outward = Some(true);
        *removed_faces = FaceSelection::Native("face:5,face:6".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert_eq!(feature.properties["RemovedFaces"], "face:5,face:6");
    assert_eq!(feature.properties["Outward"], "true");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Shell {
            thickness: Some(Length(3.0)),
            outward: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_typed_thicken() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Thicken Name="Wall" Type="Thicken" id="15" Faces="face:4" BothSides="false" Reverse="true"><Dimension Name="Thickness">0.08in</Dimension></Thicken></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Native(selection),
            thickness: Some(Length(value)),
            side: Some(ThickenSide::Reverse),
        } if selection == "face:4" && (*value - 2.032).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Thicken {
            faces,
            thickness,
            side,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed thicken feature");
        };
        *faces = FaceSelection::Native("face:5,face:6".into());
        *thickness = Some(Length(3.0));
        *side = Some(ThickenSide::Both);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.parameters["Thickness"], "3mm");
    assert_eq!(feature.properties["Faces"], "face:5,face:6");
    assert_eq!(feature.properties["BothSides"], "true");
    assert_eq!(feature.properties["Reverse"], "false");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Thicken {
            thickness: Some(Length(3.0)),
            side: Some(ThickenSide::Both),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_positional_thicken_dimension() {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, Length, ParameterValue, ThickenSide,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Wall" Type="Thicken" id="15"><Dimension Name="D1">6</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moThicken_c", "Wall", 15)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: Some(Length(6.0)),
            side: Some(ThickenSide::Forward),
        }
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length(6.0)))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Thicken { thickness, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed positional thicken");
        };
        *thickness = Some(Length(8.5));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.parameters["D1"], "8.5");
    assert!(!native.parameters.contains_key("Thickness"));
    assert!(!native.properties.contains_key("BothSides"));
    assert!(!native.properties.contains_key("Reverse"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Thicken {
            thickness: Some(Length(8.5)),
            side: Some(ThickenSide::Forward),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_typed_scale() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, ScaleCenter, ScaleFactors};
    use cadmpeg_ir::math::Point3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Scale Name="Point" Type="Scale" id="16" Bodies="body:1" Center="1mm,2mm,3mm"><Dimension Name="Factor">2</Dimension></Scale>
            <Scale Name="Centroid" Type="Scale" id="17" Bodies="body:1" CenterType="Centroid"><Dimension Name="Factor">1.1</Dimension></Scale>
            <Scale Name="Origin" Type="Scale" id="18" Bodies="body:1" CenterType="Origin"><Dimension Name="Factor">1.2</Dimension></Scale>
            <Scale Name="Reference" Type="Scale" id="19" Bodies="body:1" CenterType="CoordinateSystem" CenterRef="csys:4"><Dimension Name="Factor">1.3</Dimension></Scale>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Native(selection),
            center: Some(ScaleCenter::Point(Point3 { x: 1.0, y: 2.0, z: 3.0 })),
            factors: ScaleFactors {
                uniform: Some(2.0),
                x: None,
                y: None,
                z: None,
            },
        } if selection == "body:1"
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Centroid),
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::ModelOrigin),
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Native(reference)),
            ..
        } if reference == "csys:4"
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Scale {
            bodies,
            center,
            factors,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed scale feature");
        };
        *bodies = BodySelection::Native("body:2,body:3".into());
        *center = Some(ScaleCenter::Point(Point3::new(4.0, 5.0, 6.0)));
        *factors = ScaleFactors {
            uniform: None,
            x: Some(1.5),
            y: Some(2.0),
            z: Some(2.5),
        };
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Bodies"], "body:2,body:3");
    assert_eq!(feature.properties["Center"], "4mm,5mm,6mm");
    assert!(!feature.parameters.contains_key("Factor"));
    assert_eq!(feature.parameters["ScaleX"], "1.5");
    assert_eq!(feature.parameters["ScaleY"], "2");
    assert_eq!(feature.parameters["ScaleZ"], "2.5");
    let native_features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native_features[1].properties["CenterType"], "Centroid");
    assert!(!native_features[1].properties.contains_key("Center"));
    assert_eq!(native_features[2].properties["CenterType"], "ModelOrigin");
    assert!(!native_features[2].properties.contains_key("Center"));
    assert_eq!(native_features[3].properties["CenterType"], "Reference");
    assert_eq!(native_features[3].properties["CenterRef"], "csys:4");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Point(Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            })),
            factors: ScaleFactors {
                uniform: None,
                x: Some(1.5),
                y: Some(2.0),
                z: Some(2.5),
            },
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir().model.features[1].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Centroid),
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir().model.features[2].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::ModelOrigin),
            ..
        }
    ));
    assert!(matches!(
        &regenerated.ir().model.features[3].definition,
        FeatureDefinition::Scale {
            center: Some(ScaleCenter::Native(reference)),
            ..
        } if reference == "csys:4"
    ));
}

#[test]
fn semantic_writer_retains_partial_native_scale_construction() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, ScaleCenter, ScaleFactors};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Scale Name="Unknown center" Type="Scale" id="71" Bodies="body:1" CenterType="Point" Center="invalid"><Dimension Name="Factor">2</Dimension><Dimension Name="ScaleX">3</Dimension></Scale>
            <Scale Name="Partial axes" Type="Scale" id="72" CenterType="Centroid"><Dimension Name="Factor">0</Dimension><Dimension Name="ScaleX">1.5</Dimension><Dimension Name="ScaleY">NaN</Dimension><Dimension Name="ScaleZ">2.5</Dimension></Scale>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Native(bodies),
            center: None,
            factors: ScaleFactors {
                uniform: Some(2.0),
                x: Some(3.0),
                y: None,
                z: None,
            },
        } if bodies == "body:1"
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Unresolved,
            center: Some(ScaleCenter::Centroid),
            factors: ScaleFactors {
                uniform: None,
                x: Some(1.5),
                y: None,
                z: Some(2.5),
            },
        }
    ));

    for index in 0..2 {
        let mut detached = decoded.ir().clone();
        detached.model.features[index].native_ref = None;
        let error = SldprtCodec
            .write_preserved_with_source_fidelity(
                &detached,
                decoded.source_fidelity(),
                &mut Vec::new(),
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("unresolved scale construction"),
            "{error}"
        );
    }

    for (index, feature) in decoded.ir_mut().model.features.iter_mut().enumerate() {
        feature.name = Some(format!("Renamed scale {}", index + 1));
    }
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].properties["Center"], "invalid");
    assert_eq!(native[0].parameters["Factor"], "2");
    assert_eq!(native[0].parameters["ScaleX"], "3");
    assert_eq!(native[1].parameters["Factor"], "0");
    assert_eq!(native[1].parameters["ScaleY"], "NaN");
    assert_eq!(native[1].parameters["ScaleX"], "1.5");
    assert_eq!(native[1].parameters["ScaleZ"], "2.5");
}

#[test]
fn semantic_writer_round_trips_extrusion_with_unresolved_blind_extent() {
    use cadmpeg_ir::features::{
        ExtrudeDirection, ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination,
    };
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude" id="9" EndCondition="Blind"><Dimension Name="Depth">0mm</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                },
            },
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Extrude { direction, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed extrusion");
        };
        *direction = ExtrudeDirection::Explicit(Vector3::new(0.0, 1.0, 0.0));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["EndCondition"], "Blind");
    assert_eq!(feature.properties["Direction"], "0,1,0");
    assert_eq!(feature.parameters["Depth"], "0mm");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                },
            },
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_extrusion_with_unrecognized_end_condition() {
    use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, FeatureDefinition, Termination};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Boss-Extrude1" Type="BossExtrude" id="9" EndCondition="Unrecognized" Direction="0,0,1" Face="face:1" Vertex="vertex:2"><Dimension Name="Depth">4mm</Dimension><Dimension Name="Depth2">6mm</Dimension><Dimension Name="Draft">3deg</Dimension></Extrusion></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    ..
                },
            },
            ..
        }
    ));

    assert_eq!(
        sldprt_native(decoded.ir()).feature_histories[0].features[0].parameters["Draft"],
        "3deg"
    );
    decoded.ir_mut().model.features[0].name = Some("Renamed extrusion".into());

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.name, "Renamed extrusion");
    assert_eq!(feature.properties["EndCondition"], "Unrecognized");
    assert_eq!(feature.properties["Direction"], "0,0,1");
    assert_eq!(feature.properties["Face"], "face:1");
    assert_eq!(feature.properties["Vertex"], "vertex:2");
    assert_eq!(feature.parameters["Depth"], "4mm");
    assert_eq!(feature.parameters["Depth2"], "6mm");
    assert_eq!(
        feature.parameters["Draft"],
        format!("{}rad", 3f64.to_radians())
    );
}

#[test]
fn semantic_writer_round_trips_typed_draft() {
    use cadmpeg_ir::features::{Angle, FaceSelection, FeatureDefinition};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Draft Name="Taper" Type="Draft" id="18" Faces="face:1,face:2" NeutralPlane="face:3" Direction="0,0,1" Outward="false"><Dimension Name="Angle">3deg</Dimension></Draft></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Native(faces),
            neutral_plane: FaceSelection::Native(neutral_plane),
            parting_tool: None,
            pull_direction: Some(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            pull_plane: None,
            angle: Some(Angle(value)),
            outward: Some(false),
        } if faces == "face:1,face:2"
            && neutral_plane == "face:3"
            && (*value - 3f64.to_radians()).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Draft {
            faces,
            neutral_plane,
            pull_direction,
            angle,
            outward,
            ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed draft");
        };
        *pull_direction = Some(Vector3::new(0.0, 1.0, 0.0));
        *angle = Some(Angle(7f64.to_radians()));
        *outward = Some(true);
        *faces = FaceSelection::Native("face:4".into());
        *neutral_plane = FaceSelection::Native("face:5".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:4");
    assert_eq!(feature.properties["NeutralPlane"], "face:5");
    assert_eq!(feature.properties["Direction"], "0,1,0");
    assert_eq!(feature.properties["Outward"], "true");
    assert_eq!(
        feature.parameters["Angle"],
        format!("{}rad", 7f64.to_radians())
    );
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::Draft {
            pull_direction: Some(Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            }),
            outward: Some(true),
            ..
        }
    ));
}

#[test]
fn semantic_writer_round_trips_draft_without_angle_or_outward() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
    use cadmpeg_ir::math::Vector3;

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Draft Name="Taper" Type="Draft" id="18" Faces="face:1,face:2" NeutralPlane="face:3" Direction="0,0,1"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Native(faces),
            neutral_plane: FaceSelection::Native(neutral_plane),
            parting_tool: None,
            pull_direction: Some(Vector3 { x: 0.0, y: 0.0, z: 1.0 }),
            pull_plane: None,
            angle: None,
            outward: None,
        } if faces == "face:1,face:2" && neutral_plane == "face:3"
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Draft { faces, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed draft");
        };
        *faces = FaceSelection::Native("face:4".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:4");
    assert_eq!(feature.properties["NeutralPlane"], "face:3");
    assert_eq!(feature.properties["Direction"], "0,0,1");
    assert_eq!(feature.properties.get("Outward"), None);
    assert_eq!(feature.parameters.get("Angle"), None);
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Native(faces),
            angle: None,
            outward: None,
            ..
        } if faces == "face:4"
    ));
}

#[test]
fn semantic_writer_preserves_absent_feature_selections() {
    use cadmpeg_ir::features::{
        Angle, ChamferSpec, EdgeSelection, FaceSelection, FeatureDefinition, Length,
    };

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Chamfer Name="Bevel" Type="Chamfer" id="31"><Dimension Name="Distance">2mm</Dimension></Chamfer>
            <Shell Name="Thin" Type="Shell" id="32" Outward="false"><Dimension Name="Thickness">1mm</Dimension></Shell>
            <Draft Name="Taper" Type="Draft" id="33" Direction="0,0,1" Outward="false"><Dimension Name="Angle">3deg</Dimension></Draft>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Chamfer {
            groups,
            ..
        } if matches!(groups.as_slice(), [cadmpeg_ir::features::ChamferGroup {
            edges: EdgeSelection::Unresolved, ..
        }])
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::Shell {
            removed_faces: FaceSelection::Unresolved,
            ..
        }
    ));
    assert!(matches!(
        &decoded.ir().model.features[2].definition,
        FeatureDefinition::Draft {
            faces: FaceSelection::Unresolved,
            neutral_plane: FaceSelection::Unresolved,
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Chamfer { groups, .. } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed chamfer");
        };
        groups[0].spec = ChamferSpec::Distance {
            distance: Length(2.5),
        };
        let FeatureDefinition::Shell { thickness, .. } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed shell");
        };
        *thickness = Some(Length(1.5));
        let FeatureDefinition::Draft { angle, .. } = &mut ir_edit.model.features[2].definition
        else {
            panic!("typed draft");
        };
        *angle = Some(Angle(5f64.to_radians()));
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(features[0].parameters["Distance"], "2.5mm");
    assert!(!features[0].properties.contains_key("Edges"));
    assert_eq!(features[1].parameters["Thickness"], "1.5mm");
    assert!(!features[1].properties.contains_key("RemovedFaces"));
    assert_eq!(
        features[2].parameters["Angle"],
        format!("{}rad", 5f64.to_radians())
    );
    assert!(!features[2].properties.contains_key("Faces"));
    assert!(!features[2].properties.contains_key("NeutralPlane"));
}

#[test]
fn semantic_writer_round_trips_typed_combine() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Combine Name="Union" Type="Combine" id="19" Target="body:1" Tools="body:2,body:3" Operation="Join"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            op: BooleanOp::Join,
            keep_tools: false,
        } if target == "body:1" && tools == "body:2,body:3"
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Combine {
            target, tools, op, ..
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed combine");
        };
        *target = BodySelection::Native("body:4".into());
        *tools = BodySelection::Native("body:5,body:6".into());
        *op = BooleanOp::Intersect;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Target"], "body:4");
    assert_eq!(feature.properties["Tools"], "body:5,body:6");
    assert_eq!(feature.properties["Operation"], "Intersect");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            op: BooleanOp::Intersect,
            keep_tools: false,
        } if target == "body:4" && tools == "body:5,body:6"
    ));
}

#[test]
fn semantic_writer_round_trips_delete_and_keep_body() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <DeleteBody Name="Discard" Type="DeleteBody" id="20" Bodies="body:2,body:3"/>
            <KeepBody Name="Isolate" Type="KeepBody" id="21" Bodies="body:1"/>
        </Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(bodies),
            mode: BodyRetentionMode::DeleteSelected,
        } if bodies == "body:2,body:3"
    ));
    assert!(matches!(
        &decoded.ir().model.features[1].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(bodies),
            mode: BodyRetentionMode::KeepSelected,
        } if bodies == "body:1"
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::DeleteBody { bodies, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed delete body");
        };
        *bodies = BodySelection::Native("body:4".into());
        let FeatureDefinition::DeleteBody { bodies, .. } =
            &mut ir_edit.model.features[1].definition
        else {
            panic!("typed keep body");
        };
        *bodies = BodySelection::Native("body:5,body:6".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let features = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(features[0].properties["Bodies"], "body:4");
    assert_eq!(features[0].properties["Mode"], "Delete");
    assert_eq!(features[1].properties["Bodies"], "body:5,body:6");
    assert_eq!(features[1].properties["Mode"], "Keep");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::DeleteSelected,
            ..
        }
    ));
    assert!(matches!(
        regenerated.ir().model.features[1].definition,
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::KeepSelected,
            ..
        }
    ));
}

#[test]
fn semantic_writer_resolves_sparse_body_delete_keep_operation() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Body-Delete/Keep 1" Type="Body-Delete/Keep " id="20"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        }
    ));

    decoded.ir_mut().model.features[0].name = Some("Retained sparse operation".into());
    let mut sparse_encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut sparse_encoded,
        )
        .unwrap();
    let mut sparse = SldprtCodec
        .decode(&mut Cursor::new(sparse_encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(sparse.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Body-Delete/Keep ");
    assert!(!native.properties.contains_key("Bodies"));
    assert!(!native.properties.contains_key("Mode"));
    assert!(matches!(
        sparse.ir().model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Unresolved,
            mode: BodyRetentionMode::Unresolved,
        }
    ));

    {
        let mut ir_edit = sparse.ir_mut();
        let FeatureDefinition::DeleteBody { bodies, mode } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed sparse body operation");
        };
        *bodies = BodySelection::Native("body:2,body:3".into());
        *mode = BodyRetentionMode::KeepSelected;
    }
    let mut resolved_encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(
            sparse.ir(),
            sparse.source_fidelity(),
            &mut resolved_encoded,
        )
        .unwrap();
    let resolved = SldprtCodec
        .decode(
            &mut Cursor::new(resolved_encoded),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = &sldprt_native(resolved.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Body-Delete/Keep ");
    assert_eq!(native.properties["Bodies"], "body:2,body:3");
    assert_eq!(native.properties["Mode"], "Keep");
    assert!(matches!(
        &resolved.ir().model.features[0].definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Native(bodies),
            mode: BodyRetentionMode::KeepSelected,
        } if bodies == "body:2,body:3"
    ));
}

#[test]
fn semantic_writer_round_trips_typed_delete_face() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><DeleteFace Name="Remove Boss" Type="DeleteFace" id="20" Faces="face:4,face:5" Heal="true"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Native(faces),
            heal: true,
        } if faces == "face:4,face:5"
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::DeleteFace { faces, heal } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed delete face");
        };
        *faces = FaceSelection::Native("face:7".into());
        *heal = false;
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:7");
    assert_eq!(feature.properties["Heal"], "false");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Native(faces),
            heal: false,
        } if faces == "face:7"
    ));
}

#[test]
fn semantic_writer_round_trips_typed_replace_face() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><ReplaceFace Name="Patch" Type="ReplaceFace" id="21" Faces="face:4,face:5" ReplacementFaces="face:8"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Native(targets),
            replacements: FaceSelection::Native(replacements),
        } if targets == "face:4,face:5" && replacements == "face:8"
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::ReplaceFace {
            targets,
            replacements,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed replace face");
        };
        *targets = FaceSelection::Native("face:6".into());
        *replacements = FaceSelection::Native("face:9,face:10".into());
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:6");
    assert_eq!(feature.properties["ReplacementFaces"], "face:9,face:10");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Native(targets),
            replacements: FaceSelection::Native(replacements),
        } if targets == "face:6" && replacements == "face:9,face:10"
    ));
}

#[test]
fn semantic_writer_round_trips_all_move_face_forms() {
    use cadmpeg_ir::features::{Angle, FaceMotion, FaceSelection, FeatureDefinition, Length};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><MoveFace Name="Offset" Type="MoveFace" id="21" Faces="face:1" Mode="Offset"><Dimension Name="Distance">2mm</Dimension></MoveFace><MoveFace Name="Translate" Type="MoveFace" id="22" Faces="face:2" Mode="Translate" Direction="1,0,0"><Dimension Name="Distance">3mm</Dimension></MoveFace><MoveFace Name="Rotate" Type="MoveFace" id="23" Faces="face:3" Mode="Rotate" AxisOrigin="1mm,2mm,3mm" AxisDirection="0,0,1"><Dimension Name="Angle">15deg</Dimension></MoveFace></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::MoveFace {
            motion: FaceMotion::Offset {
                distance: Length(2.0)
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::MoveFace {
            motion: FaceMotion::Translate {
                direction: Vector3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0
                },
                distance: Length(3.0),
            },
            ..
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[2].definition,
        FeatureDefinition::MoveFace {
            motion: FaceMotion::Rotate {
                axis_origin: Point3 { x: 1.0, y: 2.0, z: 3.0 },
                axis_dir: Vector3 { x: 0.0, y: 0.0, z: 1.0 },
                angle: Angle(value),
            },
            ..
        } if (value - 15f64.to_radians()).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::MoveFace { faces, motion } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed move face");
        };
        *faces = FaceSelection::Native("face:8".into());
        *motion = FaceMotion::Translate {
            direction: Vector3::new(0.0, 1.0, 0.0),
            distance: Length(4.0),
        };
        let FeatureDefinition::MoveFace { motion, .. } = &mut ir_edit.model.features[1].definition
        else {
            panic!("typed move face");
        };
        *motion = FaceMotion::Rotate {
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_dir: Vector3::new(1.0, 0.0, 0.0),
            angle: Angle(0.5),
        };
        let FeatureDefinition::MoveFace { motion, .. } = &mut ir_edit.model.features[2].definition
        else {
            panic!("typed move face");
        };
        *motion = FaceMotion::Offset {
            distance: Length(-1.0),
        };
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].properties["Mode"], "Translate");
    assert_eq!(native[0].properties["Faces"], "face:8");
    assert_eq!(native[0].properties["Direction"], "0,1,0");
    assert_eq!(native[0].parameters["Distance"], "4mm");
    assert_eq!(native[1].properties["Mode"], "Rotate");
    assert_eq!(native[1].properties["AxisOrigin"], "0mm,0mm,0mm");
    assert_eq!(native[1].properties["AxisDirection"], "1,0,0");
    assert_eq!(native[1].parameters["Angle"], "0.5rad");
    assert_eq!(native[2].properties["Mode"], "Offset");
    assert_eq!(native[2].parameters["Distance"], "-1mm");
    assert!(!native[2].parameters.contains_key("Angle"));
}

#[test]
fn semantic_writer_round_trips_typed_dome() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Dome Name="Crown" Type="Dome" id="24" Faces="face:9" Elliptical="false" Reverse="false"><Dimension Name="Height">0.25in</Dimension></Dome></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native(faces),
            height: Some(Length(value)),
            elliptical: Some(false),
            reverse: Some(false),
        } if faces == "face:9" && (*value - 6.35).abs() < 1.0e-12
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::Dome {
            faces,
            height,
            elliptical,
            reverse,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed dome");
        };
        *faces = FaceSelection::Native("face:10,face:11".into());
        *height = Some(Length(8.0));
        *elliptical = Some(true);
        *reverse = Some(true);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Faces"], "face:10,face:11");
    assert_eq!(feature.properties["Elliptical"], "true");
    assert_eq!(feature.properties["Reverse"], "true");
    assert_eq!(feature.parameters["Height"], "8mm");
    assert!(matches!(
        &regenerated.ir().model.features[0].definition,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native(faces),
            height: Some(Length(8.0)),
            elliptical: Some(true),
            reverse: Some(true),
        } if faces == "face:10,face:11"
    ));
}

#[test]
fn semantic_writer_retains_partial_native_dome_construction() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Dome Name="Partial dome" Type="Dome" id="25" Faces="face:12" Elliptical="true" Reverse="invalid"><Dimension Name="Height">NaNmm</Dimension></Dome></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        &decoded.ir().model.features[0].definition,
        FeatureDefinition::Dome {
            faces: FaceSelection::Native(faces),
            height: None,
            elliptical: Some(true),
            reverse: None,
        } if faces == "face:12"
    ));

    let mut detached = decoded.ir().clone();
    detached.model.features[0].native_ref = None;
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(&detached, decoded.source_fidelity(), &mut Vec::new())
        .unwrap_err();
    assert!(error.to_string().contains("unresolved dome construction"));

    decoded.ir_mut().model.features[0].name = Some("Renamed dome".into());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.parameters["Height"], "NaNmm");
    assert_eq!(native.properties["Reverse"], "invalid");
    assert_eq!(native.properties["Elliptical"], "true");
}

#[test]
fn semantic_writer_round_trips_principal_reference_planes() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Vorne" Type="Ebene" id="2"/><Feature Name="Oben" Type="Ebene" id="3"/><Feature Name="Rechts" Type="Ebene" id="4"/><Feature Name="Plane2" Type="Plane" id="39"/><Feature Name="Reserved-shaped custom record" Type="Ebene" id="2" NativeRole="custom"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[
            ("moRefPlane_c", "Vorne", 2),
            ("moRefPlane_c", "Oben", 3),
            ("moRefPlane_c", "Rechts", 4),
        ]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for (feature, plane) in decoded.ir().model.features[..3].iter().zip([
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ]) {
        assert_eq!(
            feature.definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
    }
    assert!(matches!(
        &decoded.ir().model.features[3].definition,
        FeatureDefinition::DatumPlaneUnresolved
    ));
    assert!(matches!(
        &decoded.ir().model.features[4].definition,
        FeatureDefinition::Native { kind, .. } if kind == "Ebene"
    ));

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        regenerated.ir().model.features[..3]
            .iter()
            .map(|feature| feature.definition.clone())
            .collect::<Vec<_>>(),
        vec![
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Front,
            },
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Top,
            },
            FeatureDefinition::DatumPrincipalPlane {
                plane: PrincipalPlane::Right,
            },
        ]
    );
    assert_eq!(
        sldprt_native(regenerated.ir()).feature_histories[0].features[0].kind,
        "Ebene"
    );

    decoded.ir_mut().model.features[0].definition = FeatureDefinition::DatumPrincipalPlane {
        plane: PrincipalPlane::Right,
    };
    let error = SldprtCodec
        .write_preserved_with_source_fidelity(
            decoded.ir(),
            decoded.source_fidelity(),
            &mut Vec::new(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("principal-plane role"));
}

#[test]
fn semantic_writer_round_trips_legacy_principal_plane_triplet() {
    use cadmpeg_ir::features::{FeatureDefinition, PrincipalPlane};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="A" Type="LocalizedPlane" id="2"/><Feature Name="B" Type="LocalizedPlane" id="3"/><Feature Name="C" Type="LocalizedPlane" id="4"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    for (feature, plane) in decoded.ir().model.features.iter().zip([
        PrincipalPlane::Front,
        PrincipalPlane::Top,
        PrincipalPlane::Right,
    ]) {
        assert_eq!(
            feature.definition,
            FeatureDefinition::DatumPrincipalPlane { plane }
        );
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.features, regenerated.ir().model.features);
}

#[test]
fn semantic_writer_round_trips_typed_reference_plane() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><ReferencePlane Name="Datum A" Type="ReferencePlane" id="25" Origin="1mm,2mm,3mm" Normal="0,0,1" UAxis="1,0,0"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            },
            normal: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            u_axis: Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::DatumPlane {
            origin,
            normal,
            u_axis,
        } = &mut ir_edit.model.features[0].definition
        else {
            panic!("typed reference plane");
        };
        *origin = Point3::new(25.4, 0.0, -2.0);
        *normal = Vector3::new(0.0, 1.0, 0.0);
        *u_axis = Vector3::new(0.0, 0.0, 1.0);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let feature = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(feature.properties["Origin"], "25.4mm,0mm,-2mm");
    assert_eq!(feature.properties["Normal"], "0,1,0");
    assert_eq!(feature.properties["UAxis"], "0,0,1");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::DatumPlane {
            origin: Point3 {
                x: 25.4,
                y: 0.0,
                z: -2.0
            },
            normal: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
            u_axis: Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
        }
    ));
}

#[test]
fn semantic_writer_round_trips_sparse_localized_offset_plane() {
    use cadmpeg_ir::features::{FeatureDefinition, Length, ParameterValue};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Feature Name="Plano2" Type="Plano" id="549"><Dimension Name="D1">3</Dimension></Feature></Keywords>"#,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Config-0-ResolvedFeatures",
        &resolved_feature_classes_with_ids(&[("moRefPlane_c", "Plano2", 549)]),
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(3.0),
        }
    ));
    assert_eq!(
        decoded.ir().model.parameters[0].value,
        Some(ParameterValue::Length(Length(3.0)))
    );

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::DatumOffsetPlane { distance, .. } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("localized offset plane");
        };
        *distance = Length(-4.5);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features[0];
    assert_eq!(native.kind, "Plano");
    assert_eq!(native.parameters["D1"], "-4.5");
    assert!(!native.properties.contains_key("Reference"));
    assert!(!native.properties.contains_key("Plane"));
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(-4.5),
        }
    ));
}

#[test]
fn semantic_writer_round_trips_reference_axis_and_point() {
    use cadmpeg_ir::features::FeatureDefinition;
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><ReferenceAxis Name="Axis A" Type="ReferenceAxis" id="26" Origin="1mm,2mm,3mm" Direction="0,0,1"/><ReferencePoint Name="Point A" Type="ReferencePoint" id="27" Position="4mm,5mm,6mm"/></Keywords>"#,
    ));
    let mut decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        decoded.ir().model.features[0].definition,
        FeatureDefinition::DatumAxis {
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
        }
    ));
    assert!(matches!(
        decoded.ir().model.features[1].definition,
        FeatureDefinition::DatumPoint {
            position: Point3 {
                x: 4.0,
                y: 5.0,
                z: 6.0
            },
            ..
        }
    ));

    {
        let mut ir_edit = decoded.ir_mut();
        let FeatureDefinition::DatumAxis { origin, direction } =
            &mut ir_edit.model.features[0].definition
        else {
            panic!("typed reference axis");
        };
        *origin = Point3::new(-1.0, 0.0, 2.0);
        *direction = Vector3::new(0.0, 1.0, 0.0);
        let FeatureDefinition::DatumPoint { position, .. } =
            &mut ir_edit.model.features[1].definition
        else {
            panic!("typed reference point");
        };
        *position = Point3::new(7.0, 8.0, 9.0);
    }

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .unwrap();
    let regenerated = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let native = &sldprt_native(regenerated.ir()).feature_histories[0].features;
    assert_eq!(native[0].properties["Origin"], "-1mm,0mm,2mm");
    assert_eq!(native[0].properties["Direction"], "0,1,0");
    assert_eq!(native[1].properties["Position"], "7mm,8mm,9mm");
    assert!(matches!(
        regenerated.ir().model.features[0].definition,
        FeatureDefinition::DatumAxis {
            origin: Point3 {
                x: -1.0,
                y: 0.0,
                z: 2.0
            },
            direction: Vector3 {
                x: 0.0,
                y: 1.0,
                z: 0.0
            },
        }
    ));
    assert!(matches!(
        regenerated.ir().model.features[1].definition,
        FeatureDefinition::DatumPoint {
            position: Point3 {
                x: 7.0,
                y: 8.0,
                z: 9.0
            },
            ..
        }
    ));
}
