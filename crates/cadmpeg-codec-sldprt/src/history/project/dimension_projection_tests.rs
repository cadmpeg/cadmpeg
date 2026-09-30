// SPDX-License-Identifier: Apache-2.0
//! Dimension and cosmetic thread feature projection tests.
#![allow(clippy::unwrap_used)]

use crate::history::parameters::format_native_scalar;
use crate::history::project::modify::project_chamfer;
use crate::history::project::project_features;
use crate::history::tests::feature;
use crate::history::tests::feature_input_lane;
use crate::records::FeatureContent;
use crate::records::FeatureHistory;
use cadmpeg_ir::features::edge_treatments::ChamferSpec;
use cadmpeg_ir::features::holes::HoleKind;
use cadmpeg_ir::features::AngularTermination;
use cadmpeg_ir::features::BooleanOp;
use cadmpeg_ir::features::CosmeticThreadExtent;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::LinearTermination;
use cadmpeg_ir::features::RevolveExtent;
use std::collections::BTreeMap;
const EPS_PROJECTED_REVOLUTION_ANGLE: f64 = 1.0e-12;
const EPS_BOUND_REVOLUTION_ANGLE: f64 = 1.0e-12;

#[test]
fn simple_hole_uses_its_profile_dimension_roles() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        "213,212".into(),
    );
    let mut position = feature("position", Some("213"), 1);
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    let mut profile = feature("profile", Some("212"), 1);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized diameter"),
        "<MOD-DIAM>4.5".into(),
    );
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized depth"),
        "13.2".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, position, profile],
    };

    let projected = project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history),
    )
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
        projected[0].evaluation.definition()
    else {
        panic!("expected a hole definition");
    };
    let diameter = shape.diameter();
    assert_eq!(
        diameter,
        Some(cadmpeg_ir::scalar::PositiveLength::new(4.5).unwrap())
    );
    assert_eq!(
        *extent,
        Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(13.2).unwrap()
        })
    );

    let mut ambiguous = history;
    ambiguous.features[2].parameters.insert(
        cadmpeg_core::nonblank_literal!("another length"),
        "2".into(),
    );
    let ambiguous = project_features(
        &cadmpeg_test_support::service_decode_context(),
        &[ambiguous],
    )
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Hole { shape, extent, .. }) =
        ambiguous[0].evaluation.definition()
    else {
        panic!("expected a hole definition");
    };
    let diameter = shape.diameter();
    assert_eq!(diameter, None);
    assert_eq!(*extent, None);
}

#[test]
fn hole_wizard_rejects_unsupported_countersink_child_schema() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        "213,212".into(),
    );
    let mut position = feature("position", Some("213"), 1);
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "11".into());
    let mut profile = feature("profile", Some("212"), 2);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized bore"),
        "<MOD-DIAM>3.4".into(),
    );
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized depth"),
        "3".into(),
    );
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized entry"),
        "<MOD-DIAM>6.6".into(),
    );
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized angle"),
        "90°".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, position, profile],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,

            extent: None,
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::Simple,
                ..
            }, None,))));
}

#[test]
fn hole_wizard_drill_point_profile_retains_bore_and_blind_depth() {
    let mut hole = feature("hole", Some("214"), 0);
    hole.xml_tag = "HoleWizard".into();
    hole.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        "212".into(),
    );
    let mut profile = feature("profile", Some("212"), 1);
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile.parameters.insert(
        cadmpeg_core::text::NonBlankString::new("螺纹孔钻头直径").expect("named dimension"),
        "<MOD-DIAM>4.2".into(),
    );
    profile.parameters.insert(
        cadmpeg_core::text::NonBlankString::new("螺纹孔钻头深度").expect("named dimension"),
        "10".into(),
    );
    profile.parameters.insert(
        cadmpeg_core::text::NonBlankString::new("导头角度").expect("named dimension"),
        "118°".into(),
    );
    profile.content.extend([
        FeatureContent::Dimension("导头角度".into()),
        FeatureContent::Dimension("螺纹孔钻头深度".into()),
        FeatureContent::Dimension("螺纹孔钻头直径".into()),
    ]);
    profile.parameters.insert(
        cadmpeg_core::nonblank_literal!("derived native scalar"),
        "937.25".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![hole, profile],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,

            extent: Some(LinearTermination::Blind {
                length: actual_length,
            }),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: HoleKind::SimpleDrilled {
                    drill_point_angle,
                },
                ..
            }, Some(actual_diameter),) if ((drill_point_angle.get() - 118.0_f64.to_radians()).abs() < 1.0e-12) && actual_diameter.get() == 4.2 && actual_length.get() == 10.0)));
}

