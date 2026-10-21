// SPDX-License-Identifier: Apache-2.0
//! A known bad cache cannot claim a complete typed control; missing caches retain identity.
#![allow(clippy::unwrap_used)]

use super::parameter_dimension;
use crate::history::tests::feature;
use cadmpeg_ir::features::{Dimension, ParameterValue};
use cadmpeg_ir::scalar::{Length, PositiveLength};
use std::collections::BTreeMap;

#[test]
fn missing_cache_retains_the_named_parameter_binding() {
    let mut source = feature("extrusion", Some("1"), 0);
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Depth"), "Width * 2".into());
    let binding = parameter_dimension::<PositiveLength>(&source, &["Depth"]);
    assert!(matches!(binding, Some(Dimension::Parameter(_))));
    source.dimension_properties.insert(
        "Depth".into(),
        BTreeMap::from([(cadmpeg_core::nonblank_literal!("Value"), "10mm".into())]),
    );
    assert_eq!(
        parameter_dimension::<PositiveLength>(&source, &["Depth"]),
        binding
    );
}

#[test]
fn inadmissible_source_cache_does_not_complete_a_positive_length() {
    let mut source = feature("extrusion", Some("1"), 0);
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Depth"), "Width * 2".into());
    for cache in ["0mm", "-1mm", "NaNmm", "infmm"] {
        source.dimension_properties.insert(
            "Depth".into(),
            BTreeMap::from([(cadmpeg_core::nonblank_literal!("Value"), cache.into())]),
        );
        assert!(
            parameter_dimension::<PositiveLength>(&source, &["Depth"]).is_none(),
            "{cache}"
        );
    }
    source.dimension_properties.clear();
    for (literal, value) in [("0mm", 0.0), ("-1mm", -1.0)] {
        source
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), literal.into());
        assert!(parameter_dimension::<PositiveLength>(&source, &["Depth"]).is_none());
        assert_eq!(
            crate::history::parameters::source_parameter_value(&source, "Depth"),
            Some(ParameterValue::Length(
                Length::new(value).expect("finite source length")
            ))
        );
    }
    for literal in ["NaNmm", "infmm", "-infrad"] {
        source
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Depth"), literal.into());
        assert!(
            parameter_dimension::<PositiveLength>(&source, &["Depth"]).is_none(),
            "{literal}"
        );
        assert_eq!(source.parameters["Depth"], literal);
    }
}

#[test]
fn explicit_angle_literal_cannot_supply_a_native_length_control() {
    let mut source = feature("thread", Some("1"), 0);
    source.input_class = Some("moCosmeticThread_c".into());
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("D1"),
        "6.2831853071796rad".into(),
    );
    assert!(parameter_dimension::<PositiveLength>(&source, &["D1"]).is_none());
    assert_eq!(source.parameters["D1"], "6.2831853071796rad");
}

#[test]
fn supported_hole_profile_roles_type_the_canonical_unitless_depth() {
    use crate::records::{FeatureContent, FeatureHistory};
    let mut profile = feature("profile", Some("11"), 0);
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    for (name, value) in [
        ("bore", "<MOD-DIAM>2.5"),
        ("drill depth", "7.5"),
        ("major", "<MOD-DIAM>3"),
        ("thread depth", "6"),
        ("angle", "118°"),
    ] {
        profile.parameters.insert(
            cadmpeg_core::text::NonBlankString::new(name).unwrap(),
            value.into(),
        );
        profile.content.push(FeatureContent::Dimension(name.into()));
    }
    let binding = parameter_dimension::<PositiveLength>(&profile, &["drill depth"])
        .expect("the exact supported profile gives this control its length role");
    assert!(matches!(&binding, Dimension::Parameter(_)));
    let parameters = crate::history::parameters::project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![profile],
        }],
    )
    .unwrap();
    assert_eq!(binding.value_from(&parameters).unwrap().get(), 7.5);
    let parameter = parameters
        .iter()
        .find(|parameter| parameter.name == "drill depth")
        .unwrap();
    assert_eq!(parameter.expression.literal_text().as_deref(), Some("7.5"));
    assert_eq!(
        parameter.value,
        Some(ParameterValue::Length(Length::new(7.5).unwrap()))
    );

    let mut unrelated = feature("ordinary-sketch", Some("12"), 0);
    unrelated.kind = "Sketch".into();
    unrelated
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("value"), "7.5".into());
    assert_eq!(
        crate::history::parameters::source_parameter_value(&unrelated, "value"),
        Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(7.5).unwrap()
        ))
    );
}

