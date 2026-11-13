use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::{project_emboss, scope_properties};
use crate::design::test_support::parameter_record;
use crate::records::feature::emboss::{DesignEmbossConstruction, DesignEmbossControlWords};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, WrapMode};
use cadmpeg_ir::scalar::{Length, PositiveLength};

#[test]
fn missing_construction_keeps_owned_depth_in_a_neutral_wrap() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Emboss,
        32,
    );
    let depth = parse_design_parameter_record(&parameter_record(
        Some(94),
        "2 mm",
        "Depth",
        Some("mm"),
        "d7",
        0.2,
    ))
    .unwrap();
    let parameters = [(0, &depth)];
    let mut losses = Vec::new();
    let definition = project_emboss(&scope, &parameters, &[], &[], &mut losses).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            profile: cadmpeg_ir::features::PlanarProfileRef::Native(_),
            face: cadmpeg_ir::features::FaceSelection::Native(_),
            mode: WrapMode::Emboss { depth }
        }) if depth == PositiveLength::new(2.0).unwrap()
    ));
    for code in [
        crate::loss::F3dLossCode::EmbossProfileSelectionNative,
        crate::loss::F3dLossCode::EmbossTargetSelectionNative,
        crate::loss::F3dLossCode::EmbossDepthSignAssumed,
    ] {
        assert!(losses.iter().any(|loss| loss.code == code.kind()));
    }
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::EmbossDepthUnresolved.kind()));
    let properties =
        scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &parameters).unwrap();
    assert_eq!(
        properties.get("parameter:d7").map(String::as_str),
        Some("2 mm")
    );
    assert_eq!(
        properties.get("emboss:scalar:Depth:mm").map(String::as_str),
        Some("2")
    );
}

#[test]
fn emboss_source_properties_keep_controls_pairs_owners_and_each_scalar_lane() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Emboss,
        32,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Emboss(slot) = scope.payload_mut()
    {
        *slot = Some(DesignEmbossConstruction {
            control_words: DesignEmbossControlWords::Two([7, 9]),
            native_pair: Some([11, 13]),
            additional_native_pairs: Some(vec![[17, 19]]),
            depth_owner: Located {
                value: 90,
                offset: 10,
            },
            auxiliary_group: Located {
                value: 91,
                offset: 20,
            },
            horizontal_owner: Located {
                value: 92,
                offset: 30,
            },
            rotation_owner: Located {
                value: 93,
                offset: 40,
            },
            face_group: Located {
                value: 94,
                offset: 50,
            },
            vertical_owner: Located {
                value: 95,
                offset: 60,
            },
            profile_group: Located {
                value: 96,
                offset: 70,
            },
            sketch_profile: None,
        });
    }
    let parameters = [
        parse_design_parameter_record(&parameter_record(
            Some(90),
            "-2 mm",
            "Depth",
            Some("mm"),
            "d7",
            -0.2,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(92),
            "3 mm",
            "Horizontal Distance",
            Some("mm"),
            "d8",
            0.3,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(95),
            "4 mm",
            "Vertical Distance",
            Some("mm"),
            "d9",
            0.4,
        ))
        .unwrap(),
        parse_design_parameter_record(&parameter_record(
            Some(93),
            "90 deg",
            "Rotation Angle",
            Some("rad"),
            "d10",
            1.5707963267948966,
        ))
        .unwrap(),
    ];
    let indexed = [
        (0, &parameters[0]),
        (1, &parameters[1]),
        (2, &parameters[2]),
        (3, &parameters[3]),
    ];
    let mut losses = Vec::new();
    let definition = project_emboss(&scope, &indexed, &[], &[], &mut losses).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            mode: WrapMode::Deboss { depth },
            ..
        }) if depth == PositiveLength::new(2.0).unwrap()
    ));
    for code in [
        crate::loss::F3dLossCode::EmbossDepthSignAssumed,
        crate::loss::F3dLossCode::EmbossPlacementNative,
    ] {
        assert!(losses.iter().any(|loss| loss.code == code.kind()));
    }
    let properties = scope_properties(&scope, "f3d:Design/BulkStream.dat", &[], &indexed).unwrap();
    for (key, expected) in [
        ("emboss:control:0", "7"),
        ("emboss:control:1", "9"),
        ("emboss:native_pair:0:0", "11"),
        ("emboss:native_pair:0:1", "13"),
        ("emboss:native_pair:1:0", "17"),
        ("emboss:native_pair:1:1", "19"),
        ("emboss:group:profile", "96"),
        ("emboss:group:face", "94"),
        ("emboss:group:auxiliary", "91"),
        ("emboss:owner:Depth", "90"),
        ("emboss:owner:Horizontal Distance", "92"),
        ("emboss:owner:Vertical Distance", "95"),
        ("emboss:owner:Rotation Angle", "93"),
        ("emboss:scalar:Depth:mm", "-2"),
        ("emboss:scalar:Horizontal Distance:mm", "3"),
        ("emboss:scalar:Vertical Distance:mm", "4"),
        ("emboss:scalar:Rotation Angle:rad", "1.5707963267948966"),
    ] {
        assert_eq!(properties.get(key).map(String::as_str), Some(expected));
    }
    for (key, expression) in [
        ("parameter:d7", "-2 mm"),
        ("parameter:d8", "3 mm"),
        ("parameter:d9", "4 mm"),
        ("parameter:d10", "90 deg"),
    ] {
        assert_eq!(properties.get(key).map(String::as_str), Some(expression));
    }
}

