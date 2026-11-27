// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::sheet_metal::style;
use crate::record_identity::{Located, RecordTypeId};
use crate::test_support::root_part_ir;
use crate::test_support::test_fixtures::parse;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::ids::IdentityKey;
use cadmpeg_ir::scalar::FiniteReal;

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
    projected_with_losses(styles, factor).0
}

fn projected_with_losses(
    styles: &[Style],
    factor: Option<&str>,
) -> (Vec<SheetMetalRule>, Vec<LossNote>) {
    projected_with_style_factor(styles, factor, StyleFactor::Absent)
}

fn projected_with_style_factor(
    styles: &[Style],
    factor: Option<&str>,
    style_factor: StyleFactor,
) -> (Vec<SheetMetalRule>, Vec<LossNote>) {
    let ir = root_part_ir();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let active = factor.map(|expression| DocumentRule {
        id: "inventor:pmdc:sheet-metal-unfold-rule#fixture-12".into(),
        expression,
    });
    let mut losses = Vec::new();
    let rules = rules(
        &ctx,
        &ir,
        styles,
        |_, reference| match reference.index() {
            1 => Length::new(0.5),
            2 => Length::new(1.25),
            _ => None,
        },
        |_| style_factor,
        active.as_ref(),
        &mut losses,
    )
    .expect("rules");
    (rules, losses)
}

fn codes(losses: &[LossNote]) -> Vec<&str> {
    losses.iter().map(|loss| loss.code.local_code()).collect()
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
    let (rules, losses) = projected_with_losses(&[style(20)], None);
    assert_eq!(rules[0].allowance, None);
    assert_eq!(
        codes(&losses),
        ["feature.sheet-metal-unfold-rule-unresolved"]
    );
    for factor in ["Thickness * 0.5", "table.csv"] {
        let (rules, losses) = projected_with_losses(&[style(20), style(30)], Some(factor));
        assert!(rules.iter().all(|rule| rule.allowance.is_none()));
        assert_eq!(
            codes(&losses),
            ["feature.sheet-metal-unfold-factor-native"],
            "{factor:?}"
        );
    }
}

#[test]
fn sheet_metal_equal_style_k_parameter_keeps_rule_allowance() {
    let (rules, losses) = projected_with_style_factor(
        &[style(20)],
        Some("0.440 ul"),
        StyleFactor::Value(FiniteReal::new(0.44).expect("K")),
    );
    assert_eq!(
        rules[0].allowance,
        Some(BendAllowance::KFactor(Dimension::Literal(
            FiniteReal::new(0.44).expect("K")
        )))
    );
    assert!(losses.is_empty());
}

#[test]
fn sheet_metal_differing_or_unresolved_style_k_parameter_has_no_allowance() {
    for style_factor in [
        StyleFactor::Value(FiniteReal::new(0.5).expect("K")),
        StyleFactor::Unresolved,
    ] {
        let (rules, losses) =
            projected_with_style_factor(&[style(20), style(30)], Some("0.44 ul"), style_factor);
        assert!(rules.iter().all(|rule| rule.allowance.is_none()));
        assert_eq!(
            codes(&losses),
            [
                "feature.sheet-metal-style-factor-conflict",
                "feature.sheet-metal-style-factor-conflict"
            ],
            "{style_factor:?}"
        );
    }
}

#[test]
fn sheet_metal_decimal_k_or_no_style_charges_no_unfold_loss() {
    assert!(projected_with_losses(&[style(20)], Some("0.44 ul"))
        .1
        .is_empty());
    assert!(projected_with_losses(&[], Some("Thickness * 0.5"))
        .1
        .is_empty());
}

