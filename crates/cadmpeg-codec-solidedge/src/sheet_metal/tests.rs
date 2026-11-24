// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{BendAllowance, NormalSide, SheetMetalKind};
use cadmpeg_ir::design::options::NativeOption;

use crate::test_support::{
    extent_document_with_streams, variable_table, Member2d, ProfileFrame, Slot, Slots,
};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre value with the hand-computed one.
const EPS_VALUE: f64 = 1e-9;

const XY: ProfileFrame = ProfileFrame {
    origin: [0.0, 0.0, 0.0],
    normal: [0.0, 0.0, 1.0],
    x_axis: [1.0, 0.0, 0.0],
};

fn decode(bytes: Vec<u8>) -> DecodeResult {
    SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap()
}

fn codes(result: &DecodeResult) -> Vec<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect()
}

fn square() -> Vec<Member2d> {
    vec![
        Member2d::Line([0.0, 0.0, 0.01, 0.0]),
        Member2d::Line([0.01, 0.0, 0.01, 0.01]),
        Member2d::Line([0.01, 0.01, 0.0, 0.01]),
        Member2d::Line([0.0, 0.01, 0.0, 0.0]),
    ]
}

/// The records of a sheet-metal document: thickness 2 mm, bend radius 3 mm,
/// relief width 1 mm and length 4 mm, neutral factor 0.4, the lengths of
/// indices 9 to 12, and bend method `method`.
fn records(method: f64) -> Vec<(u32, f64)> {
    vec![
        (2, 7800.0),
        (3, 0.001),
        (4, 0.002),
        (5, 0.003),
        (6, 0.001),
        (7, 0.004),
        (8, 0.4),
        (9, 0.005),
        (10, 0.0001),
        (11, 0.006),
        (12, 0.007),
        (13, method),
    ]
}

fn base_tab(thickness: f64, side: f64, table: Option<Vec<u8>>) -> DecodeResult {
    let streams: Vec<(&str, Vec<u8>)> = table
        .map(|table| ("CustomPropertyVariableInfo", table))
        .into_iter()
        .collect();
    decode(extent_document_with_streams(
        "JMBaseTabFunction",
        &XY,
        &square(),
        Slots {
            kind: Slot::Double(thickness),
            side: Slot::Double(side),
            third: Slot::Empty,
            angle: Slot::Empty,
        },
        &streams,
    ))
}

fn length_option(options: &cadmpeg_ir::design::options::NativeOptions, name: &str) -> f64 {
    match options
        .iter()
        .find(|(key, _)| key.as_str() == format!("solidedge:{name}").as_str())
        .unwrap()
        .1
    {
        NativeOption::Length(Dimension::Literal(length)) => length.get(),
        other => panic!("expected a literal length, found {other:?}"),
    }
}

#[test]
fn twelve_records_state_a_rule() {
    let result = base_tab(0.002, 2.0, Some(variable_table(12, &records(0.0))));
    let rules = &result.ir().design.sheet_metal_rules;
    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(
        rule.native_ref.as_deref(),
        Some("CustomPropertyVariableInfo")
    );
    let Some(Dimension::Literal(thickness)) = &rule.thickness else {
        panic!("expected a literal thickness");
    };
    assert!((thickness.get() - 2.0).abs() <= EPS_VALUE);
    let Some(Dimension::Literal(radius)) = &rule.bend.radius else {
        panic!("expected a literal radius");
    };
    assert!((radius.get() - 3.0).abs() <= EPS_VALUE);
    assert!(rule.bend.radius_side.is_none());
    let relief = rule.bend.relief.as_ref().unwrap();
    let Some(Dimension::Literal(width)) = &relief.width else {
        panic!("expected a literal relief width");
    };
    assert!((width.get() - 1.0).abs() <= EPS_VALUE);
    let Some(BendAllowance::KFactor(Dimension::Literal(factor))) = &rule.allowance else {
        panic!("expected a K-factor allowance");
    };
    assert!((factor.get() - 0.4).abs() <= EPS_VALUE);
    assert!((length_option(&rule.native_options, "minimum_arc_length") - 5.0).abs() <= EPS_VALUE);
    assert!((length_option(&rule.native_options, "bend_deduction") - 6.0).abs() <= EPS_VALUE);
    assert!((length_option(&rule.native_options, "bend_allowance") - 7.0).abs() <= EPS_VALUE);
    assert!((length_option(&rule.native_options, "relief_length") - 4.0).abs() <= EPS_VALUE);
    assert!(
        (length_option(&rule.native_options, "deviational_tolerance") - 0.1).abs() <= EPS_VALUE
    );
    let method = rule
        .source_settings
        .iter()
        .find(|(key, _)| key.as_str() == "solidedge:bend_method")
        .unwrap()
        .1;
    assert_eq!(method, &NativeOption::Integer(Dimension::Literal(0)));
    assert!(!codes(&result).contains(&"sheet-metal.bend-method-unread".to_owned()));
}

