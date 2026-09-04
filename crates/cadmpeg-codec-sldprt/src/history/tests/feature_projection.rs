// SPDX-License-Identifier: Apache-2.0
//! Feature-class, hole, plane, and profile projection tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use super::*;

#[test]
fn configuration_dependencies_participate_in_the_shared_regeneration_order() {
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![
            feature("sldprt:history:feature#0:0", None, 0),
            feature("sldprt:history:feature#0:1", None, 1),
        ],
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features = project_features(&[history]);
    let predecessor = ir.model.features[1].id.clone();
    let consumer = ir.model.features[0].id.clone();
    ir.model
        .configurations
        .push(cadmpeg_ir::features::DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId("configuration".into()),
            ordinal: 0,
            active: true,
            source_index: None,
            name: "configuration".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            bodies: cadmpeg_ir::features::ConfigurationBodies::Unresolved,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                consumer.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState {
                    suppressed: false,
                    dependencies: vec![predecessor.clone()],
                    outputs: Vec::new(),
                    definition: ir.model.features[0].definition.clone(),
                },
            )]),
            native_ref: None,
        });

    assert!(order_model_features_for_regeneration(&mut ir));
    let ordinals = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature.ordinal))
        .collect::<HashMap<_, _>>();
    assert!(ordinals[&predecessor] < ordinals[&consumer]);
    assert!(ir.model.features[0].dependencies.is_empty());
}

#[test]
fn blind_extrusion_uses_its_sole_dimension_as_depth() {
    let mut feature = feature("sldprt:history:feature#1:2", Some("12"), 2);
    feature.xml_tag = "Extrusion".into();
    feature.input_class = Some("moExtrusion_c".into());
    feature.parameters.insert("s".into(), "2.1".into());
    feature
        .properties
        .insert("EndCondition".into(), "Blind".into());

    assert!(native_parameter_is_length(&feature, "s", Some("2.1")));
    assert!(matches!(
        project_extrude(&feature, &HashMap::new(), &HashMap::new()),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(2.1)
                    },
                    ..
                }
            },
            ..
        })
    ));
}

#[test]
fn modern_extrusion_with_one_source_dimension_defaults_to_blind() {
    let mut feature = feature("sldprt:history:feature#1:2", Some("12"), 2);
    feature.xml_tag = "Extrusion".into();
    feature.input_class = Some("moExtrusion_c".into());
    feature.content = vec![FeatureContent::Dimension("m".into())];
    feature.parameters.insert("m".into(), "6.4".into());

    assert!(matches!(
        project_extrude(&feature, &HashMap::new(), &HashMap::new()),
        Some(FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: Length(6.4)
                    },
                    ..
                }
            },
            ..
        })
    ));
}

#[test]
fn legacy_history_extrusion_uses_preceding_profile_and_sole_source_depth() {
    let mut profile = feature("sldprt:history:feature#1:0", Some("9"), 0);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    let mut extrusion = feature("sldprt:history:feature#1:1", Some("20"), 1);
    extrusion.xml_tag = "Extrusion".into();
    extrusion.kind = "localized-boss-kind".into();
    extrusion.input_class = None;
    extrusion.parameters.insert("m".into(), "6.8".into());
    extrusion.parameters.insert("aux-1".into(), "1.2".into());
    extrusion.parameters.insert("aux-2".into(), "3.4".into());
    extrusion.content = vec![
        FeatureContent::Dimension("m".into()),
        FeatureContent::Dimension("m".into()),
    ];
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![extrusion, profile],
    };

    let projected = project_features(&[history]);
    let extrusion = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:1"))
        .expect("legacy extrusion feature");
    let profile = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:0"))
        .expect("legacy extrusion profile");
    assert!(profile.ordinal < extrusion.ordinal);
    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile_ref),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: Length(6.8) },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        } if profile_ref == &profile.id
    ));
}

#[test]
fn root_history_extrusion_uses_preceding_profile_without_overriding_cut() {
    let mut early_profile = feature("sldprt:history:feature#1:0", Some("9"), 0);
    early_profile.xml_tag = "Sketch".into();
    early_profile.kind = "Sketch".into();

    let mut origin_profile = feature("sldprt:history:feature#1:1", Some("18"), 1);
    origin_profile.xml_tag = "Sketch".into();
    origin_profile.kind = "Sketch".into();
    origin_profile.input_class = Some("moOriginProfileFeature_c".into());

    let mut preceding_profile = feature("sldprt:history:feature#1:2", Some("19"), 2);
    preceding_profile.xml_tag = "Sketch".into();
    preceding_profile.kind = "Sketch".into();

    let mut extrusion = feature("sldprt:history:feature#1:3", Some("20"), 3);
    extrusion.xml_tag = "Extrusion".into();
    extrusion.kind = "Cut-Extrude".into();
    extrusion.input_class = Some("moICE_c".into());
    extrusion
        .properties
        .insert("DissectableRoot".into(), "true".into());
    extrusion
        .properties
        .insert("EndCondition".into(), "Blind".into());
    extrusion.parameters.insert("D1".into(), "4.2".into());
    extrusion.content = vec![FeatureContent::Dimension("D1".into())];

    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![extrusion, early_profile, origin_profile, preceding_profile],
    };

    let projected = project_features(&[history]);
    let extrusion = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:3"))
        .expect("root extrusion feature");
    let profile = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:2"))
        .expect("preceding extrusion profile");

    assert!(matches!(
        &extrusion.definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile_ref),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: Length(4.2) },
                    ..
                }
            },
            op: BooleanOp::Cut,
            ..
        } if profile_ref == &profile.id
    ));
}

