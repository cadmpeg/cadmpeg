// SPDX-License-Identifier: Apache-2.0
//! Feature-class, hole, plane, and profile projection tests.
#![allow(clippy::unwrap_used)]

use crate::history::bind::order_model_features_for_regeneration;
use crate::history::classify::is_offset_plane;
use crate::history::classify::HistoryIndex;
use crate::history::configuration::restore_configuration_tree_node_definitions;
use crate::history::enrich_scene_classes;
use crate::history::parameters::native_parameter_is_length;
use crate::history::parameters::project_parameters;
use crate::history::project::neutral_feature_id_charged;
use crate::history::project::project_definition;
use crate::history::project::project_feature_content;
use crate::history::project::project_features;
use crate::history::project::projected_parameter_names;
use crate::history::project::solid::hole_sketch_construction;
use crate::history::project::solid::project_extrude;
use crate::history::project::solid::project_hole;
use crate::history::tests::feature;
use crate::history::tests::feature_input_lane;
use crate::records::Feature;
use crate::records::FeatureContent;
use crate::records::FeatureHistory;
use crate::records::ObjectId;
use cadmpeg_ir::features::holes::HoleBottom;
use cadmpeg_ir::features::holes::HoleKind;
use cadmpeg_ir::features::BooleanOp;
use cadmpeg_ir::features::ExtrudeExtent;
use cadmpeg_ir::features::ExtrudeSide;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::FeatureSourceContent;
use cadmpeg_ir::features::FeatureTreeNodeRole;
use cadmpeg_ir::features::LinearTermination;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::features::ProfileRef;
use cadmpeg_ir::features::UnresolvedFamily;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Length;
use std::collections::BTreeMap;
use std::collections::HashMap;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn role_in(feature: &Feature, features: &[Feature]) -> Option<FeatureTreeNodeRole> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = HistoryIndex::new(&ctx, features).unwrap();
    index.tree_node_role(&ctx, feature).unwrap()
}

fn plane_in(
    feature: &Feature,
    features: &[Feature],
) -> Option<cadmpeg_ir::features::PrincipalPlane> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = HistoryIndex::new(&ctx, features).unwrap();
    index.principal_plane(&ctx, feature).unwrap()
}

fn records_by_id(features: &[Feature]) -> HashMap<&str, Option<&Feature>> {
    let mut records = HashMap::new();
    for feature in features {
        records
            .entry(feature.id.as_str())
            .and_modify(|record| *record = None)
            .or_insert(Some(feature));
    }
    records
}

fn definition_of(feature: &Feature) -> FeatureDefinition {
    let ctx = cadmpeg_test_support::service_decode_context();
    let features = std::slice::from_ref(feature);
    let index = HistoryIndex::new(&ctx, features).unwrap();
    let sources = super::solid::SourceFeatures::new(&ctx, &[]).unwrap();
    project_definition(
        &ctx,
        feature,
        &HashMap::new(),
        &HashMap::new(),
        &sources,
        &records_by_id(features),
        &index,
    )
    .unwrap()
}