#[test]
fn native_scalar_refresh_preserves_radial_dimension_semantics() {
    let profile = feature("profile", Some("212"), 1);

    assert_eq!(
        format_native_scalar(&profile, "bore", 0.0042, Some("<MOD-DIAM>4.2")).as_deref(),
        Some("<MOD-DIAM>4.2")
    );
    assert_eq!(
        format_native_scalar(&profile, "radius", 0.003, Some("&lt;MOD-RHO&gt;3")).as_deref(),
        Some("&lt;MOD-RHO&gt;3")
    );
}

/// A native scalar in metres whose millimetre value overflows has no
/// expression; it was written as `inf`.
#[test]
fn native_scalar_whose_millimetre_value_overflows_has_no_expression() {
    let profile = feature("profile", Some("212"), 1);

    assert_eq!(
        format_native_scalar(&profile, "bore", 1.0e306, Some("<MOD-DIAM>4.2")),
        None
    );
}

#[test]
fn legacy_revolve_uses_d1_angle_and_cut_class_operation() {
    let mut revolve = feature("revolve", Some("42"), 0);
    revolve.input_class = Some("moRevCut_c".into());
    revolve
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "360°".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![revolve],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            op: BooleanOp::Cut,
        }) if matches!(construction.extent(), Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value }
                }) if (value.get() - std::f64::consts::TAU).abs() < EPS_PROJECTED_REVOLUTION_ANGLE)
    ));
}

#[test]
fn localized_cut_extrusion_uses_its_native_class_operation() {
    let mut cut = feature("cut", Some("43"), 0);
    cut.kind = "BossExtrude".into();
    cut.input_class = Some("moCut_c".into());
    cut.parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "45".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![cut],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            op: BooleanOp::Cut,
            ..
        })
    ));
}

#[test]
fn revolve_uses_its_ordered_angle_dimension_name() {
    let mut revolve = feature("revolve", Some("42"), 0);
    revolve.input_class = Some("moRevolution_c".into());
    revolve
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("FIX_1"), "360°".into());
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

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if matches!(construction.extent(), Some(RevolveExtent::OneSided {
                    termination: AngularTermination::Angle { angle: value }
                }) if (value.get() - std::f64::consts::TAU).abs() < EPS_BOUND_REVOLUTION_ANGLE)
    ));
}

#[test]
fn chamfer_uses_physical_types_of_ordered_localized_dimensions() {
    let mut chamfer = feature("chamfer", Some("42"), 0);
    chamfer.input_class = Some("Chamfer_c".into());
    chamfer.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized length"),
        "1.5".into(),
    );
    chamfer.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized angle"),
        "45°".into(),
    );
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

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert!(matches!(
        projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { ref groups, .. })
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    spec: ChamferSpec::DistanceAngle {
                        distance: actual_distance,
                        angle: value,
                    },
                    ..
                }] if ((value.get() - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12) && actual_distance.get() == 1.5
            )
    ));

    let mut distance = feature("distance", Some("43"), 0);
    distance.input_class = Some("Chamfer_c".into());
    distance.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized distance"),
        "2mm".into(),
    );
    distance
        .content
        .push(FeatureContent::Dimension("localized distance".into()));
    assert!(matches!(
        project_chamfer(&cadmpeg_test_support::service_decode_context(), &distance).unwrap(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { ref groups, .. })
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    spec: ChamferSpec::Distance {
                        distance: actual_distance,
                    },
                    ..
                }] if actual_distance.get() == 2.0
            )
    ));

    distance.parameters.insert(
        cadmpeg_core::nonblank_literal!("localized second distance"),
        "3mm".into(),
    );
    distance.content.push(FeatureContent::Dimension(
        "localized second distance".into(),
    ));
    assert!(matches!(
        project_chamfer(&cadmpeg_test_support::service_decode_context(), &distance).unwrap(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { ref groups, .. })
            if matches!(
                groups.as_slice(),
                [cadmpeg_ir::features::edge_treatments::ChamferGroup {
                    spec: ChamferSpec::TwoDistances {
                        first: actual_first,
                        second: actual_second,
                    },
                    ..
                }] if actual_first.get() == 2.0 && actual_second.get() == 3.0
            )
    ));
}

