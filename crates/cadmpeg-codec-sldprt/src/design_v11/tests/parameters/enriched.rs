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
    if let ParameterDefinition::Expression(expression) = &parameter.definition {
        let Expression::Neutral(Neutral(Node::Number { value, unit })) = expression else {
            panic!("a one-literal expression: {expression:#?}");
        };
        let [factor] = unit.0.as_slice() else {
            panic!("a literal with one unit: {unit:#?}");
        };
        let (quantity, scale) = crate::design_v11::parameters::canonical(factor.symbol).unwrap();
        assert_eq!(quantity, Quantity::LENGTH);
        assert!((value.get() * scale - 25.0).abs() < 1e-9, "{expression:#?}");
    }
    assert_no_design_findings(&ir);

    let (omitted, read) = round_trip(&ir);
    assert!(
        !omitted
            .iter()
            .any(|message| message.contains(legacy.id.as_str())),
        "{omitted:#?}"
    );
    assert!(read
        .design
        .parameters
        .iter()
        .any(|written| written.id == legacy.id));
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