#[test]
fn repeated_dimension_content_projects_one_owned_parameter() {
    let mut feature = feature("sldprt:history:feature#1:2", None, 2);
    feature.parameters.insert("D1".into(), "2".into());
    feature.content = vec![
        FeatureContent::Dimension("D1".into()),
        FeatureContent::Dimension("D1".into()),
    ];

    assert_eq!(parameter_names(&feature), vec!["D1", "D1"]);
    assert_eq!(projected_parameter_names(&feature), vec!["D1"]);
    assert_eq!(
        project_feature_content(&feature, &HashMap::new()),
        vec![FeatureSourceContent::Parameter(ParameterId(
            "sldprt:model:parameter#1:2:0".into()
        ))]
    );
}

#[test]
fn spatial_profile_class_projects_a_spatial_sketch() {
    let mut spatial = feature("spatial", Some("7"), 0);
    spatial.xml_tag = "Sketch".into();
    spatial.kind = "Sketch".into();
    spatial.input_class = Some("mo3DProfileFeature_c".into());

    assert_eq!(
        project_definition(
            &spatial,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&spatial),
        ),
        FeatureDefinition::SpatialSketch { sketch: None }
    );
}

#[test]
fn base_body_class_projects_stored_geometry_independently_of_display_name() {
    let mut base_body = feature("base-body", Some("18"), 0);
    base_body.kind = "Localized imported body".into();
    base_body.input_class = Some("moBaseBody_c".into());

    assert_eq!(
        project_definition(
            &base_body,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&base_body),
        ),
        FeatureDefinition::StoredGeometry
    );
}

#[test]
fn hole_profile_dimension_order_distinguishes_counterbore_and_thread() {
    let profile = |roles: &[(&str, &str)]| {
        let mut profile = feature("profile", Some("7"), 0);
        profile.kind = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        for (name, expression) in roles {
            profile
                .parameters
                .insert((*name).into(), (*expression).into());
            profile
                .content
                .push(FeatureContent::Dimension((*name).into()));
        }
        profile
    };

    let counterbore = profile(&[
        ("a", "118°"),
        ("b", "5.7"),
        ("c", "<MOD-DIAM>9"),
        ("d", "12"),
        ("e", "<MOD-DIAM>5.5"),
    ]);
    let construction = hole_sketch_construction(&counterbore).expect("required invariant");
    assert_eq!(construction.diameter, Length(5.5));
    assert_eq!(construction.depth, Some(Length(12.0)));
    assert!(matches!(
        construction.kind,
        HoleKind::CounterboreDrilled {
            diameter: Length(9.0),
            depth: Length(5.7),
            ..
        }
    ));

    let threaded = profile(&[
        ("a", "<MOD-DIAM>4.2"),
        ("b", "12.4"),
        ("c", "<MOD-DIAM>5"),
        ("d", "10"),
        ("e", "118°"),
    ]);
    let construction = hole_sketch_construction(&threaded).expect("required invariant");
    assert_eq!(construction.diameter, Length(4.2));
    assert_eq!(construction.depth, Some(Length(12.4)));
    assert!(matches!(
        construction.kind,
        HoleKind::Threaded {
            major_diameter: Length(5.0),
            thread_depth: Length(10.0),
            pitch: None,
            ..
        }
    ));

    let tapered_thread = profile(&[
        ("a", "3.43°"),
        ("b", "6.92"),
        ("c", "118°"),
        ("d", "<MOD-DIAM>8.43"),
        ("e", "11.62"),
        ("f", "<MOD-DIAM>10.29"),
    ]);
    let construction = hole_sketch_construction(&tapered_thread).expect("tapered thread profile");
    assert_eq!(construction.diameter, Length(8.43));
    assert_eq!(construction.depth, Some(Length(11.62)));
    assert!(matches!(
        construction.kind,
        HoleKind::Threaded {
            major_diameter: Length(10.29),
            thread_depth: Length(6.92),
            pitch: None,
            drill_point_angle: Angle(angle),
        } if (angle - 118_f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: Angle(118_f64.to_radians()),
            depth_to_tip: false,
        })
    );
    assert_eq!(construction.taper_angle, Some(Angle(3.43_f64.to_radians())));

    let counterbore_with_exit_countersink = profile(&[
        ("a", "4.6"),
        ("b", "<MOD-DIAM>8"),
        ("c", "90°"),
        ("d", "10"),
        ("e", "<MOD-DIAM>4.5"),
        ("f", "<MOD-DIAM>4.55"),
    ]);
    let construction =
        hole_sketch_construction(&counterbore_with_exit_countersink).expect("dual-ended profile");
    assert_eq!(construction.diameter, Length(4.5));
    assert_eq!(construction.depth, Some(Length(10.0)));
    assert_eq!(
        construction.kind,
        HoleKind::Counterbore {
            diameter: Length(8.0),
            depth: Length(4.6),
        }
    );
    assert_eq!(
        construction.exit_kind,
        Some(HoleKind::Countersink {
            diameter: Length(4.55),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        })
    );

    let counterdrill = profile(&[
        ("a", "12.4"),
        ("b", "<MOD-DIAM>5.5"),
        ("c", "118°"),
        ("d", "<MOD-DIAM>10.05"),
        ("e", "90°"),
        ("f", "5.4"),
        ("g", "<MOD-DIAM>9.95"),
    ]);
    let construction = hole_sketch_construction(&counterdrill).expect("counterdrill profile");
    assert_eq!(construction.diameter, Length(5.5));
    assert_eq!(construction.depth, Some(Length(12.4)));
    assert_eq!(
        construction.kind,
        HoleKind::Counterdrill {
            diameter: Length(9.95),
            entry_diameter: Some(Length(10.05)),
            depth: Length(5.4),
            angle: Angle(std::f64::consts::FRAC_PI_2),
        }
    );
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: Angle(118_f64.to_radians()),
            depth_to_tip: false,
        })
    );

    let placement_dimensions = profile(&[
        ("a", "<MOD-DIAM>9"),
        ("b", "6"),
        ("c", "4"),
        ("d", "4"),
        ("e", "6"),
    ]);
    assert!(hole_sketch_construction(&placement_dimensions).is_none());

    let unsupported_countersink = profile(&[
        ("diameter", "<MOD-DIAM>5"),
        ("entry", "<MOD-DIAM>9"),
        ("depth", "6"),
        ("angle", "82°"),
    ]);
    assert!(hole_sketch_construction(&unsupported_countersink).is_none());

    let unsupported_counterbore = profile(&[
        ("diameter", "<MOD-DIAM>5"),
        ("entry", "<MOD-DIAM>9"),
        ("entry depth", "3"),
        ("depth", "6"),
    ]);
    assert!(hole_sketch_construction(&unsupported_counterbore).is_none());

    let mut native_profile = profile(&[("diameter", "<MOD-DIAM>6.6"), ("depth", "9.4")]);
    native_profile.id = "native-profile".into();
    native_profile.source_id = None;
    let mut native_owned = feature("native-owned-hole", None, 0);
    native_owned
        .properties
        .insert("DissectableChildren".into(), native_profile.id.clone());
    let projected = project_hole(
        &native_owned,
        &HashMap::new(),
        &[native_owned.clone(), native_profile],
    );
    assert!(matches!(
        projected,
        FeatureDefinition::Hole {
            diameter: Some(Length(6.6)),
            extent: Some(LinearTermination::Blind {
                length: Length(9.4)
            }),
            ..
        }
    ));

    let mut canonical = feature("hole", Some("8"), 0);
    canonical.parameters = [
        ("Diameter".into(), "4.2mm".into()),
        ("Depth".into(), "12.4mm".into()),
        ("ThreadMajorDiameter".into(), "5mm".into()),
        ("ThreadDepth".into(), "10mm".into()),
        ("DrillPointAngle".into(), "118°".into()),
    ]
    .into();
    let projected = project_hole(
        &canonical,
        &HashMap::new(),
        std::slice::from_ref(&canonical),
    );
    let FeatureDefinition::Hole {
        kind:
            HoleKind::Threaded {
                major_diameter,
                thread_depth,
                ..
            },
        diameter: Some(diameter),
        extent: Some(LinearTermination::Blind { length }),
        ..
    } = projected
    else {
        panic!("expected canonical threaded hole: {projected:?}");
    };
    assert!((diameter.0 - 4.2).abs() < 1.0e-12);
    assert!((major_diameter.0 - 5.0).abs() < 1.0e-12);
    assert!((thread_depth.0 - 10.0).abs() < 1.0e-12);
    assert!((length.0 - 12.4).abs() < 1.0e-12);
}