#[test]
fn bend_deduction_states_no_allowance() {
    let result = base_tab(0.002, 2.0, Some(variable_table(12, &records(1.0))));
    let rule = &result.ir().design.sheet_metal_rules[0];
    assert!(rule.allowance.is_none());
    assert!(codes(&result).contains(&"sheet-metal.bend-method-unread".to_owned()));
}

#[test]
fn part_table_states_no_rule() {
    let result = base_tab(
        0.002,
        2.0,
        Some(variable_table(2, &[(2, 7800.0), (3, 0.001)])),
    );
    assert!(result.ir().design.sheet_metal_rules.is_empty());
    assert!(!codes(&result).contains(&"sheet-metal.rule-unread".to_owned()));
}

#[test]
fn truncated_table_adds_a_loss() {
    let result = base_tab(0.002, 2.0, Some(variable_table(12, &records(0.0)[..5])));
    assert!(result.ir().design.sheet_metal_rules.is_empty());
    assert!(codes(&result).contains(&"sheet-metal.rule-unread".to_owned()));
}

#[test]
fn mismatched_key_adds_a_loss() {
    let mut table = variable_table(12, &records(0.0));
    // Index 2 keyed as `2 << 24` instead of `1 << 24 | 2`.
    table[8..12].copy_from_slice(&(2_u32 << 24).to_le_bytes());
    let result = base_tab(0.002, 2.0, Some(table));
    assert!(result.ir().design.sheet_metal_rules.is_empty());
    assert!(codes(&result).contains(&"sheet-metal.rule-unread".to_owned()));
}

#[test]
fn base_tab_states_a_wall() {
    let result = base_tab(0.002, 2.0, Some(variable_table(12, &records(0.0))));
    let feature = &result.ir().design.features[0];
    let FeatureOperation::SheetMetal(sheet) = &feature.definition.operation else {
        panic!("expected a sheet-metal operation");
    };
    let SheetMetalKind::Wall {
        thickness,
        side,
        rule,
        ..
    } = &sheet.kind
    else {
        panic!("expected a wall");
    };
    let Some(Dimension::Literal(thickness)) = thickness else {
        panic!("expected a literal thickness");
    };
    assert!((thickness.get() - 2.0).abs() <= EPS_VALUE);
    assert_eq!(*side, Some(NormalSide::Along));
    assert_eq!(
        rule.as_ref(),
        Some(&result.ir().design.sheet_metal_rules[0].id)
    );
    assert!(!codes(&result).contains(&"sheet-metal.base-tab-unread".to_owned()));
}

#[test]
fn side_one_is_against_the_normal() {
    let result = base_tab(0.002, 1.0, None);
    let FeatureOperation::SheetMetal(sheet) = &result.ir().design.features[0].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    let SheetMetalKind::Wall { side, rule, .. } = &sheet.kind else {
        panic!("expected a wall");
    };
    assert_eq!(*side, Some(NormalSide::Against));
    assert!(rule.is_none());
}

#[test]
fn bad_side_keeps_the_feature_unresolved() {
    let result = base_tab(0.002, 3.0, None);
    assert!(matches!(
        &result.ir().design.features[0].definition.operation,
        FeatureOperation::Native { native_options, .. }
            if native_options.keys().any(|key| key.as_str() == "solidedge:profile")
    ));
    assert!(codes(&result).contains(&"sheet-metal.base-tab-unread".to_owned()));
}