#[test]
fn emboss_depth_keeps_signed_and_zero_values_and_refuses_duplicates() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Emboss,
        32,
    );
    let zero = parse_design_parameter_record(&parameter_record(
        Some(94),
        "0 mm",
        "Depth",
        Some("mm"),
        "d7",
        0.0,
    ))
    .unwrap();
    let positive = parse_design_parameter_record(&parameter_record(
        Some(94),
        "2 mm",
        "Depth",
        Some("mm"),
        "d6",
        0.2,
    ))
    .unwrap();
    let negative = parse_design_parameter_record(&parameter_record(
        Some(104),
        "-3 mm",
        "Depth",
        Some("mm"),
        "d8",
        -0.3,
    ))
    .unwrap();
    let definition = project_emboss(&scope, &[(0, &positive)], &[], &[], &mut Vec::new()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            mode: WrapMode::Emboss { depth },
            ..
        }) if depth == PositiveLength::new(2.0).unwrap()
    ));
    let mut zero_losses = Vec::new();
    let definition = project_emboss(&scope, &[(0, &zero)], &[], &[], &mut zero_losses).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            mode: WrapMode::Unresolved { depth: Some(value) },
            ..
        }) if value == Length::ZERO
    ));
    assert!(zero_losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::EmbossDepthUnresolved.kind()));
    assert!(!zero_losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::EmbossDepthSignAssumed.kind()));
    let definition = project_emboss(&scope, &[(0, &negative)], &[], &[], &mut Vec::new()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            mode: WrapMode::Deboss { depth },
            ..
        }) if depth == PositiveLength::new(3.0).unwrap()
    ));
    let duplicate = [(0, &zero), (1, &negative)];
    let mut duplicate_losses = Vec::new();
    let definition = project_emboss(&scope, &duplicate, &[], &[], &mut duplicate_losses).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            mode: WrapMode::Unresolved { depth: None },
            ..
        })
    ));
    assert!(duplicate_losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::EmbossDepthUnresolved.kind()));
    let mut missing_losses = Vec::new();
    let definition = project_emboss(&scope, &[], &[], &[], &mut missing_losses).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::Wrap {
            mode: WrapMode::Unresolved { depth: None },
            ..
        })
    ));
    assert!(missing_losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::EmbossDepthUnresolved.kind()));
}