#[test]
fn scene_class_binds_only_its_explicit_source_identifier() {
    let mut first = feature("first", Some("153"), 0);
    first.kind = "localized light".into();
    let mut second = feature("second", Some("155"), 1);
    second.kind = first.kind.clone();
    let mut singleton = feature("singleton", Some("200"), 2);
    singleton.kind = "unrelated".into();
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second, singleton],
    }];
    let scene = crate::tessellation::SceneFeatureClasses {
        by_source: HashMap::from([("153".into(), "moDirectionLight_c".into())]),
    };

    enrich_scene_classes(&mut histories, &scene);

    assert_eq!(
        histories[0].features[0].input_class.as_deref(),
        Some("moDirectionLight_c")
    );
    assert_eq!(histories[0].features[1].input_class, None);
    assert_eq!(histories[0].features[2].input_class, None);
}

#[test]
fn structurally_stable_feature_manager_nodes_use_source_identity() {
    let roster = |node: &Feature| {
        let mut roster = vec![node.clone()];
        for (source, class) in [
            ("7", "moDocsFolder_c"),
            ("8", "moCommentsFolder_c"),
            ("9", "moSolidBodyFolder_c"),
            ("10", "moSurfaceBodyFolder_c"),
        ] {
            let mut sentinel = feature("sentinel", Some(source), roster.len() as u32);
            sentinel.input_class = Some(class.into());
            roster.push(sentinel);
        }
        roster
    };
    let cases = [
        ("1", FeatureTreeNodeRole::Annotations),
        ("5", FeatureTreeNodeRole::ModelOrigin),
        ("6", FeatureTreeNodeRole::LightsAndCameras),
        ("12", FeatureTreeNodeRole::AmbientLight),
        ("13", FeatureTreeNodeRole::DirectionalLight),
        ("14", FeatureTreeNodeRole::DirectionalLight),
        ("15", FeatureTreeNodeRole::DirectionalLight),
    ];

    for (source_id, expected) in cases {
        let mut node = feature("node", Some(source_id), 0);
        node.kind = "任意本地化標籤".into();
        if source_id == "5" {
            node.xml_tag = "Sketch".into();
        }
        assert_eq!(
            feature_tree_node_role(&node, &roster(&node)),
            Some(expected)
        );
    }

    let mut fourth_light = feature("fourth", Some("70"), 0);
    fourth_light.kind = "本地化方向光".into();
    let mut directional_roster = roster(&fourth_light);
    let mut first_light = feature("light", Some("13"), 13);
    first_light.kind = fourth_light.kind.clone();
    directional_roster.push(first_light);
    assert_eq!(
        feature_tree_node_role(&fourth_light, &directional_roster),
        Some(FeatureTreeNodeRole::DirectionalLight)
    );

    let mut additional_ambient = feature("additional ambient", Some("16"), 0);
    additional_ambient.kind = "本地化环境光".into();
    let mut ambient_roster = roster(&additional_ambient);
    let mut reserved_ambient = feature("ambient", Some("12"), 12);
    reserved_ambient.kind = additional_ambient.kind.clone();
    ambient_roster.push(reserved_ambient);
    assert_eq!(
        feature_tree_node_role(&additional_ambient, &ambient_roster),
        Some(FeatureTreeNodeRole::AmbientLight)
    );

    let legacy_roster = |node: &Feature| {
        let mut roster = vec![node.clone()];
        for (source, class) in [
            ("6", "moOriginProfileFeature_c"),
            ("9", "moSurfaceBodyFolder_c"),
            ("10", "moSolidBodyFolder_c"),
            ("12", "moDocsFolder_c"),
            ("13", "moCommentsFolder_c"),
        ] {
            let mut sentinel = feature("sentinel", Some(source), roster.len() as u32);
            sentinel.input_class = Some(class.into());
            roster.push(sentinel);
        }
        roster
    };
    for (source, expected) in [
        ("2", FeatureTreeNodeRole::LightsAndCameras),
        ("7", FeatureTreeNodeRole::AmbientLight),
        ("8", FeatureTreeNodeRole::DirectionalLight),
    ] {
        let node = feature("legacy", Some(source), 0);
        assert_eq!(
            feature_tree_node_role(&node, &legacy_roster(&node)),
            Some(expected)
        );
    }
    let legacy_lights = feature("legacy lights", Some("2"), 0);
    let mut complete_legacy_roster = legacy_roster(&legacy_lights);
    for (source, class) in [
        ("1", "moDetailCabinet_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moRefPlane_c"),
    ] {
        let mut sentinel = feature(
            "legacy frame",
            Some(source),
            complete_legacy_roster.len() as u32,
        );
        sentinel.input_class = Some(class.into());
        complete_legacy_roster.push(sentinel);
    }
    for source in ["7", "8"] {
        complete_legacy_roster.push(feature(
            "legacy light",
            Some(source),
            complete_legacy_roster.len() as u32,
        ));
    }
    assert_eq!(
        feature_tree_node_role(&legacy_lights, &complete_legacy_roster),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let roster_from = |node: &Feature, classes: &[(&str, &str)], classless_sources: &[&str]| {
        let mut features = vec![node.clone()];
        for (source, class) in classes {
            let mut sentinel = feature("sentinel", Some(source), features.len() as u32);
            sentinel.input_class = Some((*class).into());
            features.push(sentinel);
        }
        for source in classless_sources {
            features.push(feature("reserved", Some(source), features.len() as u32));
        }
        features
    };
    let default_frame = [
        ("1", "moDetailCabinet_c"),
        ("2", "moRefPlane_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moOriginProfileFeature_c"),
    ];
    let lights = feature("lights", Some("6"), 0);
    assert_eq!(
        feature_tree_node_role(&lights, &roster_from(&lights, &default_frame, &["7", "8"])),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let ambient = feature("ambient", Some("10"), 0);
    let mut folders_at_seven = default_frame.to_vec();
    folders_at_seven.extend([("7", "moSolidBodyFolder_c"), ("8", "moSurfaceBodyFolder_c")]);
    assert_eq!(
        feature_tree_node_role(
            &ambient,
            &roster_from(&ambient, &folders_at_seven, &["6", "11", "12"]),
        ),
        Some(FeatureTreeNodeRole::AmbientLight)
    );

    let early_lights = feature("lights", Some("2"), 0);
    let origin_at_six = [
        ("1", "moDetailCabinet_c"),
        ("3", "moRefPlane_c"),
        ("4", "moRefPlane_c"),
        ("5", "moRefPlane_c"),
        ("6", "moOriginProfileFeature_c"),
    ];
    assert_eq!(
        feature_tree_node_role(
            &early_lights,
            &roster_from(&early_lights, &origin_at_six, &["7", "8"]),
        ),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let ambiguous = feature("node", Some("99"), 0);
    assert_eq!(feature_tree_node_role(&ambiguous, &[]), None);

    let mut exploded_views = ambiguous.clone();
    exploded_views.name.clear();
    assert_eq!(
        feature_tree_node_role(&exploded_views, &roster(&exploded_views)),
        Some(FeatureTreeNodeRole::ExplodedViews)
    );

    let mut reference_plane = feature("node", Some("5"), 0);
    reference_plane.input_class = Some("moRefPlane_c".into());
    assert_eq!(feature_tree_node_role(&reference_plane, &[]), None);

    let mut sheet_metal = feature("node", Some("-1"), 0);
    sheet_metal.name.clear();
    assert_eq!(
        feature_tree_node_role(&sheet_metal, &roster(&sheet_metal)),
        Some(FeatureTreeNodeRole::SheetMetal)
    );
    sheet_metal.name = "任意本地化鈑金根節點".into();
    assert_eq!(
        feature_tree_node_role(&sheet_metal, &roster(&sheet_metal)),
        Some(FeatureTreeNodeRole::SheetMetal)
    );
    assert_eq!(feature_tree_node_role(&sheet_metal, &[]), None);
}

#[test]
fn sketch_block_instances_bind_to_adjacent_typed_definition_objects() {
    let mut instance = feature("instance", Some("25"), 1);
    instance.input_class = Some("moSketchBlockInst_c".into());
    let mut compact_instance = feature("compact instance", Some("34"), 2);
    compact_instance.input_class = Some("moSketchBlockInst_c".into());
    let mut definition = feature("definition", Some("23"), 0);
    definition.input_class = Some("moSketchBlockDef_c".into());
    let mut histories = vec![FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition, instance, compact_instance],
    }];
    let mut lane = feature_input_lane("lane", None);
    lane.native_payload.resize(500, 0);
    let write_local_id = |payload: &mut [u8], offset: usize, token: [u8; 4], local_id: u16| {
        payload[offset..offset + 4].copy_from_slice(&[0xff; 4]);
        payload[offset + 4..offset + 8].copy_from_slice(&token);
        payload[offset + 12..offset + 18].copy_from_slice(&[0x02, 0, 0, 0, 0, 0]);
        payload[offset + 18..offset + 20].copy_from_slice(&local_id.to_le_bytes());
        payload[offset + 40..offset + 44].copy_from_slice(&[0, 0, 1, 0]);
    };
    write_local_id(&mut lane.native_payload, 180, [0x11, 0x22, 0x33, 0x01], 0);
    write_local_id(
        &mut lane.native_payload,
        250,
        [0x11, 0x22, 0x33, 0x01],
        0x0115,
    );
    lane.native_payload[294..296].copy_from_slice(&[0x26, 0x81]);
    for (index, value) in [0.00575_f64, -0.169, 0.0].into_iter().enumerate() {
        let start = 296 + index * 8;
        lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    lane.native_payload[388..390].copy_from_slice(&0x0115_u16.to_le_bytes());
    write_local_id(
        &mut lane.native_payload,
        420,
        [0x44, 0x55, 0x66, 0x01],
        0x0115,
    );
    lane.native_payload[464..466].copy_from_slice(&[0x73, 0x81]);
    for (index, value) in [0.01075_f64, -0.132, 0.0].into_iter().enumerate() {
        let start = 466 + index * 8;
        lane.native_payload[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    lane.names = vec![
        crate::records::FeatureInputName {
            id: "instance-name".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 100,
            object_id: Some(25),
            value: "instance".into(),
        },
        crate::records::FeatureInputName {
            id: "definition-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 140,
            object_id: Some(23),
            value: "definition".into(),
        },
        crate::records::FeatureInputName {
            id: "compact-instance-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 340,
            object_id: Some(34),
            value: "compact".into(),
        },
    ];

    crate::resolved_features::reference_geometry::enrich_history_sketch_block_references(
        &mut histories,
        &[lane],
    );

    assert_eq!(
        histories[0].features[1]
            .properties
            .get("BlockDefinition")
            .map(String::as_str),
        Some("23")
    );
    assert_eq!(
        histories[0].features[1]
            .properties
            .get("BlockOrigin")
            .map(String::as_str),
        Some("5.75mm,-169mm,0mm")
    );
    assert_eq!(
        histories[0].features[2]
            .properties
            .get("BlockOrigin")
            .map(String::as_str),
        Some("10.75mm,-132mm,0mm")
    );
    assert_eq!(
        histories[0].features[2]
            .properties
            .get("BlockDefinition")
            .map(String::as_str),
        Some("23")
    );
}

#[test]
fn principal_plane_requires_the_reference_plane_native_class() {
    let mut plane = feature("plane", Some("2"), 0);
    assert_eq!(crate::classification::principal_plane(&plane), None);
    plane.input_class = Some("moRefPlane_c".into());
    assert_eq!(
        crate::classification::principal_plane(&plane),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );
}

#[test]
fn shifted_reserved_triplet_does_not_classify_principal_planes() {
    let mut scene = feature("scene", Some("2"), 0);
    let mut front = feature("front", Some("3"), 1);
    let mut top = feature("top", Some("4"), 2);
    let mut right = feature("right", Some("5"), 3);
    for plane in [&mut front, &mut top, &mut right] {
        plane.input_class = Some("moRefPlane_c".into());
    }
    scene.input_class = Some("moSceneFolder_c".into());
    let features = [scene, front.clone(), top.clone(), right.clone()];
    let by_source = features
        .iter()
        .filter_map(|feature| Some((feature.source_id.as_deref()?, feature)))
        .collect::<HashMap<_, _>>();

    assert_eq!(
        principal_plane_in_history(&front, &by_source, &features),
        None
    );
    assert_eq!(
        principal_plane_in_history(&top, &by_source, &features),
        None
    );
    assert_eq!(
        principal_plane_in_history(&right, &by_source, &features),
        None
    );
}

#[test]
fn angular_plane_parameter_does_not_claim_offset_semantics() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());
    plane.parameters.insert("D1".into(), "0rad".into());
    plane
        .properties
        .insert("Origin".into(), "0mm,70mm,0mm".into());
    plane.properties.insert("Normal".into(), "0,1,0".into());
    plane.properties.insert("UAxis".into(), "-1,0,0".into());

    assert!(!is_offset_plane(&plane));
    assert_eq!(
        project_definition(
            &plane,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&plane),
        ),
        FeatureDefinition::DatumPlane {
            origin: Point3::new(0.0, 70.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(-1.0, 0.0, 0.0),
        }
    );
}

#[test]
fn length_plane_parameter_claims_offset_semantics() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());
    plane.parameters.insert("D1".into(), "70mm".into());

    assert!(is_offset_plane(&plane));
    assert_eq!(
        project_definition(
            &plane,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&plane),
        ),
        FeatureDefinition::DatumOffsetPlane {
            reference: None,
            distance: Length(70.0),
        }
    );
}

