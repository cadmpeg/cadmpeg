// SPDX-License-Identifier: Apache-2.0
//! Sketch-history binding and sketch-geometry projection decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_projects_nested_feature_input_profile_as_a_sketch() {
    use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchGeometry, SketchLocus};

    let source = sldprt_with_nested_sketch_profile(&triangle_body());
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();

    assert_eq!(decoded.ir().model.sketches.len(), 1);
    assert_eq!(decoded.ir().model.sketch_entities.len(), 3);
    assert_eq!(decoded.ir().model.sketch_constraints.len(), 3);
    let sketch = &decoded.ir().model.sketches[0];
    assert_eq!(sketch.configuration.as_deref(), Some("0"));
    let (origin, normal, _) = sketch
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(normal, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(sketch.profiles.len(), 1);
    assert_eq!(sketch.profiles[0].len(), 3);
    assert!(decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .all(|entity| matches!(entity.geometry, SketchGeometry::Line { .. })));
    assert!(decoded.ir().model.sketch_entities.iter().all(|entity| {
        entity
            .native_ref
            .as_deref()
            .is_some_and(|id| id.contains(":sldprt:brep:edge#"))
            && entity.endpoint_refs.len() == 2
            && entity
                .endpoint_refs
                .iter()
                .all(|id| id.contains(":sldprt:brep:point#"))
    }));
    assert!(decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .all(|constraint| {
            matches!(
                &constraint.definition,
                SketchConstraintDefinition::CoincidentLoci { loci }
                    if loci.len() == 2
                        && loci.iter().all(|locus| matches!(
                            locus,
                            SketchLocus::Start(_) | SketchLocus::End(_)
                        ))
            )
        }));
    assert!(sketch.native_ref.as_deref().is_some_and(|native_ref| {
        native_ref.starts_with("sldprt:feature-input:resolved-features#")
    }));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_binds_profile_stream_by_feature_object_interval() {
    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch = decoded
        .ir()
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some("Sketch1"))
        .expect("named feature-input sketch");
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sketch history feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_sweep() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir().model.sketches.as_slice() else {
        panic!("one enclosed sweep profile stream");
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(id)),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_sweep() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sweep Name="Sketch1" Type="Sweep"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("sweep history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let [sketch] = decoded.ir().model.sketches.as_slice() else {
        panic!("one enclosed extrusion profile stream");
    };
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(id),
            ..
        } if id == &sketch.id
    ));
}

#[test]
fn decode_binds_configuration_sketch_state_after_geometry_projection() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" id="0"/><Sketch Name="Sketch1" Type="Sketch" id="0"/></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("projected sketch feature");
    assert!(matches!(
        &decoded.ir().model.configurations[0].feature_states[&feature.id].definition,
        FeatureDefinition::Sketch {
            sketch: Some(configuration_sketch),
            ..
        } if decoded.ir().model.sketches.iter().any(|sketch| &sketch.id == configuration_sketch)
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profiles(&triangle_body(), 2);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Extrusion Name="Sketch1" Type="Boss-Extrude"><Dimension Name="D1">25</Dimension></Extrusion></Keywords>"#,
    ));

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let feature = decoded
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Sketch1"))
        .expect("extrusion history feature");
    assert!(matches!(
        &feature.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            ..
        }
    ));
}

#[test]
fn decode_binds_unique_sketch_history_to_profile_consumers() {
    use cadmpeg_ir::features::{FeatureDefinition, ProfileRef};

    let mut source = sldprt_with_nested_sketch_profile(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Profile" Type="Sketch" id="21"/><Rib Name="Web" Type="Rib" id="22" Profile="21" Direction="0,1,0" BothSides="false" Operation="Join"><Dimension Name="Thickness">2mm</Dimension></Rib></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let sketch_id = decoded.ir().model.sketches[0].id.clone();
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Planar,
            sketch: Some(value), ..
        } if value == &sketch_id
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        &feature.definition,
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile: Some(ProfileRef::Sketch(value)),
                ..
            },
            ..
        } if value == &sketch_id
    )));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let mut written = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut written)
        .unwrap();
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .unwrap();
    assert!(round_trip
        .ir()
        .model
        .features
        .iter()
        .any(|feature| matches!(
            feature.definition,
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(_),
                ..
            }
        )));
}

