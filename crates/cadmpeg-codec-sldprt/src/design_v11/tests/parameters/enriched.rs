// SPDX-License-Identifier: Apache-2.0
//! Dimensions that only the decoder's readings of other records state, and
//! what a CADIR write of them keeps.
#![allow(clippy::unwrap_used)]

use super::*;

use cadmpeg_ir::design::expr::{Expression, Neutral, Node};
use cadmpeg_ir::design::parameter::{ParameterDefinition, ParameterOwner, ParameterValue};
use cadmpeg_ir::design::units::{Quantity, ValueKind};

use crate::test_support::cadir::round_trip;
use crate::test_support::pmi::pmi_semantic_payload;

/// A sketch whose history names no dimension, and a PMI dimension record
/// that states `D1@Sketch1` as 25 mm.
fn sketch_with_pmi_dimension() -> CadIr {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Sketch Name="Sketch1" Type="ProfileFeature"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x49,
        "Contents/PMISemanticDataDB",
        &pmi_semantic_payload(),
    ));
    SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap()
        .ir()
        .clone()
}

#[test]
fn a_dimension_only_a_pmi_record_states_is_a_parameter_of_its_feature() {
    let ir = sketch_with_pmi_dimension();
    let legacy = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    let parameter = ir
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == legacy.id)
        .expect("the design restates the dimension under the decoder's id");
    assert_eq!(
        parameter.owner,
        Some(ParameterOwner::Feature(
            feature_named(&ir, "Sketch1").id.clone()
        ))
    );
    assert_eq!(
        parameter
            .name
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("D1")
    );
    assert_eq!(parameter.kind, ValueKind::Quantity(Quantity::LENGTH));
    let Some(ParameterValue::Number(value)) = &parameter.value else {
        panic!("a length value: {parameter:#?}");
    };
    assert!((value.get() - 25.0).abs() < 1e-9, "{parameter:#?}");
    let ParameterDefinition::Expression(expression) = &parameter.definition else {
        panic!("an expression: {parameter:#?}");
    };
    let Expression::Neutral(Neutral(Node::Number { value, unit })) = expression else {
        panic!("a one-literal expression: {expression:#?}");
    };
    let [factor] = unit.0.as_slice() else {
        panic!("a literal with one unit: {unit:#?}");
    };
    let (quantity, scale) = crate::design_v11::parameters::canonical(factor.symbol).unwrap();
    assert_eq!(quantity, Quantity::LENGTH);
    assert!((value.get() * scale - 25.0).abs() < 1e-9, "{expression:#?}");
    assert_no_design_findings(&ir);

    let read = round_trip(&ir);
    assert_eq!(
        read.design
            .parameters
            .iter()
            .find(|written| written.id == legacy.id),
        Some(parameter)
    );
}

/// A dimension that only a `ResolvedFeatures` scalar states keeps the
/// decoder's native reference, the id of that scalar.
#[test]
fn a_dimension_only_a_lane_scalar_states_keeps_its_native_reference() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Fillet Name="Round1" Type="Fillet"/></Keywords>"#,
    ));
    source.extend(make_block(
        0x45,
        "Contents/Config-0-ResolvedFeatures",
        &crate::test_support::history::resolved_features_payload_with_names_relation_and_scalar(
            &[0],
            &["Round1", "D1"],
            "sgPntPntDist",
            0.025,
        ),
    ));
    let ir = decode(&source);
    let legacy = ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "D1")
        .unwrap();
    assert!(legacy.native_ref.is_some(), "{legacy:#?}");
    let parameter = ir
        .design
        .parameters
        .iter()
        .find(|parameter| parameter.id == legacy.id)
        .expect("the design restates the dimension under the decoder's id");
    assert_eq!(parameter.native_ref, legacy.native_ref);
}

/// The decoder's reading of the history may hold a feature the history does
/// not: each parameter still takes its text from the history's record of its
/// own feature.
#[test]
fn a_feature_only_the_reading_holds_leaves_each_parameter_its_own_record_text() {
    let mut ir = decode(&with_history(
        sldprt_with_body(&triangle_body()),
        br#"<Keywords Name="Bracket">
            <Extrusion Name="Boss" Type="BossExtrude" id="7">
                <Dimension Name="D1">12.5mm</Dimension>
            </Extrusion>
        </Keywords>"#,
        &[],
    ));
    let native = crate::test_support::native::load_under(
        ir.native.namespace("sldprt").unwrap(),
        cadmpeg_core::decode::ResourceLimits::default(),
        ir.tolerances.linear,
    )
    .unwrap();
    let histories = native.feature_histories.clone();
    let mut read = histories.clone();
    let boss = read[0]
        .features
        .iter()
        .position(|feature| feature.name == "Boss")
        .unwrap();
    let mut extra = read[0].features[boss].clone();
    extra.id = format!("{}-extra", extra.id);
    extra.name = "Extra".to_owned();
    extra.parameters.clear();
    read[0].features[boss].parameters.insert(
        cadmpeg_core::text::NonBlankString::new("D1".to_owned()).unwrap(),
        "99mm".to_owned(),
    );
    read[0].features.insert(boss, extra);
    let mut empty = read[0].clone();
    empty.id.push_str("-extra");
    empty.features.clear();
    read.insert(0, empty);
    super::super::outside_history::project_from(
        &mut ir,
        &histories,
        &read,
        &native.feature_input_lanes,
        super::super::super::EquationManagers::default(),
    );

    let Some(ParameterValue::Number(value)) = parameter_named(&ir, "D1").value else {
        panic!("a number");
    };
    assert!((value.get() - 12.5).abs() < 1e-9, "{value:?}");
}