#[test]
fn frameless_reference_plane_remains_typed_unresolved() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());

    assert_eq!(
        project_definition(
            &plane,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            std::slice::from_ref(&plane),
        ),
        FeatureDefinition::DatumPlaneUnresolved
    );
}

#[test]
fn legacy_principal_plane_requires_a_complete_matching_triplet() {
    let front = feature("front", Some("2"), 0);
    let top = feature("top", Some("3"), 1);
    let right = feature("right", Some("4"), 2);
    let features = [&front, &top, &right]
        .into_iter()
        .map(|feature| {
            (
                feature.source_id.as_deref().expect("required invariant"),
                feature,
            )
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(
        principal_plane_in_history(&front, &features, &[]),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );

    let mut mismatched = right.clone();
    mismatched.kind = "Different".into();
    let features = [&front, &top, &mismatched]
        .into_iter()
        .map(|feature| {
            (
                feature.source_id.as_deref().expect("required invariant"),
                feature,
            )
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(principal_plane_in_history(&front, &features, &[]), None);
}

#[test]
fn idless_legacy_principal_planes_require_an_exact_bounded_triplet() {
    let front = feature("front", None, 10);
    let top = feature("top", None, 11);
    let right = feature("right", None, 12);
    let mut successor = feature("origin", None, 13);
    successor.kind = "Other".into();
    let records = [front.clone(), top.clone(), right.clone(), successor.clone()];

    assert_eq!(
        principal_plane_in_history(&front, &HashMap::new(), &records),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );

    let mut unbounded = records.clone();
    unbounded[3].kind = unbounded[0].kind.clone();
    assert_eq!(
        principal_plane_in_history(&front, &HashMap::new(), &unbounded),
        None
    );

    let second_front = feature("front-2", None, 20);
    let second_top = feature("top-2", None, 21);
    let second_right = feature("right-2", None, 22);
    let mut second_successor = feature("origin-2", None, 23);
    second_successor.kind = "Other".into();
    let ambiguous = [
        front,
        top,
        right,
        successor,
        second_front,
        second_top,
        second_right,
        second_successor,
    ];
    assert_eq!(
        principal_plane_in_history(&ambiguous[0], &HashMap::new(), &ambiguous),
        None
    );
}

#[test]
fn custom_properties_are_document_attributes_not_model_features() {
    let mut property = feature("property", None, 0);
    property.xml_tag = "CustomProperty".into();
    property.name = "PartNumber".into();
    property.text = Some("A-123".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![property],
    };

    assert!(project_features(std::slice::from_ref(&history)).is_empty());
    let attributes = custom_property_attributes(std::slice::from_ref(&history));
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].name, "PartNumber");
    assert_eq!(
        attributes[0].values,
        vec![AttributeValue::String("A-123".into())]
    );

    let mut native = Some(crate::native::SldprtNative {
        version: crate::native::SLDPRT_NATIVE_VERSION,
        feature_histories: vec![history],
        feature_input_lanes: Vec::new(),
        pmi_dimensions: Vec::new(),
    });
    sync_neutral_features(&[], &[], &[], &mut native).expect("required invariant");
    assert_eq!(
        native.expect("required invariant").feature_histories[0]
            .features
            .len(),
        1
    );
}

#[test]
fn native_attribute_records_are_metadata_not_model_features() {
    let mut definition = feature("definition", Some("-1"), 0);
    definition.name = "VendorSettings.1".into();
    definition
        .parameters
        .insert("VendorSettings.1".into(), "0".into());
    let mut attribute = feature("attribute", Some("27"), 1);
    attribute.name = "VendorSettings.14236".into();
    attribute.input_class = Some("moAttribute_c".into());
    let mut comments = feature("comments", Some("28"), 2);
    comments.input_class = Some("moConfigCommentsFolder_c".into());
    let mut alignment = feature("alignment", Some("29"), 3);
    alignment.input_class = Some("moAlignGroup_c".into());
    let mut model = feature("model", Some("30"), 4);
    model.xml_tag = "Sketch".into();
    model.kind = "Sketch".into();
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition, attribute, comments, alignment, model],
    };

    let projected = project_features(std::slice::from_ref(&history));
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].native_ref.as_deref(), Some("model"));
    assert!(project_parameters(&[history]).is_empty());
}