#[test]
fn charged_feature_ids_preserve_native_key_escaping() {
    with_test_ctx(|ctx| {
        for (native, expected) in [
            ("sldprt:history:feature#1:2", "sldprt:model:feature#1:2"),
            (
                "sldprt:history:feature#two#percent% space",
                "sldprt:model:feature#two%23percent%25%20space",
            ),
            ("custom-native-id", "sldprt:model:feature#custom-native-id"),
        ] {
            assert_eq!(
                neutral_feature_id_charged(ctx, native).unwrap(),
                cadmpeg_ir::features::FeatureId::mint(expected).unwrap()
            );
        }
    });
}

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
    ir.model.features =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let predecessor = ir.model.features[1].id.clone();
    let consumer = ir.model.features[0].id.clone();
    ir.model
        .configurations
        .push(cadmpeg_ir::features::DesignConfiguration {
            id: cadmpeg_ir::features::ConfigurationId::mint("synthetic:test:id#configuration")
                .expect("identity grammar"),
            ordinal: 0,
            active: true,
            source_index: None,
            name: Some("configuration".to_string()),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            bodies: None,
            parameter_values: BTreeMap::new(),
            feature_states: BTreeMap::from([(
                consumer.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                        outputs: cadmpeg_ir::features::DistinctMembers::default(),
                    },
                    dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                        vec![predecessor.clone()],
                        &cadmpeg_test_support::service_decode_context(),
                    )
                    .unwrap(),
                    definition: ir.model.features[0].evaluation.definition().clone(),
                },
            )]),
            native_ref: None,
        });

    assert!(
        with_test_ctx(|ctx| order_model_features_for_regeneration(ctx, &mut ir))
            .expect("test feature ordering")
    );
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
    feature
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("s"), "2.1".into());
    feature.properties.insert(
        cadmpeg_core::nonblank_literal!("EndCondition"),
        "Blind".into(),
    );

    assert!(native_parameter_is_length(
        &cadmpeg_test_support::service_decode_context(),
        &feature,
        "s",
        Some("2.1")
    )
    .unwrap());
    assert!(matches!(
        project_extrude(&cadmpeg_test_support::service_decode_context(), &feature, &HashMap::new(), &super::solid::SourceFeatures::new(&cadmpeg_test_support::service_decode_context(), &[]).unwrap()).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            ..
        })) if actual_length.get() == 2.1
    ));
}

#[test]
fn modern_extrusion_with_one_source_dimension_defaults_to_blind() {
    let mut feature = feature("sldprt:history:feature#1:2", Some("12"), 2);
    feature.xml_tag = "Extrusion".into();
    feature.input_class = Some("moExtrusion_c".into());
    feature.content = vec![FeatureContent::Dimension("m".into())];
    feature
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("m"), "6.4".into());

    assert!(matches!(
        project_extrude(&cadmpeg_test_support::service_decode_context(), &feature, &HashMap::new(), &super::solid::SourceFeatures::new(&cadmpeg_test_support::service_decode_context(), &[]).unwrap()).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            ..
        })) if actual_length.get() == 6.4
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
    extrusion
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("m"), "6.8".into());
    extrusion
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("aux-1"), "1.2".into());
    extrusion
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("aux-2"), "3.4".into());
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

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
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
        extrusion.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Feature(profile_ref)),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: actual_length },
                    ..
                }
            },
            op: BooleanOp::Join,
            ..
        }) if (profile_ref == &profile.id) && actual_length.get() == 6.8
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
    extrusion.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableRoot"),
        "true".into(),
    );
    extrusion.properties.insert(
        cadmpeg_core::nonblank_literal!("EndCondition"),
        "Blind".into(),
    );
    extrusion
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "4.2".into());
    extrusion.content = vec![FeatureContent::Dimension("D1".into())];

    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![extrusion, early_profile, origin_profile, preceding_profile],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let extrusion = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:3"))
        .expect("root extrusion feature");
    let profile = projected
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("sldprt:history:feature#1:2"))
        .expect("preceding extrusion profile");

    assert!(matches!(
        extrusion.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Feature(profile_ref)),
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind { length: actual_length },
                    ..
                }
            },
            op: BooleanOp::Cut,
            ..
        }) if (profile_ref == &profile.id) && actual_length.get() == 4.2
    ));
}

#[test]
fn repeated_dimension_content_projects_one_owned_parameter() {
    let mut feature = feature("sldprt:history:feature#1:2", None, 2);
    feature
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "2".into());
    feature.content = vec![
        FeatureContent::Dimension("D1".into()),
        FeatureContent::Dimension("D1".into()),
    ];

    assert_eq!(
        projected_parameter_names(&cadmpeg_test_support::service_decode_context(), &feature)
            .unwrap(),
        vec!["D1"]
    );
    assert_eq!(
        (&*project_feature_content(
            &cadmpeg_test_support::service_decode_context(),
            &feature,
            &HashMap::new()
        )
        .unwrap()),
        vec![FeatureSourceContent::Parameter(
            ParameterId::mint("sldprt:model:parameter#1:2:0").expect("identity grammar")
        )]
    );
}