#[test]
fn matching_numbered_sketch_alias_binds_the_base_geometry() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureId, ProfileRef,
        Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId};

    let sketch_id = SketchId("sketch".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![cadmpeg_ir::sketches::SketchEntityUse {
            entity: cadmpeg_ir::sketches::SketchEntityId("sketch:entity".into()),
            reversed: false,
        }]],
        native_ref: None,
    };
    let neutral =
        |id: &str, name: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: Some(name.into()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Sketch".into()),
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: Some(native_ref.into()),
        };
    let mut features = vec![
        neutral(
            "base",
            "Profile",
            "native-base",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "alias",
            "Profile<3>",
            "native-alias",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "different",
            "Profile<4>",
            "native-different",
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: None,
            },
        ),
        neutral(
            "consumer",
            "Boss",
            "native-consumer",
            FeatureDefinition::Extrude {
                profile: ProfileRef::Native("native-alias".into()),
                direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Unresolved,
                        draft: None,
                        offset: None,
                    },
                },
                op: BooleanOp::Join,
                direction_source: None,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        ),
    ];
    let native = |id: &str, name: &str, depth: &str| crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
        parent_source_id: None,
        ordinal: 0,
        name: name.into(),
        kind: "Sketch".into(),
        input_class: Some("moProfileFeature_c".into()),
        suppressed: false,
        parameters: BTreeMap::from([("Depth".into(), depth.into())]),
        dimension_properties: BTreeMap::new(),
        properties: BTreeMap::new(),
        text: None,
        content: vec![crate::records::FeatureContent::Dimension("Depth".into())],
    };
    let history = crate::records::FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            native("native-base", "Profile", "2mm"),
            native("native-alias", "Profile<3>", "2mm"),
            native("native-different", "Profile<4>", "3mm"),
        ],
    };

    crate::history::bind_unique_sketch_feature(&mut features, &[sketch], &[history]);

    assert!(matches!(
        &features[1].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert_eq!(features[1].dependencies, vec![FeatureId("base".into())]);
    assert!(matches!(
        &features[2].definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    ));
    assert!(matches!(
        &features[3].definition,
        FeatureDefinition::Extrude { profile: ProfileRef::Sketch(id), .. } if id == &sketch_id
    ));
    assert_eq!(features[3].dependencies, vec![FeatureId("base".into())]);
}

#[test]
fn decode_binds_multiple_sketch_history_nodes_by_exact_name() {
    use cadmpeg_ir::features::{FeatureDefinition, PathRef, ProfileRef};

    let mut source = sldprt_with_nested_nurbs_sketches(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="feature input spline sketch" Type="Sketch" id="21"/><Sketch Name="feature input rational spline sketch" Type="Sketch" id="22"/><Sweep Name="Pipe" Type="Sweep" id="23" Profile="21" Path="22" Operation="NewBody"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let bound = decoded
        .ir()
        .model
        .features
        .iter()
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch),
                ..
            } => Some(sketch.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match &feature.definition {
            FeatureDefinition::Sweep {
                section: cadmpeg_ir::features::SweepSection::Profile(ProfileRef::Sketch(profile)),
                path: Some(PathRef::Sketch(path)),
                ..
            } => Some((profile, path)),
            _ => None,
        })
        .expect("bound sweep");
    assert_ne!(sweep.0, sweep.1);
    assert!(bound.contains(sweep.0) && bound.contains(sweep.1));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
}

#[test]
fn decode_does_not_bind_duplicate_sketch_names_by_order() {
    use cadmpeg_ir::features::FeatureDefinition;

    let mut source = sldprt_with_body(&triangle_body());
    let mut payload = resolved_features_payload(&[1, 1]);
    for _ in 0..2 {
        payload.extend(parasolid_with_body(
            "Duplicate",
            "SCH_SW_33103_11000",
            &nurbs_sketch_body(false),
        ));
    }
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &payload,
    ));
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Duplicate" Type="Sketch" id="21"/><Sketch Name="Duplicate" Type="Sketch" id="22"/></Keywords>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model.sketches.len(), 2);
    assert!(decoded.ir().model.features.iter().all(|feature| matches!(
        feature.definition,
        FeatureDefinition::Sketch { sketch: None, .. }
    )));
}

#[test]
fn decode_distinguishes_full_circle_sketch_geometry() {
    use cadmpeg_ir::features::Length;
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.sketches[0].profiles[0].len(), 1);
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Circle {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            radius: Length(1000.0),
        }
    ));
}

#[test]
fn decode_projects_full_ellipse_sketch_geometry() {
    use cadmpeg_ir::features::{Angle, Length};
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry,
        SketchGeometry::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            major_angle: Angle(value),
            major_radius: Length(2000.0),
            minor_radius: Length(1000.0),
            start_angle: None,
            end_angle: None,
        } if (value - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
}

#[test]
fn decode_projects_non_rational_and_rational_nurbs_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometry;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_nurbs_sketches(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let splines = decoded
        .ir()
        .model
        .sketch_entities
        .iter()
        .filter_map(|entity| match &entity.geometry {
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } => Some((degree, knots, control_points, weights, periodic)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines.iter().all(|(degree, knots, points, _, periodic)| {
        **degree == 2
            && knots.as_slice() == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && points.len() == 3
            && !**periodic
    }));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| weights.is_none()));
    assert!(splines
        .iter()
        .any(|(_, _, _, weights, _)| { weights.as_deref() == Some(&[1.0, 0.5, 1.0]) }));
}