#[test]
fn native_attribute_definition_type_is_metadata_without_an_instance_name_match() {
    let mut definition = feature("definition", Some("-1"), 0);
    definition.kind = "Attribute-Definition".into();
    definition.name = "NativeAttributeFamily".into();
    definition
        .parameters
        .insert("NativeAttributeFamily".into(), "0".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition],
    };

    assert!(project_features(std::slice::from_ref(&history)).is_empty());
    assert!(project_parameters(&[history]).is_empty());
}

#[test]
fn configuration_snapshots_preserve_base_tree_node_roles() {
    let light = feature("light", Some("30"), 0);
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![light],
    };
    let mut configured = project_features(std::slice::from_ref(&history));
    assert!(matches!(
        configured[0].definition,
        FeatureDefinition::Native { .. }
    ));
    let mut base = configured.clone();
    base[0].definition = FeatureDefinition::TreeNode {
        role: FeatureTreeNodeRole::DirectionalLight,
        children: Vec::new(),
        active_child: None,
    };

    restore_configuration_tree_node_definitions(&mut configured, &base);
    assert!(matches!(
        configured[0].definition,
        FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::DirectionalLight,
            ..
        }
    ));
}

#[test]
fn simple_hole_uses_its_profile_dimension_roles() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties
        .insert("DissectableChildren".into(), "213,212".into());
    let mut position = feature("position", Some("213"), 1);
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    let mut profile = feature("profile", Some("212"), 1);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile
        .parameters
        .insert("localized diameter".into(), "<MOD-DIAM>4.5".into());
    profile
        .parameters
        .insert("localized depth".into(), "13.2".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, position, profile],
    };

    let projected = project_features(std::slice::from_ref(&history));
    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = &projected[0].definition
    else {
        panic!("expected a hole definition");
    };
    assert_eq!(*diameter, Some(Length(4.5)));
    assert_eq!(
        *extent,
        Some(LinearTermination::Blind {
            length: Length(13.2)
        })
    );

    let mut ambiguous = history;
    ambiguous.features[2]
        .parameters
        .insert("another length".into(), "2".into());
    let ambiguous = project_features(&[ambiguous]);
    let FeatureDefinition::Hole {
        diameter, extent, ..
    } = &ambiguous[0].definition
    else {
        panic!("expected a hole definition");
    };
    assert_eq!(*diameter, None);
    assert_eq!(*extent, None);
}

