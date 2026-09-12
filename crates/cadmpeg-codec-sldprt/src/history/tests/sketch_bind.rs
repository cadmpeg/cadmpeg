// SPDX-License-Identifier: Apache-2.0
//! Sketch-history binding and sketch-geometry projection decode tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_projects_nested_feature_input_profile_as_a_sketch() {
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchGeometryDefinition, SketchLocus,
    };

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
        .all(|entity| matches!(
            *entity.geometry.definition(),
            SketchGeometryDefinition::Line { .. }
        )));
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
                constraint.definition.kind(),
                SketchConstraintDefinitionInput::CoincidentLoci { loci }
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
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(id)),
        }) if id == &sketch.id
    ));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_sweep() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef};

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
        feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            ..
        }) if matches!((shape.referenced_profile(),), (Some(PlanarProfileRef::Sketch(id)),) if id == &sketch.id)));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_sweep() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

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
        feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape,
            ..
        }) if shape.section_is_unresolved()));
}

#[test]
fn decode_binds_uniquely_enclosed_profile_stream_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};

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
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Sketch(id)),
            ..
        }) if id == &sketch.id
    ));
}

#[test]
fn decode_binds_configuration_sketch_state_after_geometry_projection() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

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
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(configuration_sketch)),
            ..
        }) if decoded.ir().model.sketches.iter().any(|sketch| &sketch.id == configuration_sketch)
    ));
}

#[test]
fn decode_does_not_bind_ambiguous_enclosed_profile_streams_to_extrusion() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};

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
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            ..
        })
    ));
}

#[test]
fn decode_binds_unique_sketch_history_to_profile_consumers() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

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
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(value)),
        }) if value == &sketch_id
    )));
    assert!(decoded.ir().model.features.iter().any(|feature| matches!(
        feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                profile,
                ..
            },
            ..
        }) if matches!((profile.as_ref(),), (Some(cadmpeg_ir::features::PlanarProfileRef::Sketch(value)),) if value == &sketch_id))));
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new());
    assert!(validation.is_ok(), "{:?}", validation.findings);
    let mut written = Vec::new();
    crate::test_support::plan_inherited_write(
        decoded.ir(),
        decoded.source_fidelity(),
        &mut written,
    )
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
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(_))
            })
        )));
}

#[test]
fn matching_numbered_sketch_alias_binds_the_base_geometry() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureId, FeatureOperation,
        LinearTermination, PlanarProfileRef, ProfileRef,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{Sketch, SketchId};

    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![
            cadmpeg_ir::sketches::SketchEntityUse {
                entity: cadmpeg_ir::sketches::SketchEntityId::mint(
                    "synthetic:test:id#sketch:entity",
                )
                .unwrap(),
                reversed: false,
            },
        ]])
        .unwrap(),
        native_ref: None,
    };
    let neutral =
        |id: &str, name: &str, native_ref: &str, definition| cadmpeg_ir::features::Feature {
            id: FeatureId::mint(id).expect("identity grammar"),
            ordinal: 0,
            name: Some(name.into()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Sketch".into()),
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
            native_ref: Some(native_ref.into()),
        };
    let mut features = vec![
        neutral(
            "synthetic:test:id#base",
            "Profile",
            "native-base",
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }),
        ),
        neutral(
            "synthetic:test:id#alias",
            "Profile<3>",
            "native-alias",
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }),
        ),
        neutral(
            "synthetic:test:id#different",
            "Profile<4>",
            "native-different",
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            }),
        ),
        neutral(
            "synthetic:test:id#consumer",
            "Boss",
            "native-consumer",
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Native("native-alias".into())),
                direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal {},
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Unresolved {},
                        draft: None,
                    },
                },
                op: BooleanOp::Join,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
    ];
    let native = |id: &str, name: &str, depth: &str| crate::records::Feature {
        id: id.into(),
        parent: "history".into(),
        xml_tag: "Sketch".into(),
        tree_parent: None,
        source_id: None,
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
        features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            ..
        })
    ));
    assert_eq!(
        features[1].dependencies.as_slice(),
        vec![FeatureId::mint("synthetic:test:id#base").expect("identity grammar")]
    );
    assert!(matches!(
        features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            ..
        })
    ));
    assert!(matches!(
        features[3].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude { profile: ProfileRef::Planar(PlanarProfileRef::Sketch(id)), .. }) if id == &sketch_id
    ));
    assert_eq!(
        features[3].dependencies.as_slice(),
        vec![FeatureId::mint("synthetic:test:id#base").expect("identity grammar")]
    );
}

#[test]
fn decode_binds_multiple_sketch_history_nodes_by_exact_name() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PathRef, PlanarProfileRef};

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
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }) => Some(sketch.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bound.len(), 2);
    let sweep = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape,
                path: Some(PathRef::Sketch(path)),
                ..
            }) => match (shape.referenced_profile(),) {
                (Some(PlanarProfileRef::Sketch(profile)),) => Some((profile, path)),
                _ => None,
            },
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
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

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
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
            ..
        })
    )));
}

#[test]
fn decode_distinguishes_full_circle_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_circular_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(decoded.ir().model.sketches[0].profiles[0].len(), 1);
    assert!(matches!(
        (decoded.ir().model.sketch_entities[0].geometry).definition(),
        SketchGeometryDefinition::Circle {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            radius: actual_radius,
        } if actual_radius.get() == 1000.0
    ));
}

#[test]
fn decode_projects_full_ellipse_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

    const EPS_ELLIPSE_ANGLE: f64 = 1.0e-12;

    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_nested_elliptical_sketch(&triangle_body())),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        decoded.ir().model.sketch_entities[0].geometry.definition(),
        SketchGeometryDefinition::Ellipse {
            center: cadmpeg_ir::math::Point2 { u: 0.0, v: 0.0 },
            major_angle: value,
            major_radius: actual_major_radius,
            minor_radius: actual_minor_radius,
            bounds: None,
        } if ((value.get() - std::f64::consts::FRAC_PI_2).abs() < EPS_ELLIPSE_ANGLE) && actual_major_radius.get() == 2000.0 && actual_minor_radius.get() == 1000.0
    ));
}

#[test]
fn decode_projects_non_rational_and_rational_nurbs_sketch_geometry() {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;

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
        .filter_map(|entity| match entity.geometry.definition() {
            SketchGeometryDefinition::Nurbs { curve } => Some(curve),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(splines.len(), 2);
    assert!(splines.iter().all(|curve| {
        curve.degree() == 2
            && curve.knots() == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && curve.control_points().len() == 3
            && !curve.periodic()
    }));
    assert!(splines.iter().any(|curve| curve.weights().is_none()));
    assert!(splines
        .iter()
        .any(|curve| curve.weights() == Some(&[1.0, 0.5, 1.0])));
}
