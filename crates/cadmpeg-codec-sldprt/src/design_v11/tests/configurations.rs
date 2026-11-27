// SPDX-License-Identifier: Apache-2.0
//! The design configurations of a part (§5.11) and what a CADIR write of
//! them keeps.
#![allow(clippy::unwrap_used)]

use super::*;

use cadmpeg_ir::design::configuration::DesignConfiguration;
use cadmpeg_ir::design::expr::{Expression, Neutral, Node};
use cadmpeg_ir::design::parameter::ParameterValue;
use cadmpeg_ir::design::units::{Unit, UnitFactor, UnitSymbol};

use crate::test_support::cadir::round_trip;
use crate::test_support::history::resolved_features_payload_with_names_relation_and_scalar;

/// A `Contents/SolidWorks` block whose active configuration is `active`.
fn active_configuration(active: &str) -> Vec<u8> {
    make_block(
        0x43,
        "Contents/SolidWorks",
        format!(r#"<swSolidWorks><swModel swConfigurationName="{active}"/></swSolidWorks>"#)
            .as_bytes(),
    )
}

/// A part with the configurations `Default` and `Long`, no feature, and the
/// active configuration `active`.
fn without_features(active: &str) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let mut source = with_history(
        sldprt_with_body(&triangle_body()),
        br#"<Keywords Name="Bracket">
            <Configuration Name="Default" SourceIndex="0"/>
            <Configuration Name="Long" SourceIndex="1"/>
        </Keywords>"#,
        &[],
    );
    source.extend(active_configuration(active));
    let result = SldprtCodec
        .decode(
            &mut Cursor::new(native_envelope_fixture(&source)),
            &DecodeOptions::default(),
        )
        .unwrap();
    (result.ir().clone(), result.report().losses.clone())
}

fn named<'a>(ir: &'a CadIr, name: &str) -> &'a DesignConfiguration {
    ir.design
        .configurations
        .iter()
        .find(|configuration| configuration.name == name)
        .unwrap()
}

#[test]
fn the_configurations_of_a_part_with_no_feature_are_those_of_its_scope() {
    let (ir, _) = without_features("Default");
    let [scope] = ir.design.scopes.as_slice() else {
        panic!("one scope: {:#?}", ir.design.scopes);
    };
    assert!(scope.roots.is_empty());
    assert!(scope.current.is_empty());
    let bodies = body_ids(&ir);
    assert!(!bodies.is_empty());
    assert_eq!(
        scope
            .unproduced
            .iter()
            .map(|body| body.as_str().to_owned())
            .collect::<Vec<_>>(),
        bodies
    );
    assert_eq!(ir.design.configurations.len(), 2);
    assert_eq!(
        scope.active_configuration.as_ref(),
        Some(&named(&ir, "Default").id)
    );
    assert_no_design_findings(&ir);

    let (omitted, read) = round_trip(&ir);
    for configuration in &ir.design.configurations {
        assert!(
            !omitted
                .iter()
                .any(|message| message.contains(configuration.id.as_str())),
            "{omitted:#?}"
        );
    }
    assert_eq!(read.design.configurations, ir.design.configurations);
}

#[test]
fn configurations_whose_active_one_does_not_resolve_are_named_in_a_note() {
    let (ir, losses) = without_features("Missing");
    assert!(ir.design.configurations.is_empty());
    let [message] = losses
        .iter()
        .filter(|loss| loss.code == SldprtLossCode::DesignConfigurationActiveUnread.kind())
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>()[..]
    else {
        panic!("one note: {losses:#?}");
    };
    for configuration in &ir.model.configurations {
        assert!(message.contains(configuration.id.as_str()), "{message}");
    }
    assert!(message.starts_with("2 configurations"), "{message}");
}

/// A part whose fillet radius `D1` the history states as `text`, with `D2`
/// defined as `D1 * 2`, and whose configurations `Default`, the active one,
/// and `Large` store `D1` as `radii` metres in their own lanes.
fn with_configured_radius(text: &str, radii: [f64; 2]) -> CadIr {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(r#"<Keywords><Configuration Name="Default"/><Configuration Name="Large"/><Fillet Name="Round1" Type="Fillet"><Dimension Name="D1">{text}</Dimension><Dimension Name="D2">D1 * 2</Dimension></Fillet></Keywords>"#).as_bytes(),
    ));
    for (lane, radius) in radii.into_iter().enumerate() {
        source.extend(make_block(
            0x45,
            &format!("Contents/Config-{lane}-ResolvedFeatures"),
            &resolved_features_payload_with_names_relation_and_scalar(
                &[0],
                &["Round1", "D1"],
                "sgPntPntDist",
                radius,
            ),
        ));
    }
    source.extend(active_configuration("Default"));
    decode(&source)
}

#[test]
fn a_dimension_another_configuration_stores_apart_is_an_override_of_it() {
    let ir = with_configured_radius("30mm", [0.025, 0.050]);
    let parameter = |name: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| {
                parameter
                    .name
                    .as_ref()
                    .map(cadmpeg_core::text::NonBlankString::as_str)
                    == Some(name)
            })
            .unwrap()
    };
    let (radius, doubled) = (parameter("D1"), parameter("D2"));
    assert!(named(&ir, "Default").overrides.parameters.is_empty());
    let overrides = &named(&ir, "Large").overrides.parameters;
    assert_eq!(overrides.len(), 2, "{overrides:#?}");

    let literal = &overrides[&radius.id];
    let Some(Expression::Neutral(Neutral(Node::Number { value, unit }))) = &literal.expression
    else {
        panic!("a literal: {literal:#?}");
    };
    assert!((value.get() - 50.0).abs() < 1e-9);
    assert_eq!(
        *unit,
        Unit(vec![UnitFactor {
            symbol: UnitSymbol::Mm,
            power: std::num::NonZeroI8::new(1).unwrap(),
        }])
    );
    let Some(ParameterValue::Number(cached)) = &literal.value else {
        panic!("a number: {literal:#?}");
    };
    assert!((cached.get() - 50.0).abs() < 1e-9);

    let cache = &overrides[&doubled.id];
    assert_eq!(cache.expression, None);
    let Some(ParameterValue::Number(cached)) = &cache.value else {
        panic!("a number: {cache:#?}");
    };
    assert!((cached.get() - 100.0).abs() < 1e-9);
    assert_no_design_findings(&ir);

    let (_, read) = round_trip(&ir);
    assert_eq!(read.design.configurations, ir.design.configurations);
}

#[test]
fn a_configuration_storing_the_active_configurations_values_states_no_override() {
    let ir = with_configured_radius("12.34567mm", [0.012_345_666_666_666_7; 2]);
    let radius = ir
        .design
        .parameters
        .iter()
        .find(|parameter| {
            parameter
                .name
                .as_ref()
                .map(cadmpeg_core::text::NonBlankString::as_str)
                == Some("D1")
        })
        .unwrap();
    let stored = ir
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Large"))
        .unwrap();
    assert!(stored.parameter_values.contains_key(&radius.id));
    assert!(named(&ir, "Default").overrides.parameters.is_empty());
    let overrides = &named(&ir, "Large").overrides.parameters;
    assert!(overrides.is_empty(), "{overrides:#?}");
}