#[test]
fn hole_wizard_rejects_unsupported_countersink_child_schema() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties
        .insert("DissectableChildren".into(), "213,212".into());
    let mut position = feature("position", Some("213"), 1);
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.parameters.insert("D1".into(), "11".into());
    let mut profile = feature("profile", Some("212"), 2);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile
        .parameters
        .insert("localized bore".into(), "<MOD-DIAM>3.4".into());
    profile
        .parameters
        .insert("localized depth".into(), "3".into());
    profile
        .parameters
        .insert("localized entry".into(), "<MOD-DIAM>6.6".into());
    profile
        .parameters
        .insert("localized angle".into(), "90°".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, position, profile],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::Simple,
            diameter: None,
            extent: None,
            ..
        }
    ));
}

#[test]
fn hole_wizard_drill_point_profile_retains_bore_and_blind_depth() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties
        .insert("DissectableChildren".into(), "212".into());
    let mut profile = feature("profile", Some("212"), 1);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile
        .parameters
        .insert("螺纹孔钻头直径".into(), "<MOD-DIAM>4.2".into());
    profile
        .parameters
        .insert("螺纹孔钻头深度".into(), "10".into());
    profile.parameters.insert("导头角度".into(), "118°".into());
    profile.content.extend([
        FeatureContent::Dimension("导头角度".into()),
        FeatureContent::Dimension("螺纹孔钻头深度".into()),
        FeatureContent::Dimension("螺纹孔钻头直径".into()),
    ]);
    profile
        .parameters
        .insert("derived native scalar".into(), "937.25".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, profile],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Hole {
            kind: HoleKind::SimpleDrilled {
                drill_point_angle: Angle(drill_point_angle),
            },
            diameter: Some(Length(4.2)),
            extent: Some(LinearTermination::Blind {
                length: Length(10.0),
            }),
            ..
        } if (drill_point_angle - 118.0_f64.to_radians()).abs() < 1.0e-12
    ));
}