#[test]
fn spatial_profile_class_projects_a_spatial_sketch() {
    let mut spatial = feature("spatial", Some("7"), 0);
    spatial.xml_tag = "Sketch".into();
    spatial.kind = "Sketch".into();
    spatial.input_class = Some("mo3DProfileFeature_c".into());

    assert_eq!(
        definition_of(&spatial),
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: None })
    );
}

#[test]
fn base_body_class_projects_stored_geometry_independently_of_display_name() {
    let mut base_body = feature("base-body", Some("18"), 0);
    base_body.kind = "Localized imported body".into();
    base_body.input_class = Some("moBaseBody_c".into());

    assert_eq!(
        definition_of(&base_body),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    );
}

#[test]
fn hole_profile_dimension_order_distinguishes_counterbore_and_thread() {
    let profile = |roles: &[(&str, &str)]| {
        let mut profile = feature("profile", Some("7"), 0);
        profile.kind = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        for (name, expression) in roles {
            profile.parameters.insert(
                cadmpeg_core::text::NonBlankString::try_from(*name).expect("named dimension"),
                (*expression).into(),
            );
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
    let construction = hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &counterbore,
    )
    .expect("resource budget")
    .expect("required invariant");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(5.5).unwrap()
    );
    assert_eq!(
        construction.depth,
        Some(cadmpeg_ir::scalar::PositiveLength::new(12.0).unwrap())
    );
    assert!(matches!(
        construction.construction,
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: HoleKind::CounterboreDrilled {
                diameter: actual_diameter,
                depth: actual_depth,
                ..
            },
            ..
        } if actual_diameter.get() == 9.0 && actual_depth.get() == 5.7
    ));

    let threaded = profile(&[
        ("a", "<MOD-DIAM>4.2"),
        ("b", "12.4"),
        ("c", "<MOD-DIAM>5"),
        ("d", "10"),
        ("e", "118°"),
    ]);
    let construction =
        hole_sketch_construction(&cadmpeg_test_support::service_decode_context(), &threaded)
            .expect("resource budget")
            .expect("required invariant");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(4.2).unwrap()
    );
    assert_eq!(
        construction.depth,
        Some(cadmpeg_ir::scalar::PositiveLength::new(12.4).unwrap())
    );
    assert!(matches!(
        construction.construction,
        cadmpeg_ir::features::holes::HoleConstruction::NativeThread {
            major_diameter: actual_major_diameter,
            thread_depth: actual_thread_depth,
            pitch: None,
            ..
        } if actual_major_diameter.get() == 5.0 && actual_thread_depth.get() == 10.0
    ));

    let tapered_thread = profile(&[
        ("a", "3.43°"),
        ("b", "6.92"),
        ("c", "118°"),
        ("d", "<MOD-DIAM>8.43"),
        ("e", "11.62"),
        ("f", "<MOD-DIAM>10.29"),
    ]);
    let construction = hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &tapered_thread,
    )
    .expect("resource budget")
    .expect("tapered thread profile");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(8.43).unwrap()
    );
    assert_eq!(
        construction.depth,
        Some(cadmpeg_ir::scalar::PositiveLength::new(11.62).unwrap())
    );
    assert!(matches!(
        construction.construction,
        cadmpeg_ir::features::holes::HoleConstruction::NativeThread {
            major_diameter: actual_major_diameter,
            thread_depth: actual_thread_depth,
            pitch: None,
            drill_point_angle: angle,
        } if ((angle.get() - 118_f64.to_radians()).abs() < 1.0e-12) && actual_major_diameter.get() == 10.29 && actual_thread_depth.get() == 6.92
    ));
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: cadmpeg_ir::scalar::InteriorAngle::new(118_f64.to_radians()).unwrap(),
            depth_to_tip: false,
        })
    );
    assert_eq!(
        construction.taper_angle,
        Some(cadmpeg_ir::scalar::InteriorAngle::new(3.43_f64.to_radians()).unwrap())
    );

    let counterbore_with_exit_countersink = profile(&[
        ("a", "4.6"),
        ("b", "<MOD-DIAM>8"),
        ("c", "90°"),
        ("d", "10"),
        ("e", "<MOD-DIAM>4.5"),
        ("f", "<MOD-DIAM>4.55"),
    ]);
    let construction = hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &counterbore_with_exit_countersink,
    )
    .expect("resource budget")
    .expect("dual-ended profile");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(4.5).unwrap()
    );
    assert_eq!(
        construction.depth,
        Some(cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap())
    );
    assert_eq!(
        construction.construction,
        cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
            depth: cadmpeg_ir::scalar::PositiveLength::new(4.6).unwrap()
        })
    );
    assert_eq!(
        construction.exit_kind,
        Some(HoleKind::Countersink {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(4.55).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
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
    let construction = hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &counterdrill,
    )
    .expect("resource budget")
    .expect("counterdrill profile");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(5.5).unwrap()
    );
    assert_eq!(
        construction.depth,
        Some(cadmpeg_ir::scalar::PositiveLength::new(12.4).unwrap())
    );
    assert_eq!(
        construction.construction,
        cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Counterdrill {
            diameters: cadmpeg_ir::features::holes::CounterdrillDiameters::new(
                cadmpeg_ir::scalar::PositiveLength::new(9.95).unwrap(),
                Some(cadmpeg_ir::scalar::PositiveLength::new(10.05).unwrap())
            )
            .unwrap(),

            depth: cadmpeg_ir::scalar::PositiveLength::new(5.4).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
    );
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: cadmpeg_ir::scalar::InteriorAngle::new(118_f64.to_radians()).unwrap(),
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
    assert!(hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &placement_dimensions
    )
    .expect("resource budget")
    .is_none());

    let unsupported_countersink = profile(&[
        ("diameter", "<MOD-DIAM>5"),
        ("entry", "<MOD-DIAM>9"),
        ("depth", "6"),
        ("angle", "82°"),
    ]);
    assert!(hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &unsupported_countersink
    )
    .expect("resource budget")
    .is_none());

    let unsupported_counterbore = profile(&[
        ("diameter", "<MOD-DIAM>5"),
        ("entry", "<MOD-DIAM>9"),
        ("entry depth", "3"),
        ("depth", "6"),
    ]);
    assert!(hole_sketch_construction(
        &cadmpeg_test_support::service_decode_context(),
        &unsupported_counterbore
    )
    .expect("resource budget")
    .is_none());

    let mut native_profile = profile(&[("diameter", "<MOD-DIAM>6.6"), ("depth", "9.4")]);
    native_profile.id = "native-profile".into();
    native_profile.source_id = None;
    let mut native_owned = feature("native-owned-hole", None, 0);
    native_owned.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        native_profile.id.clone(),
    );
    let projected = project_hole(
        &cadmpeg_test_support::service_decode_context(),
        &native_owned,
        &BTreeMap::new(),
        &records_by_id(&[native_owned.clone(), native_profile]),
    )
    .expect("resource budget")
    .unwrap();
    assert!(matches!(
        projected, FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::Blind {
                length: actual_length
            }),
            ..
        }) if matches!((&shape.diameter(),), (Some(actual_diameter),) if actual_diameter.get() == 6.6 && actual_length.get() == 9.4)));

    let mut canonical = feature("hole", Some("8"), 0);
    canonical.parameters = [
        (cadmpeg_core::nonblank_literal!("Diameter"), "4.2mm".into()),
        (cadmpeg_core::nonblank_literal!("Depth"), "12.4mm".into()),
        (
            cadmpeg_core::nonblank_literal!("ThreadMajorDiameter"),
            "5mm".into(),
        ),
        (
            cadmpeg_core::nonblank_literal!("ThreadDepth"),
            "10mm".into(),
        ),
        (
            cadmpeg_core::nonblank_literal!("DrillPointAngle"),
            "118°".into(),
        ),
    ]
    .into();
    let projected = project_hole(
        &cadmpeg_test_support::service_decode_context(),
        &canonical,
        &BTreeMap::new(),
        &records_by_id(std::slice::from_ref(&canonical)),
    )
    .expect("resource budget")
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        ref shape,

        extent: Some(LinearTermination::Blind { length }),
        ..
    }) = projected
    else {
        panic!("expected canonical threaded hole: {projected:?}");
    };
    let cadmpeg_ir::features::holes::HoleConstruction::NativeThread {
        major_diameter,
        thread_depth,
        ..
    } = shape.construction()
    else {
        panic!("expected canonical threaded hole: {projected:?}");
    };
    let Some(diameter) = &shape.diameter() else {
        panic!("expected canonical threaded hole: {projected:?}");
    };
    assert!((diameter.get() - 4.2).abs() < 1.0e-12);
    assert!((major_diameter.get() - 5.0).abs() < 1.0e-12);
    assert!((thread_depth.get() - 10.0).abs() < 1.0e-12);
    assert!((length.get() - 12.4).abs() < 1.0e-12);
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
    let scene = HashMap::from([(153, "moDirectionLight_c".into())]);

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    enrich_scene_classes(&ctx, &mut histories, &scene).unwrap();

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
            let mut sentinel = feature(
                "sentinel",
                Some(source),
                u32::try_from(roster.len()).unwrap(),
            );
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
        assert_eq!(role_in(&node, &roster(&node)), Some(expected));
    }

    let mut fourth_light = feature("fourth", Some("70"), 0);
    fourth_light.kind = "本地化方向光".into();
    let mut directional_roster = roster(&fourth_light);
    let mut first_light = feature("light", Some("13"), 13);
    first_light.kind = fourth_light.kind.clone();
    directional_roster.push(first_light);
    assert_eq!(
        role_in(&fourth_light, &directional_roster),
        Some(FeatureTreeNodeRole::DirectionalLight)
    );

    let mut additional_ambient = feature("additional ambient", Some("16"), 0);
    additional_ambient.kind = "本地化环境光".into();
    let mut ambient_roster = roster(&additional_ambient);
    let mut reserved_ambient = feature("ambient", Some("12"), 12);
    reserved_ambient.kind = additional_ambient.kind.clone();
    ambient_roster.push(reserved_ambient);
    assert_eq!(
        role_in(&additional_ambient, &ambient_roster),
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
            let mut sentinel = feature(
                "sentinel",
                Some(source),
                u32::try_from(roster.len()).unwrap(),
            );
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
        assert_eq!(role_in(&node, &legacy_roster(&node)), Some(expected));
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
            u32::try_from(complete_legacy_roster.len()).unwrap(),
        );
        sentinel.input_class = Some(class.into());
        complete_legacy_roster.push(sentinel);
    }
    for source in ["7", "8"] {
        complete_legacy_roster.push(feature(
            "legacy light",
            Some(source),
            u32::try_from(complete_legacy_roster.len()).unwrap(),
        ));
    }
    assert_eq!(
        role_in(&legacy_lights, &complete_legacy_roster),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let roster_from = |node: &Feature, classes: &[(&str, &str)], classless_sources: &[&str]| {
        let mut features = vec![node.clone()];
        for (source, class) in classes {
            let mut sentinel = feature(
                "sentinel",
                Some(source),
                u32::try_from(features.len()).unwrap(),
            );
            sentinel.input_class = Some((*class).into());
            features.push(sentinel);
        }
        for source in classless_sources {
            features.push(feature(
                "reserved",
                Some(source),
                u32::try_from(features.len()).unwrap(),
            ));
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
        role_in(&lights, &roster_from(&lights, &default_frame, &["7", "8"])),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let ambient = feature("ambient", Some("10"), 0);
    let mut folders_at_seven = default_frame.to_vec();
    folders_at_seven.extend([("7", "moSolidBodyFolder_c"), ("8", "moSurfaceBodyFolder_c")]);
    assert_eq!(
        role_in(
            &ambient,
            &roster_from(&ambient, &folders_at_seven, &["6", "11", "12"])
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
        role_in(
            &early_lights,
            &roster_from(&early_lights, &origin_at_six, &["7", "8"])
        ),
        Some(FeatureTreeNodeRole::LightsAndCameras)
    );

    let ambiguous = feature("node", Some("99"), 0);
    assert_eq!(role_in(&ambiguous, &[]), None);

    let mut exploded_views = ambiguous.clone();
    exploded_views.name.clear();
    assert_eq!(
        role_in(&exploded_views, &roster(&exploded_views)),
        Some(FeatureTreeNodeRole::ExplodedViews)
    );

    let mut reference_plane = feature("node", Some("5"), 0);
    reference_plane.input_class = Some("moRefPlane_c".into());
    assert_eq!(role_in(&reference_plane, &[]), None);

    let mut sheet_metal = feature("node", Some("-1"), 0);
    sheet_metal.name.clear();
    assert_eq!(
        role_in(&sheet_metal, &roster(&sheet_metal)),
        Some(FeatureTreeNodeRole::SheetMetal)
    );
    sheet_metal.name = "任意本地化鈑金根節點".into();
    assert_eq!(
        role_in(&sheet_metal, &roster(&sheet_metal)),
        Some(FeatureTreeNodeRole::SheetMetal)
    );
    assert_eq!(role_in(&sheet_metal, &[]), None);
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
            object_id: ObjectId::from_value(25),
            value: "instance".into(),
        },
        crate::records::FeatureInputName {
            id: "definition-name".into(),
            parent: "lane".into(),
            ordinal: 1,
            offset: 140,
            object_id: ObjectId::from_value(23),
            value: "definition".into(),
        },
        crate::records::FeatureInputName {
            id: "compact-instance-name".into(),
            parent: "lane".into(),
            ordinal: 2,
            offset: 340,
            object_id: ObjectId::from_value(34),
            value: "compact".into(),
        },
    ];

    crate::resolved_features::reference_geometry::enrich_history_sketch_block_references(
        &cadmpeg_test_support::service_decode_context(),
        &mut histories,
        &[lane],
    )
    .unwrap();

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
    assert_eq!(plane_in(&front, &features), None);
    assert_eq!(plane_in(&top, &features), None);
    assert_eq!(plane_in(&right, &features), None);
}

#[test]
fn angular_plane_parameter_does_not_claim_offset_semantics() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());
    plane
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "0rad".into());
    plane.properties.insert(
        cadmpeg_core::nonblank_literal!("Origin"),
        "0mm,70mm,0mm".into(),
    );
    plane
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Normal"), "0,1,0".into());
    plane
        .properties
        .insert(cadmpeg_core::nonblank_literal!("UAxis"), "-1,0,0".into());

    assert!(!is_offset_plane(&cadmpeg_test_support::service_decode_context(), &plane).unwrap());
    assert_eq!(
        definition_of(&plane),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 70.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0)
            )
            .unwrap(),
        })
    );
}