#[test]
fn operation_real_controls_preserve_integral_literal_spelling_and_quantity() {
    use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
    for (kind, name) in [
        ("Scale", "Factor"),
        ("Scale", "ScaleX"),
        ("Flex", "Factor"),
        ("Helix", "Revolutions"),
    ] {
        let mut source = feature(kind, Some("1"), 0);
        source.kind = kind.into();
        source.xml_tag = kind.into();
        source.parameters.insert(
            cadmpeg_core::text::NonBlankString::new(name).unwrap(),
            "2".into(),
        );
        assert_eq!(
            crate::history::parameters::source_parameter_value(&source, name),
            Some(ParameterValue::Real(FiniteReal::new(2.0).unwrap()))
        );
        assert!(matches!(
            parameter_dimension::<PositiveReal>(&source, &[name]),
            Some(Dimension::Parameter(_))
        ));
        assert_eq!(source.parameters[name], "2");
        source.parameters.insert(
            cadmpeg_core::text::NonBlankString::new(name).unwrap(),
            "-2".into(),
        );
        assert_eq!(
            crate::history::parameters::source_parameter_value(&source, name),
            Some(ParameterValue::Real(FiniteReal::new(-2.0).unwrap()))
        );
        assert!(parameter_dimension::<PositiveReal>(&source, &[name]).is_none());
    }
    let mut ordinary = feature("ordinary", Some("2"), 0);
    ordinary
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Factor"), "2".into());
    assert_eq!(
        crate::history::parameters::source_parameter_value(&ordinary, "Factor"),
        Some(ParameterValue::Integer(2))
    );
}

#[test]
fn sole_authored_extrusion_depth_does_not_retype_auxiliary_parameters() {
    use crate::records::FeatureContent;
    let mut source = feature("extrusion", Some("20"), 1);
    source.xml_tag = "Extrusion".into();
    for (name, value) in [("m", "6.8"), ("aux-1", "1.2"), ("aux-2", "3.4")] {
        source.parameters.insert(
            cadmpeg_core::text::NonBlankString::new(name).unwrap(),
            value.into(),
        );
    }
    source.content = vec![
        FeatureContent::Dimension("m".into()),
        FeatureContent::Dimension("m".into()),
    ];
    assert_eq!(
        crate::history::parameters::source_parameter_value(&source, "m"),
        Some(ParameterValue::Length(Length::new(6.8).unwrap()))
    );
    assert!(matches!(
        parameter_dimension::<PositiveLength>(&source, &["m"]),
        Some(Dimension::Parameter(_))
    ));
    assert_eq!(
        crate::history::parameters::source_parameter_value(&source, "aux-1"),
        Some(ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(1.2).unwrap()
        ))
    );
    source
        .content
        .push(FeatureContent::Dimension("aux-1".into()));
    assert!(parameter_dimension::<PositiveLength>(&source, &["m"]).is_none());
}

#[test]
fn retained_rotation_parameter_does_not_reactivate_a_removed_body_motion() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let mut source = feature("move-body", Some("7"), 0);
    source.kind = "MoveBody".into();
    source.properties.insert(
        cadmpeg_core::nonblank_literal!("Translation"),
        "0mm,0mm,0mm".into(),
    );
    source.parameters.insert(
        cadmpeg_core::nonblank_literal!("Rotation"),
        "0.25rad".into(),
    );
    assert!(matches!(
        super::modify::project_move_body(&source),
        Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
            rotation: None,
            ..
        }))
    ));
    assert_eq!(source.parameters["Rotation"], "0.25rad");
    source.properties.insert(
        cadmpeg_core::nonblank_literal!("RotationOrigin"),
        "0mm,0mm,0mm".into(),
    );
    assert!(
        super::modify::project_move_body(&source).is_none(),
        "partial rotation construction stays malformed"
    );
    source.properties.insert(
        cadmpeg_core::nonblank_literal!("RotationAxis"),
        "0,1,0".into(),
    );
    assert!(
        matches!(super::modify::project_move_body(&source), Some(FeatureDefinition::Operation(FeatureOperation::MoveBody { rotation: Some(rotation), .. })) if matches!(rotation.angle, Dimension::Parameter(_)))
    );
}

#[test]
fn known_invalid_sweep_scale_does_not_become_an_absent_optional_control() {
    let mut source = feature("sweep", Some("7"), 0);
    source.xml_tag = "Sweep".into();
    for value in ["inf", "0", "-2"] {
        source
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Scale"), value.into());
        assert!(
            super::project_sweep(&source, &std::collections::HashMap::new()).is_none(),
            "{value}"
        );
        assert_eq!(source.parameters["Scale"], value);
    }
    for value in ["2", "Factor * 2"] {
        source
            .parameters
            .insert(cadmpeg_core::nonblank_literal!("Scale"), value.into());
        assert!(matches!(
            super::project_sweep(&source, &std::collections::HashMap::new()),
            Some(cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Sweep {
                    scale: Some(Dimension::Parameter(_)),
                    ..
                }
            ))
        ));
    }
}