/// Project one style through the document path with the given unfold
/// records and an optional style `KFactor` parameter value.
fn projected_document(
    unfold_rules: Vec<unfold::Rule>,
    selections: Vec<unfold::Selection>,
    style_factor: Option<f64>,
) -> (Vec<SheetMetalRule>, Vec<LossNote>) {
    use crate::design::{PmDcParameter, PmDcParameterPayload};
    use crate::pmdc::PmDcContentHeader;
    use cadmpeg_ir::features::parameters::DesignParameter;
    use cadmpeg_ir::features::selections::DistinctMembers;
    use cadmpeg_ir::features::ParameterId;

    let mut ir = root_part_ir();
    let null = PmDcReference::from_packed(0);
    let mut parameters: Vec<PmDcParameter> = Vec::new();
    if let Some(value) = style_factor {
        let value = FiniteReal::new(value).expect("K");
        let raw: PmDcParameter = Located::new(
            PmDcParameterPayload {
                save_version_major: 19,
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
                name: STYLE_FACTOR_NAME.into(),
                name_value: 0,
                unit: null,
                formula: null,
                nominal_value: value,
                model_value: value,
                tolerance: 0,
                terminal_value: 0,
            },
            RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d26)),
            IdentityKey::try_new("fixture").expect("token"),
            70,
        );
        ir.model.parameters.push(DesignParameter {
            product_definition: None,
            id: ParameterId::mint("inventor:design:parameter#kfactor").expect("id"),
            owner: None,
            ordinal: 0,
            name: STYLE_FACTOR_NAME.into(),
            expression: "0.44".into(),
            display: None,
            value: Some(ParameterValue::Real(value)),
            dependencies: DistinctMembers::default(),
            properties: BTreeMap::default(),
            pmi: None,
            native_ref: Some(raw.id()),
        });
        parameters.push(raw);
    }
    let mut inventory = FeatureInventory::default();
    inventory.sheet_metal_styles.push(style(20));
    inventory.sheet_metal_unfold_rules = unfold_rules;
    inventory.sheet_metal_unfold_selections = selections;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &inventory, &parameters, &mut losses).expect("project");
    (ir.design.sheet_metal_rules, losses)
}

fn k_allowance(value: f64) -> BendAllowance {
    BendAllowance::KFactor(Dimension::Literal(FiniteReal::new(value).expect("K")))
}

#[test]
fn sheet_metal_no_unfold_rule_takes_style_k_parameter() {
    let (rules, losses) = projected_document(Vec::new(), Vec::new(), Some(0.44));
    assert_eq!(rules[0].allowance, Some(k_allowance(0.44)));
    assert_eq!(
        codes(&losses),
        [
            "feature.sheet-metal-unfold-rule-unresolved",
            "feature.sheet-metal-style-factor-assumed",
            "feature.sheet-metal-relief-native"
        ]
    );
}

#[test]
fn sheet_metal_no_unfold_rule_without_style_k_parameter_has_no_allowance() {
    let (rules, losses) = projected_document(Vec::new(), Vec::new(), None);
    assert_eq!(rules[0].allowance, None);
    assert_eq!(
        codes(&losses),
        [
            "feature.sheet-metal-unfold-rule-unresolved",
            "feature.sheet-metal-relief-native"
        ]
    );
}

#[test]
fn sheet_metal_several_unfold_entries_take_style_k_parameter() {
    use unfold::tests::{rule, selection, RULE_ORDINAL};
    let entries = [("Unfold_A", RULE_ORDINAL + 1), ("Unfold_B", 40)];
    for selections in [
        vec![selection(&entries)],
        vec![selection(&entries[..1]), selection(&entries[..1])],
    ] {
        let (rules, losses) =
            projected_document(vec![rule("Unfold_A", "0.37 ul")], selections, Some(0.44));
        assert_eq!(rules[0].allowance, Some(k_allowance(0.44)));
        assert_eq!(
            codes(&losses),
            [
                "feature.sheet-metal-unfold-rule-unresolved",
                "feature.sheet-metal-style-factor-assumed",
                "feature.sheet-metal-relief-native"
            ]
        );
    }
}

#[test]
fn sheet_metal_unfold_entry_naming_no_rule_is_unresolved() {
    use unfold::tests::{rule, selection, RULE_ORDINAL};
    for (entry, factor) in [
        (("Unfold_B", RULE_ORDINAL + 1), Some(0.44)),
        (("Unfold_A", RULE_ORDINAL), None),
    ] {
        let (rules, losses) = projected_document(
            vec![rule("Unfold_A", "0.37 ul")],
            vec![selection(&[entry])],
            factor,
        );
        assert_eq!(rules[0].allowance, factor.map(k_allowance));
        let mut expected = vec!["feature.sheet-metal-unfold-rule-unresolved"];
        if factor.is_some() {
            expected.push("feature.sheet-metal-style-factor-assumed");
        }
        expected.push("feature.sheet-metal-relief-native");
        assert_eq!(codes(&losses), expected, "{entry:?}");
    }
}

