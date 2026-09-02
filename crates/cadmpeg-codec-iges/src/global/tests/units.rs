// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    code_count, point_file_with_field, report_code_count, resolve_global_fields, strict_options,
    valid_global_fields,
};
use crate::loss::IgesLossCode;
use crate::test_support::{fixed_ascii_with_global, point_file_with_global};
use crate::IgesCodec;

const DELEGATED_LENGTH_SYMBOLS: [(&str, f64); 25] = [
    ("A", 0.000_000_1_f64),
    ("in", 25.4),
    ("ft", 304.8),
    ("mi", 1_609_344.0),
    ("mil", 0.0254),
    ("uin", 0.000_025_4),
    ("yd", 914.4),
    ("nmi", 1_852_000.0),
    ("dam", 10_000.0),
    ("hm", 100_000.0),
    ("km", 1_000_000.0),
    ("Mm", 1_000_000_000.0),
    ("Gm", 1_000_000_000_000.0),
    ("Tm", 1_000_000_000_000_000.0),
    ("Pm", 1_000_000_000_000_000_000.0),
    ("Em", 1_000_000_000_000_000_000_000.0),
    ("m", 1_000.0),
    ("dm", 100.0),
    ("cm", 10.0),
    ("mm", 1.0),
    ("um", 0.001),
    ("nm", 0.000_001),
    ("pm", 0.000_000_001),
    ("fm", 0.000_000_000_001),
    ("am", 0.000_000_000_000_001),
];

#[test]
fn delegated_length_symbols_use_exact_case_sensitive_factors() {
    for (name, expected) in DELEGATED_LENGTH_SYMBOLS {
        let mut fields = valid_global_fields();
        fields[13] = "3".into();
        fields[14] = format!("{}H{name}", name.len());
        let (parsed, losses) = resolve_global_fields(&fields);
        let actual = parsed.length_context().unwrap().length_factor_mm();
        let tolerance = f64::EPSILON * 64.0 * expected.abs().max(1.0);
        assert!((actual - expected).abs() <= tolerance, "{name}: {actual}");
        assert!(losses.is_empty(), "{name}: {losses:#?}");
    }

    let uppercased = DELEGATED_LENGTH_SYMBOLS
        .into_iter()
        .map(|(name, _)| name.to_uppercase())
        .filter(|name| {
            DELEGATED_LENGTH_SYMBOLS
                .into_iter()
                .all(|(symbol, _)| symbol != name.as_str())
        })
        .chain(std::iter::once("INCH".to_owned()));
    for name in uppercased {
        let mut fields = valid_global_fields();
        fields[13] = "3".into();
        fields[14] = format!("{}H{name}", name.len());
        let (parsed, losses) = resolve_global_fields(&fields);
        assert!(parsed.length_context().is_none(), "{name}");
        assert_eq!(
            parsed.units_name().as_deref(),
            Some(name.as_str()),
            "{name}"
        );
        assert_eq!(losses.len(), 1, "{name}: {losses:#?}");
        assert_eq!(
            code_count(&losses, IgesLossCode::GlobalLengthUnitUnresolved),
            1,
            "{name}"
        );
    }

    let mut fields = valid_global_fields();
    fields[13] = "2".into();
    fields[14] = "7Hgarbage".into();
    let (parsed, losses) = resolve_global_fields(&fields);
    assert_eq!(parsed.length_context().unwrap().length_factor_mm(), 1.0);
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn flag_three_units_require_a_nonempty_name_and_accept_delegated_symbols() {
    for (units_name, expected) in [("2Hmm", 1.0_f64), ("3Hnmi", 1_852_000.0)] {
        let mut fields = valid_global_fields();
        fields[13] = "3".into();
        fields[14] = units_name.into();
        let (parsed, losses) = resolve_global_fields(&fields);
        assert_eq!(parsed.units_name().as_deref(), Some(&units_name[2..]));
        let actual = parsed.length_context().unwrap().length_factor_mm();
        let tolerance = f64::EPSILON * 64.0 * expected.max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{units_name}: {actual}"
        );
        assert!(losses.is_empty(), "{units_name}: {losses:#?}");
    }

    let mut fields = valid_global_fields();
    fields[13] = "3".into();
    fields[14] = "0H".into();
    let (parsed, losses) = resolve_global_fields(&fields);
    assert!(parsed.length_context().is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalLengthUnitUnresolved),
        1
    );
}