#[test]
fn length_plane_parameter_claims_offset_semantics() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());
    plane
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "70mm".into());

    assert!(is_offset_plane(&cadmpeg_test_support::service_decode_context(), &plane).unwrap());
    assert_eq!(
        definition_of(&plane),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: None,
            distance: Length::new(70.0).unwrap(),
        })
    );
}

#[test]
fn frameless_reference_plane_remains_typed_unresolved() {
    let mut plane = feature("plane", Some("90"), 0);
    plane.input_class = Some("moRefPlane_c".into());

    assert_eq!(
        definition_of(&plane),
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane
        })
    );
}

#[test]
fn legacy_principal_plane_requires_a_complete_matching_triplet() {
    let front = feature("front", Some("2"), 0);
    let top = feature("top", Some("3"), 1);
    let right = feature("right", Some("4"), 2);
    let features = [front.clone(), top.clone(), right.clone()];
    assert_eq!(
        plane_in(&front, &features),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );

    let mut mismatched = right.clone();
    mismatched.kind = "Different".into();
    let features = [front.clone(), top.clone(), mismatched];
    assert_eq!(plane_in(&front, &features), None);
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
        plane_in(&front, &records),
        Some(cadmpeg_ir::features::PrincipalPlane::Front)
    );

    let mut unbounded = records.clone();
    unbounded[3].kind = unbounded[0].kind.clone();
    assert_eq!(plane_in(&front, &unbounded), None);

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
    assert_eq!(plane_in(&ambiguous[0], &ambiguous), None);
}

