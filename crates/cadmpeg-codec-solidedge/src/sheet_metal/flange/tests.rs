// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sheet_metal::{
    Flange, FlangeHeight, FlangeWidth, HeightDatum, RadiusSide, ReliefShape, SheetMetal,
    SheetMetalKind,
};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::selection::Picks;

use crate::test_support::{flange_document, variable_table, FlangeSpec};
use crate::SolidEdgeCodec;

/// Agreement of a computed millimetre or radian value with the hand-computed
/// one.
const EPS_VALUE: f64 = 1e-9;

fn decode(specs: &[FlangeSpec]) -> DecodeResult {
    decode_with(specs, &[])
}

fn decode_with(specs: &[FlangeSpec], streams: &[(&str, Vec<u8>)]) -> DecodeResult {
    SolidEdgeCodec
        .decode(
            &mut Cursor::new(flange_document(specs, streams)),
            &DecodeOptions::default(),
        )
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

fn sheet_metal(result: &DecodeResult, index: usize) -> &SheetMetal {
    let FeatureOperation::SheetMetal(sheet) =
        &result.ir().design.features[index].definition.operation
    else {
        panic!("expected a sheet-metal operation");
    };
    sheet
}

fn flange(result: &DecodeResult, index: usize) -> &Flange {
    let SheetMetalKind::Flange(flange) = &sheet_metal(result, index).kind else {
        panic!("expected a flange");
    };
    flange
}

fn millimetres(dimension: Option<&Dimension<cadmpeg_ir::scalar::PositiveLength>>) -> f64 {
    let Some(Dimension::Literal(value)) = dimension else {
        panic!("expected a literal length");
    };
    value.get()
}

#[test]
fn full_edge_flange_states_its_fields() {
    let result = decode(&[FlangeSpec::default()]);
    let flange = flange(&result, 0);
    let Some(FlangeHeight::Distance(Dimension::Literal(height))) = &flange.height else {
        panic!("expected a distance height");
    };
    assert!((height.get() - 10.0).abs() <= EPS_VALUE);
    assert_eq!(flange.height_datum, Some(HeightDatum::OuterSharp));
    let Some(Dimension::Literal(angle)) = &flange.angle else {
        panic!("expected a literal angle");
    };
    assert!((angle.get() - std::f64::consts::FRAC_PI_2).abs() <= EPS_VALUE);
    assert_eq!(flange.width, Some(FlangeWidth::Full));
    assert!((millimetres(flange.bend.radius.as_ref()) - 3.0).abs() <= EPS_VALUE);
    assert_eq!(flange.bend.radius_side, Some(RadiusSide::Inside));
    assert!(flange.bend.relief.is_none());
    let Picks::Named(picks) = &flange.edges.picks else {
        panic!("expected a stated edge");
    };
    assert_eq!(picks.as_slice().len(), 1);
    assert!(sheet_metal(&result, 0).native_options.is_empty());
    assert!(codes(&result)
        .iter()
        .all(|code| !code.starts_with("sheet-metal.flange")));
}

#[test]
fn entered_angle_is_the_half_turn_less_the_turn() {
    let result = decode(&[FlangeSpec {
        angle: std::f64::consts::FRAC_PI_3,
        ..FlangeSpec::default()
    }]);
    let Some(Dimension::Literal(angle)) = &flange(&result, 0).angle else {
        panic!("expected a literal angle");
    };
    assert!((angle.get() - 2.0 * std::f64::consts::FRAC_PI_3).abs() <= EPS_VALUE);
}

#[test]
fn centred_partial_width_shifts_the_later_slots() {
    let result = decode(&[FlangeSpec {
        extent: 135.0,
        partial_width: 0.02,
        relief_type: 52.0,
        relief_width: 0.0015,
        relief_length: 0.003,
        ..FlangeSpec::default()
    }]);
    let flange = flange(&result, 0);
    let Some(FlangeWidth::Centered { width }) = &flange.width else {
        panic!("expected a centred width");
    };
    let Dimension::Literal(width) = width else {
        panic!("expected a literal width");
    };
    assert!((width.get() - 20.0).abs() <= EPS_VALUE);
    let relief = flange.bend.relief.as_ref().unwrap();
    assert_eq!(relief.shape, Some(ReliefShape::Obround));
    assert!((millimetres(relief.width.as_ref()) - 1.5).abs() <= EPS_VALUE);
    assert!((millimetres(relief.depth.as_ref()) - 3.0).abs() <= EPS_VALUE);
    assert!(codes(&result)
        .iter()
        .all(|code| !code.starts_with("sheet-metal.flange")));
}

#[test]
fn square_relief_is_a_native_shape_and_zero_length_states_no_depth() {
    let result = decode(&[FlangeSpec {
        relief_type: 51.0,
        ..FlangeSpec::default()
    }]);
    let relief = flange(&result, 0).bend.relief.as_ref().unwrap();
    let Some(ReliefShape::Native(name)) = &relief.shape else {
        panic!("expected a native shape");
    };
    assert_eq!(name.as_str(), "square");
    assert!((millimetres(relief.width.as_ref()) - 1.0).abs() <= EPS_VALUE);
    assert!(relief.depth.is_none());
}

#[test]
fn unlisted_relief_type_adds_a_loss() {
    let result = decode(&[FlangeSpec {
        relief_type: 77.0,
        ..FlangeSpec::default()
    }]);
    let relief = flange(&result, 0).bend.relief.as_ref().unwrap();
    assert!(relief.shape.is_none());
    assert!(codes(&result).contains(&"sheet-metal.flange-field-unread".to_owned()));
}

#[test]
fn radius_without_a_relation_overrides_the_rule() {
    let result = decode(&[FlangeSpec {
        radius: 0.002,
        radius_bound: false,
        ..FlangeSpec::default()
    }]);
    assert!((millimetres(flange(&result, 0).bend.radius.as_ref()) - 2.0).abs() <= EPS_VALUE);
    let options = &sheet_metal(&result, 0).native_options;
    assert_eq!(options.len(), 1);
    let (key, value) = options.iter().next().unwrap();
    assert_eq!(key.as_str(), "solidedge:bend_radius_overrides_rule");
    assert_eq!(value, &NativeOption::Bool(Dimension::Literal(true)));
}

#[test]
fn chained_flanges_state_one_feature_each() {
    let second = FlangeSpec {
        proxy_len: 575,
        edge_start: [0.04, 0.0, 0.001],
        edge_direction: [0.0, 1.0, 0.0],
        edge_length: 0.028,
        length: 0.015,
        ..FlangeSpec::default()
    };
    let result = decode(&[FlangeSpec::default(), second]);
    assert_eq!(result.ir().design.features.len(), 2);
    let first = flange(&result, 0);
    let second = flange(&result, 1);
    let Some(FlangeHeight::Distance(Dimension::Literal(height))) = &second.height else {
        panic!("expected a distance height");
    };
    assert!((height.get() - 15.0).abs() <= EPS_VALUE);
    assert_ne!(first.edges, second.edges);
    assert!(matches!(second.edges.picks, Picks::Named(_)));
}

#[test]
fn flange_names_the_rule_of_the_document() {
    let records: Vec<(u32, f64)> = vec![
        (2, 7800.0),
        (3, 0.001),
        (4, 0.001),
        (5, 0.001),
        (6, 0.001),
        (7, 0.0),
        (8, 0.33),
        (9, 0.005),
        (10, 0.0001),
        (11, 0.0),
        (12, 0.0),
        (13, 0.0),
    ];
    let table = variable_table(12, &records);
    let result = decode_with(
        &[FlangeSpec::default()],
        &[("CustomPropertyVariableInfo", table)],
    );
    assert_eq!(
        flange(&result, 0).rule.as_ref(),
        Some(&result.ir().design.sheet_metal_rules[0].id)
    );
}

#[test]
fn side_other_than_two_adds_a_loss() {
    let result = decode(&[FlangeSpec {
        side: 1.0,
        ..FlangeSpec::default()
    }]);
    assert!(codes(&result).contains(&"sheet-metal.flange-field-unread".to_owned()));
}

#[test]
fn unreadable_edge_adds_a_loss_and_states_an_unread_edge() {
    let result = decode(&[FlangeSpec {
        edge_direction: [2.0, 0.0, 0.0],
        ..FlangeSpec::default()
    }]);
    assert!(matches!(flange(&result, 0).edges.picks, Picks::Unread));
    assert!(codes(&result).contains(&"sheet-metal.flange-field-unread".to_owned()));
}