#[test]
fn sheet_metal_active_rule_and_equal_style_k_parameter_charge_only_the_relief_loss() {
    use unfold::tests::{rule, selection, RULE_ORDINAL};
    let (rules, losses) = projected_document(
        vec![rule("Unfold_A", "0.44 ul")],
        vec![selection(&[("Unfold_A", RULE_ORDINAL + 1)])],
        Some(0.44),
    );
    assert_eq!(rules[0].allowance, Some(k_allowance(0.44)));
    assert_eq!(codes(&losses), ["feature.sheet-metal-relief-native"]);
}

#[test]
fn sheet_metal_no_unfold_rule_with_unresolved_style_k_parameter_is_a_conflict() {
    let (rules, losses) = projected_with_style_factor(&[style(20)], None, StyleFactor::Unresolved);
    assert_eq!(rules[0].allowance, None);
    assert_eq!(
        codes(&losses),
        [
            "feature.sheet-metal-unfold-rule-unresolved",
            "feature.sheet-metal-style-factor-conflict"
        ]
    );
}

#[test]
fn sheet_metal_projection_joins_parameter_literals_and_retains_unread_fields() {
    use crate::design::{PmDcParameter, PmDcParameterPayload};
    use crate::pmdc::PmDcContentHeader;
    use cadmpeg_ir::features::parameters::DesignParameter;
    use cadmpeg_ir::features::selections::DistinctMembers;
    use cadmpeg_ir::features::ParameterId;

    let mut ir = root_part_ir();
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
    let mut losses = Vec::new();
    project(
        &ctx,
        &mut ir,
        &inventory,
        std::slice::from_ref(&raw),
        &mut losses,
    )
    .expect("project");
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

#[test]
fn sheet_metal_binds_projected_length_parameters() {
    use cadmpeg_ir::design::expr::{Expression, ExpressionDialect};
    use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};

    let mut ir = root_part_ir();
    let (raw, legacy) = crate::feature::fillet::tests::parameter(
        0,
        ParameterValue::Length(Length::new(0.75).expect("length")),
    );
    let id = legacy.id.clone();
    ir.design.parameters.push(DesignParameter {
        id: id.clone(),
        scope: ir.model.product_definitions[0].id.clone(),
        owner: None,
        position: None,
        name: cadmpeg_core::text::NonBlankString::new("Thickness"),
        kind: ValueKind::Quantity(Quantity::LENGTH),
        unit: None,
        definition: ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Inventor,
            tokens: Vec::new(),
        }),
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::default(),
        source_settings: BTreeMap::default(),
        native_ref: Some(raw.id()),
    });
    ir.model.parameters.push(legacy);
    let mut inventory = FeatureInventory::default();
    let mut style = style(20);
    style.identity.segment_token = raw.identity.segment_token.clone();
    style.edit(|payload| {
        payload.thickness = PmDcReference::from_packed(1);
        payload.bend_radius = PmDcReference::from_packed(1);
    });
    inventory.sheet_metal_styles.push(style);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &inventory, &[raw], &mut losses).expect("projection");
    assert_eq!(
        ir.design.sheet_metal_rules[0].thickness,
        Some(Dimension::Parameter(id.clone()))
    );
    assert_eq!(
        ir.design.sheet_metal_rules[0].bend.radius,
        Some(Dimension::Parameter(id))
    );
    assert_eq!(ir.design.sheet_metal_rules[0].allowance, None);
}