#[test]
fn native_attribute_records_are_metadata_not_model_features() {
    let mut definition = feature("definition", Some("-1"), 0);
    definition.name = "VendorSettings.1".into();
    definition.parameters.insert(
        cadmpeg_core::nonblank_literal!("VendorSettings.1"),
        "0".into(),
    );
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

    let projected = project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history),
    )
    .unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].native_ref.as_deref(), Some("model"));
    assert!(
        project_parameters(&cadmpeg_test_support::service_decode_context(), &[history])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn native_attribute_definition_type_is_metadata_without_an_instance_name_match() {
    let mut definition = feature("definition", Some("-1"), 0);
    definition.kind = "Attribute-Definition".into();
    definition.name = "NativeAttributeFamily".into();
    definition.parameters.insert(
        cadmpeg_core::nonblank_literal!("NativeAttributeFamily"),
        "0".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![definition],
    };

    assert!(project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history)
    )
    .unwrap()
    .is_empty());
    assert!(
        project_parameters(&cadmpeg_test_support::service_decode_context(), &[history])
            .unwrap()
            .is_empty()
    );
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
    let mut configured = project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history),
    )
    .unwrap();
    assert!(matches!(
        configured[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    let mut base = configured.clone();
    base[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::DirectionalLight,
            children: cadmpeg_ir::features::TreeChildren::default(),
        }));

    restore_configuration_tree_node_definitions(
        &cadmpeg_test_support::service_decode_context(),
        &mut configured,
        &base,
    )
    .unwrap();
    assert!(matches!(
        configured[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::DirectionalLight,
            ..
        })
    ));
}