#[test]
fn cosmetic_thread_retains_nominal_diameter_and_blind_length() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D1"), "16".into());
    thread
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D2"), "<MOD-DIAM>8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert_eq!(
        *projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face: FaceSelection::Unresolved,
            diameter: Some(cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap()),
            extent: Some(CosmeticThreadExtent::Blind {
                length: cadmpeg_ir::scalar::PositiveLength::new(16.0).unwrap(),
            }),
        })
    );
}

#[test]
fn cosmetic_thread_without_blind_length_is_through() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("D2"), "<MOD-DIAM>8".into());
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    assert_eq!(
        *projected[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face: FaceSelection::Unresolved,
            diameter: Some(cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap()),
            extent: Some(CosmeticThreadExtent::Through {}),
        })
    );
}

#[test]
fn cosmetic_thread_non_length_d1_and_named_diameter_are_through() {
    for d1 in ["0", "6.2831853071796rad"] {
        let mut thread = feature("thread", Some("42"), 0);
        thread.input_class = Some("moCosmeticThread_c".into());
        thread
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("D1"), d1.into());
        thread.parameters.insert(
            cadmpeg_core::nonblank_literal!("thread size"),
            "<MOD-DIAM>4.9".into(),
        );
        let history = FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![thread],
        };

        let projected =
            project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
        assert_eq!(
            *projected[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face: FaceSelection::Unresolved,
                diameter: Some(cadmpeg_ir::scalar::PositiveLength::new(4.9).unwrap()),
                extent: Some(CosmeticThreadExtent::Through {}),
            })
        );
    }
}

#[test]
fn cosmetic_thread_requires_one_named_diameter() {
    let mut thread = feature("thread", Some("42"), 0);
    thread.input_class = Some("moCosmeticThread_c".into());
    thread.parameters.insert(
        cadmpeg_core::nonblank_literal!("major"),
        "<MOD-DIAM>8".into(),
    );
    thread.parameters.insert(
        cadmpeg_core::nonblank_literal!("minor"),
        "<MOD-DIAM>6.8".into(),
    );
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![thread],
    };

    let projected =
        project_features(&cadmpeg_test_support::service_decode_context(), &[history]).unwrap();
    let FeatureDefinition::Operation(FeatureOperation::CosmeticThread { diameter, .. }) =
        projected[0].evaluation.definition()
    else {
        panic!("expected a cosmetic thread");
    };
    assert_eq!(*diameter, None);
}

#[test]
fn cosmetic_thread_inherits_one_threaded_hole_major_diameter() {
    let mut hole = feature("hole", Some("10"), 0);
    hole.input_class = Some("moHoleWzd_c".into());
    hole.properties.insert(
        cadmpeg_core::nonblank_literal!("DissectableChildren"),
        "11".into(),
    );

    let mut profile = feature("profile", Some("11"), 1);
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile.parameters = [
        (
            cadmpeg_core::nonblank_literal!("bore"),
            "<MOD-DIAM>2.5".into(),
        ),
        (cadmpeg_core::nonblank_literal!("drill depth"), "7.5".into()),
        (
            cadmpeg_core::nonblank_literal!("major"),
            "<MOD-DIAM>3".into(),
        ),
        (cadmpeg_core::nonblank_literal!("thread depth"), "6".into()),
        (cadmpeg_core::nonblank_literal!("angle"), "118°".into()),
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
            kind: crate::records::FeatureInputSurfaceSelectionKind::Component,
            object_name_ref: "thread-name".into(),
            feature_ref: thread_id,
            producer_feature_refs: vec![hole_id.clone()],
            terminal_feature_ref: Some(hole_id),
            components: Vec::new(),
        });

    crate::resolved_features::holes::enrich_history_cosmetic_thread_diameters(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_mut(&mut history),
        &[lane],
    )
    .unwrap();
    assert_eq!(
        history.features[2].parameters.get("D2"),
        Some(&"<MOD-DIAM>3mm".to_string())
    );
}