#[test]
fn relief_keeps_enumeration_pair_and_positive_dimensions() {
    use crate::feature::{sheet_metal::SheetMetalProperty, PmDcFeaturePropertyKind};
    use crate::test_support::sheet_metal::property;
    let mut ir = root_part_ir();
    let mut inventory = FeatureInventory::default();
    let mut style = style(20);
    style.edit(|payload| {
        payload.bend_relief_form = PmDcReference::from_packed(80);
        payload.bend_relief_width = PmDcReference::from_packed(1);
        payload.bend_relief_depth = PmDcReference::from_packed(2);
    });
    inventory.sheet_metal_styles.push(style);
    inventory.properties.push(property(
        80,
        &PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration {
                type_value: 501,
                value: 7,
            },
        },
    ));
    let mut parameters = Vec::new();
    for (index, value) in [(0, 0.25), (1, -0.5)] {
        let (raw, legacy) = crate::feature::fillet::tests::parameter(
            index,
            ParameterValue::Length(Length::new(value).expect("length")),
        );
        parameters.push(raw);
        ir.model.parameters.push(legacy);
    }
    let token = parameters[0].identity.segment_token.clone();
    inventory.sheet_metal_styles[0].identity.segment_token = token.clone();
    inventory.properties[0].identity.segment_token = token;
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    project(&ctx, &mut ir, &inventory, &parameters, &mut Vec::new()).expect("projection");
    let relief = ir.design.sheet_metal_rules[0]
        .bend
        .relief
        .as_ref()
        .expect("relief");
    assert_eq!(
        relief.shape,
        Some(ReliefShape::Native(cadmpeg_core::nonblank_literal!(
            "inventor:bend_relief:501:7"
        )))
    );
    assert_eq!(
        relief.width,
        Some(Dimension::Literal(
            PositiveLength::new(0.25).expect("width")
        ))
    );
    assert_eq!(relief.depth, None);
}

#[test]
fn native_validation_rejects_a_changed_relief_form() {
    use crate::feature::{sheet_metal::SheetMetalProperty, PmDcFeaturePropertyKind};
    use crate::test_support::sheet_metal::property;
    let mut ir = root_part_ir();
    let mut inventory = FeatureInventory::default();
    let mut style = style(20);
    style.edit(|payload| payload.bend_relief_form = PmDcReference::from_packed(80));
    inventory.sheet_metal_styles.push(style);
    inventory.properties.push(property(
        80,
        &PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration {
                type_value: 501,
                value: 7,
            },
        },
    ));
    let sketches = crate::sketch::SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        text_members: Vec::new(),
        issues: Vec::new(),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::design_v11::project(
        &ctx,
        &mut ir,
        &inventory,
        &sketches,
        &[],
        &[],
        &mut Vec::new(),
    )
    .expect("projection");
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("inventor"));
    namespace
        .set_arena(
            &ctx,
            "pm_dc_sheet_metal_styles",
            &inventory.sheet_metal_styles,
        )
        .expect("native styles");
    namespace
        .set_arena(&ctx, "pm_dc_feature_properties", &inventory.properties)
        .expect("native properties");
    assert!(crate::design_v11::tests::validate_from_native(&ctx, &ir)
        .expect("validation")
        .is_empty());
    ir.design.sheet_metal_rules[0]
        .bend
        .relief
        .as_mut()
        .expect("relief")
        .shape = Some(ReliefShape::Native(cadmpeg_core::nonblank_literal!(
        "changed"
    )));
    assert_eq!(
        crate::design_v11::tests::validate_from_native(&ctx, &ir)
            .expect("validation")
            .len(),
        1
    );
}

#[test]
fn style_factor_fallback_binds_the_decoded_unitless_parameter() {
    use cadmpeg_ir::design::expr::{Expression, ExpressionDialect};
    use cadmpeg_ir::design::parameter::{DesignParameter, ParameterDefinition};
    use cadmpeg_ir::design::units::{Quantity, ValueKind};
    let mut ir = root_part_ir();
    let (mut raw, mut legacy) = crate::feature::fillet::tests::parameter(
        0,
        ParameterValue::Real(FiniteReal::new(0.42).expect("factor")),
    );
    raw.edit(|payload| payload.name = STYLE_FACTOR_NAME.into());
    legacy.name = STYLE_FACTOR_NAME.into();
    let id = legacy.id.clone();
    ir.model.parameters.push(legacy);
    ir.design.parameters.push(DesignParameter {
        id: id.clone(),
        scope: ir.model.product_definitions[0].id.clone(),
        owner: None,
        position: None,
        name: cadmpeg_core::text::NonBlankString::new("KFactor"),
        kind: ValueKind::Quantity(Quantity::REAL),
        unit: None,
        definition: ParameterDefinition::Expression(Expression::Source {
            dialect: ExpressionDialect::Inventor,
            tokens: Vec::new(),
        }),
        value: Some(cadmpeg_ir::design::parameter::ParameterValue::Number(
            FiniteReal::new(0.42).expect("factor"),
        )),
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: Some(raw.id()),
    });
    let mut inventory = FeatureInventory::default();
    let mut style = style(20);
    style.identity.segment_token = raw.identity.segment_token.clone();
    inventory.sheet_metal_styles.push(style);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut losses = Vec::new();
    project(&ctx, &mut ir, &inventory, &[raw], &mut losses).expect("projection");
    assert_eq!(
        ir.design.sheet_metal_rules[0].allowance,
        Some(BendAllowance::KFactor(Dimension::Parameter(id)))
    );
    assert!(codes(&losses).contains(&InventorLossCode::FeatureSheetMetalStyleFactorAssumed.code()));
    assert!(codes(&losses).contains(&InventorLossCode::FeatureSheetMetalReliefNative.code()));
    assert_eq!(
        InventorLossCode::FeatureSheetMetalReliefNative.code(),
        "feature.sheet-metal-relief-native"
    );
}