#[test]
fn hole_profile_rejects_more_than_supported_dimension_roles() {
    let mut profile = feature("profile", Some("7"), 0);
    for (name, expression) in [
        ("a", "<MOD-DIAM>5"),
        ("b", "1"),
        ("c", "2"),
        ("d", "3"),
        ("e", "4"),
        ("f", "5"),
        ("g", "6"),
        ("h", "7"),
    ] {
        profile.parameters.insert(
            cadmpeg_core::text::NonBlankString::try_from(name).expect("named dimension"),
            expression.into(),
        );
        profile.content.push(FeatureContent::Dimension(name.into()));
    }
    assert!(
        hole_sketch_construction(&cadmpeg_test_support::service_decode_context(), &profile)
            .expect("resource budget")
            .is_none()
    );
}

#[test]
fn hole_profile_parameter_fallback_requires_no_dimension_content() {
    let mut profile = feature("profile", Some("7"), 0);
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("diameter"),
        "<MOD-DIAM>5".into(),
    );
    profile
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("depth"), "9".into());
    let construction =
        hole_sketch_construction(&cadmpeg_test_support::service_decode_context(), &profile)
            .expect("resource budget")
            .expect("fallback hole dimensions");
    assert_eq!(construction.diameter.get(), 5.0);
    assert_eq!(
        construction
            .depth
            .map(cadmpeg_ir::scalar::PositiveLength::get),
        Some(9.0)
    );
    profile
        .content
        .push(FeatureContent::Dimension("missing".into()));
    assert!(
        hole_sketch_construction(&cadmpeg_test_support::service_decode_context(), &profile)
            .expect("resource budget")
            .is_none()
    );
}