#[test]
fn minimum_resolution_falls_back_to_zero_when_absent_or_negative() {
    for (resolution, expected) in [("", 1_usize), ("-0.001", 1)] {
        let global = format!(
            "1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,{resolution},1,1Ha,1Ho,11,0,0H,0H;"
        );
        let (parsed, losses) = crate::global::parse(
            &crate::card::scan(&fixed_ascii_with_global(global.as_bytes())).unwrap(),
        )
        .unwrap();

        assert_eq!(
            parsed.length_context().unwrap().minimum_resolution_mm(),
            0.0
        );
        assert_eq!(losses.len(), expected, "{resolution:?}: {losses:#?}");
        assert_eq!(
            code_count(&losses, IgesLossCode::GlobalSemanticContextSubstituted),
            expected,
            "{resolution:?}"
        );
    }
}

#[test]
fn trailing_exponent_decimal_recovers_global_real_without_substitution() {
    let mut fields = valid_global_fields();
    fields[18] = "2e-06.".into();

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(
        parsed.length_context().unwrap().minimum_resolution_mm(),
        2e-6
    );
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalNumericSyntaxRecovered),
        1
    );
    assert!(losses[0].message.contains("2e-06."));
    assert!(losses[0].message.contains("recovered finite value"));
}

#[test]
fn trailing_decimal_recovery_requires_an_exponent_prefix() {
    let mut fields = valid_global_fields();
    fields[18] = "2e-06..".into();

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(
        parsed.length_context().unwrap().minimum_resolution_mm(),
        0.0
    );
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalSemanticContextSubstituted),
        1
    );
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalNumericSyntaxRecovered),
        0
    );
}

#[test]
fn recovered_global_real_is_strictly_reported_as_noncanonical() {
    let bytes = point_file_with_field(18, "2e-06.");
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalNumericSyntaxRecovered),
        1
    );
    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => assert_eq!(
            rejection.loss().code.as_str(),
            IgesLossCode::GlobalNumericSyntaxRecovered.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn an_unknown_flag_three_unit_name_suppresses_geometry_and_charges_one_length_loss() {
    let bytes = point_file_with_global(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,3,7Hfurlong,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    );

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["native_units"],
        "furlong"
    );
    assert!(result.ir().model.points.is_empty());
    assert!(!result.report().geometry_transferred());
    assert!(!result
        .ir()
        .native
        .namespace("iges")
        .expect("native iges namespace")
        .arenas
        .is_empty());
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalLengthUnitUnresolved),
        1,
        "{:#?}",
        result.report().losses
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .find(|loss| loss.code == IgesLossCode::GlobalLengthUnitUnresolved.kind())
            .unwrap()
            .severity,
        cadmpeg_ir::report::Severity::Blocking
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &strict_options(false))
        .unwrap_err();
    match error {
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => assert_eq!(
            rejection.loss().code.as_str(),
            IgesLossCode::GlobalLengthUnitUnresolved.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }

    let container = IgesCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        report_code_count(container.report(), IgesLossCode::GlobalLengthUnitUnresolved),
        1
    );
    assert!(container.ir().model.points.is_empty());
}

#[test]
fn a_zero_model_scale_suppresses_geometry_and_charges_one_length_loss() {
    let bytes = point_file_with_field(12, "0.0");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.points.is_empty());
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalLengthUnitUnresolved),
        1,
        "{:#?}",
        result.report().losses
    );

    let container = IgesCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        report_code_count(container.report(), IgesLossCode::GlobalLengthUnitUnresolved),
        1
    );
}