#[test]
fn equal_style_relief_forms_admit_both_boolean_values() {
    use crate::feature::{sheet_metal::SheetMetalProperty, PmDcFeaturePropertyKind};
    use crate::test_support::sheet_metal::ContourFixture;
    use cadmpeg_ir::design::feature::{Feature, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::design::ops::sheet_metal::{SheetMetal, SheetMetalKind};
    use cadmpeg_ir::design::selection::{Anchor, Picks, Selection};
    let fixture = ContourFixture::new();
    let mut inventory = FeatureInventory::default();
    inventory.features.push(fixture.plate.clone());
    inventory.properties = fixture.properties.clone();
    inventory.sheet_metal_styles.push(fixture.style);
    let operation = FeatureOperation::SheetMetal(Box::new(SheetMetal {
        kind: SheetMetalKind::ContourFlange {
            profile: Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: Some(
                    vec!["inventor:pmdc:profile#synthetic".into()]
                        .try_into()
                        .expect("profile"),
                ),
            },
            edges: None,
            width: None,
            extent: None,
            thickness: None,
            bend: BendSpec {
                radius: None,
                radius_side: None,
                relief: None,
            },
            vertex_bends: Vec::new(),
            rule: None,
        },
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    let mut source = root_part_ir();
    source.design.features.push(Feature {
        id: crate::feature::history_feature_id(&fixture.plate.identity).expect("id"),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: Some(fixture.plate.id()),
    });
    for control in [false, true] {
        let index = inventory
            .properties
            .iter()
            .position(|property| property.identity.record_ordinal == 28)
            .expect("control");
        inventory.properties[index].edit(|payload| {
            let PmDcFeaturePropertyKind::Boolean { value, .. } = &mut payload.kind else {
                panic!("Boolean")
            };
            *value = control;
        });
        let mut ir = source.clone();
        let losses = feature_reliefs(&mut ir, &inventory, &[]);
        let FeatureOperation::SheetMetal(operation) = &ir.design.features[0].definition.operation
        else {
            panic!("sheet metal")
        };
        let SheetMetalKind::ContourFlange { bend, .. } = &operation.kind else {
            panic!("contour")
        };
        assert_eq!(
            bend.relief.as_ref().expect("relief").shape,
            Some(ReliefShape::Native(cadmpeg_core::nonblank_literal!(
                "inventor:bend_relief:501:1"
            )))
        );
        assert_eq!(
            losses[0].code.local_code(),
            InventorLossCode::FeatureSheetMetalReliefNative.code()
        );
    }
    let index = inventory
        .properties
        .iter()
        .position(|property| property.identity.record_ordinal == 39)
        .expect("style form");
    inventory.properties[index].edit(|payload| {
        payload.kind = PmDcFeaturePropertyKind::SheetMetal {
            definition: SheetMetalProperty::Enumeration {
                type_value: 501,
                value: 2,
            },
        }
    });
    feature_reliefs(&mut source, &inventory, &[]);
    let FeatureOperation::SheetMetal(operation) = &source.design.features[0].definition.operation
    else {
        panic!("sheet metal")
    };
    let SheetMetalKind::ContourFlange { bend, .. } = &operation.kind else {
        panic!("contour")
    };
    assert_eq!(bend.relief, None);
}