#[test]
fn native_scalar_refresh_preserves_radial_dimension_semantics() {
    let profile = feature("profile", Some("212"), 1);

    assert_eq!(
        format_native_scalar(&profile, "bore", 0.0042, Some("<MOD-DIAM>4.2")),
        "<MOD-DIAM>4.2"
    );
    assert_eq!(
        format_native_scalar(&profile, "radius", 0.003, Some("&lt;MOD-RHO&gt;3")),
        "&lt;MOD-RHO&gt;3"
    );
}

#[test]
fn legacy_revolve_uses_d1_angle_and_cut_class_operation() {
    let mut revolve = feature("revolve", Some("42"), 0);
    revolve.input_class = Some("moRevCut_c".into());
    revolve.parameters.insert("D1".into(), "360°".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![revolve],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: Angle(value) }
                }),
                ..
            },
            op: BooleanOp::Cut,
        } if (value - std::f64::consts::TAU).abs() < 1.0e-12
    ));
}

#[test]
fn localized_cut_extrusion_uses_its_native_class_operation() {
    let mut cut = feature("cut", Some("43"), 0);
    cut.kind = "BossExtrude".into();
    cut.input_class = Some("moCut_c".into());
    cut.parameters.insert("D1".into(), "45".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![cut],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Extrude {
            op: BooleanOp::Cut,
            ..
        }
    ));
}

#[test]
fn revolve_uses_its_ordered_angle_dimension_name() {
    let mut revolve = feature("revolve", Some("42"), 0);
    revolve.input_class = Some("moRevolution_c".into());
    revolve.parameters.insert("FIX_1".into(), "360°".into());
    revolve
        .content
        .push(FeatureContent::Dimension("FIX_1".into()));
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![revolve],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Revolve {
            construction: RevolutionConstruction {
                extent: Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: Angle(value) }
                }),
                ..
            },
            ..
        } if (value - std::f64::consts::TAU).abs() < 1.0e-12
    ));
}

#[test]
fn chamfer_uses_physical_types_of_ordered_localized_dimensions() {
    let mut chamfer = feature("chamfer", Some("42"), 0);
    chamfer.input_class = Some("Chamfer_c".into());
    chamfer
        .parameters
        .insert("localized length".into(), "1.5".into());
    chamfer
        .parameters
        .insert("localized angle".into(), "45°".into());
    chamfer
        .content
        .push(FeatureContent::Dimension("localized length".into()));
    chamfer
        .content
        .push(FeatureContent::Dimension("localized angle".into()));
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![chamfer],
    };

    let projected = project_features(&[history]);
    assert!(matches!(
        projected[0].definition,
        FeatureDefinition::Chamfer { ref groups, .. }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::ChamferGroup {
                    spec: ChamferSpec::DistanceAngle {
                        distance: Length(1.5),
                        angle: Angle(value),
                    },
                    ..
                }] if (*value - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12
            )
    ));

    let mut distance = feature("distance", Some("43"), 0);
    distance.input_class = Some("Chamfer_c".into());
    distance
        .parameters
        .insert("localized distance".into(), "2mm".into());
    distance
        .content
        .push(FeatureContent::Dimension("localized distance".into()));
    assert!(matches!(
        project_chamfer(&distance),
        FeatureDefinition::Chamfer { ref groups, .. }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::ChamferGroup {
                    spec: ChamferSpec::Distance {
                        distance: Length(2.0),
                    },
                    ..
                }]
            )
    ));

    distance
        .parameters
        .insert("localized second distance".into(), "3mm".into());
    distance.content.push(FeatureContent::Dimension(
        "localized second distance".into(),
    ));
    assert!(matches!(
        project_chamfer(&distance),
        FeatureDefinition::Chamfer { ref groups, .. }
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::ChamferGroup {
                    spec: ChamferSpec::TwoDistances {
                        first: Length(2.0),
                        second: Length(3.0),
                    },
                    ..
                }]
            )
    ));
}

