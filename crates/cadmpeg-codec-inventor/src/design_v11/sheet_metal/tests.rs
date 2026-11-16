// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::sheet_metal::style;
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::test_fixtures::parse;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::ids::IdentityKey;

fn style(ordinal: u32) -> Style {
    let mut bytes = vec![0; style::prefix_len(29).expect("prefix")];
    for value in 1_u32..=15 {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let payload = parse(&bytes, |ctx, view| style::parse(ctx, view, 29)).expect("style");
    Located::new(
        payload,
        RecordTypeId::from_bytes(style::TYPE),
        IdentityKey::try_new("fixture").expect("token"),
        ordinal,
    )
}

fn projected(styles: &[Style], factor: Option<&str>) -> Vec<SheetMetalRule> {
    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(serde_json::from_value(serde_json::json!({
        "id":"inventor:document:product#root", "kind":"part", "bom_properties":{}, "bodies":[], "sketches":[]
    })).expect("part"));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    rules(
        &ctx,
        &ir,
        styles,
        |_, reference| match reference.index() {
            1 => Length::new(0.5),
            2 => Length::new(1.25),
            _ => None,
        },
        factor.and_then(unfold::factor),
    )
    .expect("rules")
}

#[test]
fn sheet_metal_style_states_literal_thickness_radius_and_k() {
    let style = style(20);
    let rules = projected(std::slice::from_ref(&style), Some("0.44 ul"));
    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(
        rule.id.as_str(),
        crate::feature::history_feature_id(&style.identity)
            .expect("identity")
            .as_str()
    );
    assert_eq!(rule.scope.as_str(), "inventor:document:product#root");
    assert_eq!(
        rule.thickness,
        Some(Dimension::Literal(
            PositiveLength::new(0.5).expect("thickness")
        ))
    );
    assert_eq!(
        rule.bend.radius,
        Some(Dimension::Literal(
            PositiveLength::new(1.25).expect("radius")
        ))
    );
    assert_eq!(
        rule.allowance,
        Some(BendAllowance::KFactor(Dimension::Literal(
            FiniteReal::new(0.44).expect("K")
        )))
    );
    assert_eq!(rule.bend.radius_side, None);
    assert_eq!(rule.name, None);
    assert_eq!(rule.native_ref, Some(style.id()));
}

#[test]
fn sheet_metal_without_style_has_no_rule() {
    assert!(projected(&[], Some("0.44 ul")).is_empty());
}

#[test]
fn sheet_metal_two_styles_have_distinct_rules() {
    let rules = projected(&[style(20), style(30)], None);
    assert_eq!(rules.len(), 2);
    assert_ne!(rules[0].id, rules[1].id);
    assert_eq!(rules[0].scope, rules[1].scope);
}

#[test]
fn sheet_metal_unitless_k_text_states_allowance() {
    assert_eq!(
        projected(&[style(20)], Some("0.29"))[0].allowance,
        Some(BendAllowance::KFactor(Dimension::Literal(
            FiniteReal::new(0.29).expect("K")
        )))
    );
}

#[test]
fn sheet_metal_absent_or_equation_k_has_no_allowance() {
    for factor in [None, Some("Thickness * 0.5"), Some("table.csv")] {
        assert_eq!(projected(&[style(20)], factor)[0].allowance, None);
    }
}

#[test]
fn sheet_metal_projection_joins_parameter_literals_and_retains_unread_fields() {
    use crate::design::{PmDcParameter, PmDcParameterPayload};
    use crate::pmdc::PmDcContentHeader;
    use cadmpeg_ir::features::{DesignParameter, DistinctMembers, ParameterId};

    let mut ir = CadIr::empty();
    ir.model.product_definitions.push(serde_json::from_value(serde_json::json!({
        "id":"inventor:document:product#root", "kind":"part", "bom_properties":{}, "bodies":[], "sketches":[]
    })).expect("part"));
    let null = PmDcReference::from_packed(0);
    let raw: PmDcParameter = Located::new(
        PmDcParameterPayload {
            save_version_major: 29,
            header: PmDcContentHeader {
                header_value: 0,
                header_id: 0,
                next: null,
                flags: 0,
                flags_extension: None,
                context: null,
                source_index: 0,
                stamps: Vec::new(),
            },
            name: "Thickness".into(),
            name_value: 0,
            unit: null,
            formula: null,
            nominal_value: FiniteReal::ZERO,
            model_value: FiniteReal::ZERO,
            tolerance: 0,
            terminal_value: 0,
        },
        RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d26)),
        IdentityKey::try_new("fixture").expect("token"),
        0,
    );
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: ParameterId::mint("inventor:design:parameter#thickness").expect("id"),
        owner: None,
        ordinal: 0,
        name: "Thickness".into(),
        expression: "width / 2".into(),
        display: None,
        value: Some(ParameterValue::Length(Length::new(0.75).expect("length"))),
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::default(),
        pmi: None,
        native_ref: Some(raw.id()),
    });
    let mut inventory = FeatureInventory::default();
    inventory.sheet_metal_styles.extend([style(30), style(20)]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    project(&ctx, &mut ir, &inventory, &[raw]).expect("project");
    assert_eq!(ir.design.sheet_metal_rules.len(), 2);
    assert!(ir.design.sheet_metal_rules[0].id < ir.design.sheet_metal_rules[1].id);
    for rule in &ir.design.sheet_metal_rules {
        assert_eq!(
            rule.thickness,
            Some(Dimension::Literal(
                PositiveLength::new(0.75).expect("length")
            ))
        );
        assert_eq!(rule.bend.radius, None);
        assert_eq!(rule.allowance, None);
    }
}