#[test]
fn cosmetic_thread_retains_nominal_diameter_and_blind_length() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread.parameters.insert("D1".into(), "16".into());
    thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected = project_features(&[history]);
    assert_eq!(
        projected[0].definition,
        FeatureDefinition::CosmeticThread {
            face: FaceSelection::Unresolved,
            diameter: Some(Length(8.0)),
            extent: Some(CosmeticThreadExtent::Blind {
                length: Length(16.0),
            }),
        }
    );
}

#[test]
fn cosmetic_thread_without_blind_length_is_through() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread.parameters.insert("D2".into(), "<MOD-DIAM>8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected = project_features(&[history]);
    assert_eq!(
        projected[0].definition,
        FeatureDefinition::CosmeticThread {
            face: FaceSelection::Unresolved,
            diameter: Some(Length(8.0)),
            extent: Some(CosmeticThreadExtent::Through),
        }
    );
}

#[test]
fn cosmetic_thread_non_length_d1_and_named_diameter_are_through() {
    for d1 in ["0", "6.2831853071796rad"] {
        let mut thread = feature("thread", Some("42"), 0);
        thread.input_class = Some("moCosmeticThread_c".into());
        thread.parameters.insert("D1".into(), d1.into());
        thread
            .parameters
            .insert("thread size".into(), "<MOD-DIAM>4.9".into());
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![thread],
        };

        let projected = project_features(&[history]);
        assert_eq!(
            projected[0].definition,
            FeatureDefinition::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: Some(Length(4.9)),
                extent: Some(CosmeticThreadExtent::Through),
            }
        );
    }
}

#[test]
fn cosmetic_thread_requires_one_named_diameter() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread
        .parameters
        .insert("major".into(), "<MOD-DIAM>8".into());
    thread
        .parameters
        .insert("minor".into(), "<MOD-DIAM>6.8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected = project_features(&[history]);
    let FeatureDefinition::CosmeticThread { diameter, .. } = &projected[0].definition else {
        panic!("expected a cosmetic thread");
    };
    assert_eq!(*diameter, None);
}

#[test]
fn cosmetic_thread_inherits_one_threaded_hole_major_diameter() {
    let mut hole = feature("hole", Some("10"), 0);
    hole.input_class = Some("moHoleWzd_c".into());
    hole.properties
        .insert("DissectableChildren".into(), "11".into());

    let mut profile = feature("profile", Some("11"), 1);
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile.parameters = [
        ("bore".into(), "<MOD-DIAM>2.5".into()),
        ("drill depth".into(), "7.5".into()),
        ("major".into(), "<MOD-DIAM>3".into()),
        ("thread depth".into(), "6".into()),
        ("angle".into(), "118°".into()),
    ]
    .into();
    profile.content = ["bore", "drill depth", "major", "thread depth", "angle"]
        .into_iter()
        .map(|name| FeatureContent::Dimension(name.into()))
        .collect();

    let mut thread = feature("thread", Some("12"), 2);
    thread.input_class = Some("moCosmeticThread_c".into());
    let thread_id = thread.id.clone();
    let hole_id = hole.id.clone();
    let mut history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, profile, thread],
    };
    let mut lane = feature_input_lane("lane", None);
    lane.surface_selections
        .push(crate::records::FeatureInputSurfaceSelection {
            id: "selection".into(),
            parent: lane.id.clone(),
            ordinal: 0,
            offset: 0,
            selector: 0,
            endpoint_selector: None,
            object_name_ref: "thread-name".into(),
            feature_ref: thread_id,
            producer_feature_refs: vec![hole_id.clone()],
            terminal_feature_ref: Some(hole_id),
            components: Vec::new(),
        });

    crate::resolved_features::holes::enrich_history_cosmetic_thread_diameters(
        std::slice::from_mut(&mut history),
        &[lane],
    );
    assert_eq!(
        history.features[2].parameters.get("D2"),
        Some(&"<MOD-DIAM>3mm".to_string())
    );
}

#[test]
fn profile_consumers_require_a_regeneration_profile() {
    let mut definition = FeatureDefinition::Extrude {
        profile: ProfileRef::Native("sketch-native".into()),
        direction: cadmpeg_ir::features::ExtrudeDirection::ProfileNormal,
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Unresolved,
                draft: None,
                offset: None,
            },
        },
        op: BooleanOp::Unresolved,
        direction_source: None,
        solid: None,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    };
    let sketch = cadmpeg_ir::sketches::SketchId("sketch".into());

    assert!(!bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &FeatureId("sketch-feature".into()),
        &sketch,
        false,
    ));
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native(_),
            ..
        }
    ));
    assert!(bind_definition_sketch(
        &mut definition,
        "sketch-native",
        &FeatureId("sketch-feature".into()),
        &sketch,
        true,
    ));
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Sketch(ref bound),
            ..
        } if bound == &sketch
    ));
}

#[test]
fn exact_native_profile_source_projects_a_feature_dependency() {
    let mut sketch = feature("sketch", Some("42"), 0);
    sketch.kind = "Sketch".into();
    sketch.input_class = Some("moProfileFeature_c".into());
    let mut extrusion = feature("extrusion", Some("43"), 1);
    extrusion.kind = "Extrusion".into();
    extrusion.input_class = Some("moExtrusion_c".into());
    extrusion.properties.insert("Profile".into(), "42".into());
    extrusion
        .properties
        .insert("Operation".into(), "Join".into());
    extrusion.parameters.insert("D1".into(), "5".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![sketch, extrusion],
    };

    let projected = project_features(&[history]);
    let sketch_id = neutral_feature_id("sketch");
    assert!(matches!(
        &projected[1].definition,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(feature),
            ..
        } if feature == &sketch_id
    ));
    assert_eq!(projected[1].dependencies, [sketch_id]);
}
