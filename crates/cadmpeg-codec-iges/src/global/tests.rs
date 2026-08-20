// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

fn valid_global_fields() -> Vec<String> {
    [
        "1H,",
        "1H;",
        "7Hproduct",
        "8Hpart.igs",
        "7Hcadmpeg",
        "3H0.1",
        "32",
        "38",
        "6",
        "308",
        "15",
        "0H",
        "1.0",
        "2",
        "2HMM",
        "1",
        "1.0",
        "15H20260714.000000",
        "0.001",
        "1000.0",
        "6Hauthor",
        "3Horg",
        "11",
        "0",
        "0H",
        "0H",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

type ParsedGlobal = (
    crate::global::ResolvedGlobal,
    Vec<cadmpeg_ir::report::LossNote>,
);

fn parse_global_fields(fields: &[String]) -> Result<ParsedGlobal, CodecError> {
    let mut global = fields.join(",");
    global.push(';');
    let bytes = fixed_ascii_with_global(global.as_bytes());
    crate::global::parse(&crate::card::scan(&bytes)?)
}

fn resolve_global_fields(fields: &[String]) -> ParsedGlobal {
    parse_global_fields(fields).unwrap()
}

fn code_count(losses: &[cadmpeg_ir::report::LossNote], code: IgesLossCode) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

fn report_code_count(report: &cadmpeg_ir::report::DecodeReport, code: IgesLossCode) -> usize {
    code_count(&report.losses, code)
}

fn point_file_with_version_flag(flag: &str) -> Vec<u8> {
    point_file_with_global(
        format!(
            "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{flag},0,0H,0H;"
        )
        .as_bytes(),
    )
}

fn dialect_losses(report: &cadmpeg_ir::report::DecodeReport) -> usize {
    report_code_count(report, IgesLossCode::SourceDialectUnverified)
}

fn point_file_with_field(index: usize, value: &str) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[index] = value.to_owned();
    let mut global = fields.join(",");
    global.push(';');
    point_file_with_global(global.as_bytes())
}

fn point_file_with_delimiters(parameter: char, record: char) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[0] = format!("1H{parameter}");
    fields[1] = format!("1H{record}");
    let global = format!("{}{record}", fields.join(&parameter.to_string()));
    let mut bytes = fixed_ascii_with_global(global.as_bytes());
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        format!("116{parameter}1.0{parameter}2.0{parameter}3.0{record}").as_bytes(),
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(72);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn strict_options(container_only: bool) -> DecodeOptions {
    let mut options = DecodeOptions {
        container_only,
        ..DecodeOptions::default()
    };
    options.policy.mode = DecodeMode::Strict;
    options
}

fn fixed_ascii_with_global_chunks(chunks: &[&[u8]]) -> Vec<u8> {
    let mut bytes = card(b"original fixture", b'S', 1);
    let cards = chunks
        .iter()
        .flat_map(|chunk| chunk.chunks(72))
        .collect::<Vec<_>>();
    for (index, chunk) in cards.iter().enumerate() {
        bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
    }
    bytes.extend(card(
        format!("S0000001G{:07}D0000000P0000000", cards.len()).as_bytes(),
        b'T',
        1,
    ));
    bytes
}

#[test]
fn inspect_parses_alternate_delimiters_and_cross_card_hollerith() {
    let product = "p".repeat(70);
    let global = format!(
        "1H^^1H!^70H{product}^8Hpart.igs^7Hcadmpeg^3H0.1^32^38^6^308^15^0H^1.0^2^2HMM^1^1.0^15H20260714.000000^0.001^1000.0^6Hauthor^3Horg^11^0^0H^0H!"
    );
    let bytes = fixed_ascii_with_global(global.as_bytes());

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"parameter_delimiter=^".into()));
    assert!(summary.notes.contains(&"record_delimiter=!".into()));
    assert!(summary.notes.contains(&format!("sender_product={product}")));
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(summary.notes.contains(&"units=MM".into()));
}

#[test]
fn global_defaults_apply_only_to_omitted_fields() {
    let global =
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,,,2HIN,1,1.0,15H20260714.000000,0,1,1Ha,1Ho,,0,0H,0H;";
    let bytes = fixed_ascii_with_global(global);
    let scan = crate::card::scan(&bytes).unwrap();
    let (parsed, losses) = crate::global::parse(&scan).unwrap();
    let context = parsed.length_context().unwrap();

    assert_eq!(context.length_factor_mm(), 25.4);
    assert_eq!(parsed.units_flag(), Some(1));
    assert_eq!(parsed.declared_version_flag(), 3);
    assert_eq!(context.minimum_resolution_mm(), 0.0);
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn global_card_padding_is_ignored_outside_hollerith_values() {
    let bytes = fixed_ascii_with_global_chunks(&[
        b"1H,,1H;,7Hproduct,8Hpart.igs,",
        b"7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ]);
    let (parsed, _) = crate::global::parse(&crate::card::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product().as_deref(), Some("product"));
    assert_eq!(parsed.native_file_name().as_deref(), Some("part.igs"));
}

#[test]
fn global_card_padding_does_not_remove_hollerith_payload_spaces() {
    let bytes = fixed_ascii_with_global_chunks(&[
        b"1H,,1H;,3Hab ",
        b",8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ]);
    let (parsed, _) = crate::global::parse(&crate::card::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product().as_deref(), Some("ab "));
}

#[test]
fn global_field_categories_apply_defaults_and_require_no_default_fields() {
    let mut fields = valid_global_fields();
    for index in [11, 12, 13, 14, 15, 19, 20, 21, 22, 23, 24, 25] {
        fields[index].clear();
    }

    let (parsed, losses) = resolve_global_fields(&fields);
    let context = parsed.length_context().unwrap();
    assert_eq!(context.length_factor_mm(), 25.4);
    assert_eq!(parsed.units_flag(), Some(1));
    assert_eq!(parsed.declared_version_flag(), 3);
    assert!((context.minimum_resolution_mm() - 0.0254).abs() <= f64::EPSILON * 64.0);
    assert_eq!(parsed.maximum_coordinate_mm(), Some(0.0));
    assert!(losses.is_empty(), "{losses:#?}");

    for (index, expected) in [
        (3, None),
        (4, None),
        (5, None),
        (6, None),
        (7, None),
        (8, Some(IgesLossCode::GlobalSemanticContextSubstituted)),
        (9, None),
        (10, Some(IgesLossCode::GlobalSemanticContextSubstituted)),
        (16, Some(IgesLossCode::LineWeightScaleUnavailable)),
        (17, None),
        (18, None),
    ] {
        let mut fields = valid_global_fields();
        fields[index].clear();
        let (parsed, losses) = resolve_global_fields(&fields);
        match expected {
            Some(code) => {
                assert_eq!(losses.len(), 1, "field {}: {losses:#?}", index + 1);
                assert_eq!(code_count(&losses, code), 1, "field {}", index + 1);
            }
            None => assert!(losses.is_empty(), "field {}: {losses:#?}", index + 1),
        }
        assert!(
            parsed.length_context().is_some(),
            "field {} suppressed the length factor",
            index + 1
        );
    }
}

#[test]
fn omitted_sender_product_is_retained_as_null_for_reader_compatibility() {
    let mut fields = valid_global_fields();
    fields[2].clear();

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.sender_product(), None);
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn malformed_global_values_select_the_matrix_disposition_and_loss() {
    let metadata = IgesLossCode::GlobalMetadataFieldUnusable;
    let semantic = IgesLossCode::GlobalSemanticContextSubstituted;
    let length = IgesLossCode::GlobalLengthUnitUnresolved;
    let presentation = IgesLossCode::LineWeightScaleUnavailable;
    for (index, value, expected) in [
        (2, "1", metadata),
        (3, "1", metadata),
        (4, "1", metadata),
        (5, "1", metadata),
        (6, "1Hx", metadata),
        (7, "1Hx", metadata),
        (8, "1Hx", semantic),
        (9, "1Hx", metadata),
        (10, "1Hx", semantic),
        (11, "1", metadata),
        (12, "1Hx", length),
        (13, "1Hx", length),
        (14, "1", metadata),
        (15, "1Hx", presentation),
        (16, "1Hx", presentation),
        (17, "1", metadata),
        (18, "1Hx", semantic),
        (19, "1Hx", metadata),
        (20, "1", metadata),
        (21, "1", metadata),
        (23, "1Hx", metadata),
        (24, "1", metadata),
        (25, "1", metadata),
    ] {
        let mut fields = valid_global_fields();
        fields[index] = value.to_owned();
        let (parsed, losses) = resolve_global_fields(&fields);
        assert_eq!(losses.len(), 1, "field {}: {losses:#?}", index + 1);
        assert_eq!(code_count(&losses, expected), 1, "field {}", index + 1);
        assert_eq!(
            parsed.length_context().is_none(),
            expected == length,
            "field {} length suppression",
            index + 1
        );
    }

    let mut fields = valid_global_fields();
    fields[22] = "1Hx".into();
    let (parsed, losses) = resolve_global_fields(&fields);
    assert!(losses.is_empty(), "{losses:#?}");
    assert_eq!(parsed.declared_version_flag(), 3);
    assert!(parsed.unreadable_version_declaration().is_some());

    let mut fields = valid_global_fields();
    fields.push("0H".into());
    let (_, losses) = resolve_global_fields(&fields);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalNoncanonicalFraming),
        1
    );
}

#[test]
fn version_flags_clamp_unrecognized_values() {
    for (value, declared, expected) in [
        ("-1", -1, 3),
        ("0", 0, 3),
        ("12", 12, 11),
        ("99", 99, 11),
        ("6", 6, 6),
    ] {
        let mut fields = valid_global_fields();
        fields[22] = value.into();
        let (parsed, _) = resolve_global_fields(&fields);
        assert_eq!(parsed.declared_version_flag(), declared);
        assert_eq!(parsed.effective_version_flag(), expected);
    }
}

#[test]
fn delegated_length_symbols_use_exact_case_sensitive_factors() {
    for (name, expected) in [
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
    ] {
        let mut fields = valid_global_fields();
        fields[13] = "3".into();
        fields[14] = format!("{}H{name}", name.len());
        let (parsed, losses) = resolve_global_fields(&fields);
        let actual = parsed.length_context().unwrap().length_factor_mm();
        let tolerance = f64::EPSILON * 64.0 * expected.abs().max(1.0);
        assert!((actual - expected).abs() <= tolerance, "{name}: {actual}");
        assert!(losses.is_empty(), "{name}: {losses:#?}");
    }

    for name in [
        "IN", "INCH", "MM", "FT", "MI", "M", "KM", "MIL", "UM", "CM", "UIN", "NMI",
    ] {
        let mut fields = valid_global_fields();
        fields[13] = "3".into();
        fields[14] = format!("{}H{name}", name.len());
        let (parsed, losses) = resolve_global_fields(&fields);
        assert!(parsed.length_context().is_none(), "{name}");
        assert_eq!(parsed.units_name().as_deref(), Some(name), "{name}");
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
fn global_timestamps_and_scalar_ranges_follow_the_specification() {
    for (timestamp, valid) in [
        ("15H20260714.000000", true),
        ("13H240714.000000", true),
        ("15H20261314.000000", false),
        ("15H20260714.240000", false),
    ] {
        let mut fields = valid_global_fields();
        fields[17] = timestamp.into();
        let (_, losses) = resolve_global_fields(&fields);
        assert_eq!(
            code_count(&losses, IgesLossCode::GlobalMetadataFieldUnusable),
            usize::from(!valid),
            "{timestamp}"
        );
    }

    let mut fields = valid_global_fields();
    fields[24] = "15H20260714.000000".into();
    let (_, losses) = resolve_global_fields(&fields);
    assert!(losses.is_empty(), "{losses:#?}");

    for (index, value, expected) in [
        (15, "0", IgesLossCode::LineWeightScaleUnavailable),
        (19, "-1", IgesLossCode::GlobalMetadataFieldUnusable),
        (23, "8", IgesLossCode::GlobalMetadataFieldUnusable),
    ] {
        let mut fields = valid_global_fields();
        fields[index] = value.into();
        let (_, losses) = resolve_global_fields(&fields);
        assert_eq!(losses.len(), 1, "field {}: {losses:#?}", index + 1);
        assert_eq!(code_count(&losses, expected), 1, "field {}", index + 1);
    }
}

#[test]
fn malformed_global_integer_does_not_select_its_default() {
    let global = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,1.0,2.,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;";
    let (parsed, losses) =
        crate::global::parse(&crate::card::scan(&fixed_ascii_with_global(global)).unwrap())
            .unwrap();

    assert_eq!(parsed.units_flag(), None);
    assert!(parsed.length_context().is_none());
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalLengthUnitUnresolved),
        1
    );
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Blocking);
}

#[test]
fn absent_or_nonpositive_significance_fields_substitute_seventeen_digits() {
    for (global, field) in [
        (
            b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;".as_slice(),
            9,
        ),
        (
            b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,0,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1,1Ha,1Ho,11,0,0H,0H;".as_slice(),
            11,
        ),
    ] {
        let (parsed, losses) =
            crate::global::parse(&crate::card::scan(&fixed_ascii_with_global(global)).unwrap())
                .unwrap();

        let precision = parsed.real_precision();
        let substituted = if field == 9 {
            precision.single_significance
        } else {
            precision.double_significance
        };
        assert_eq!(substituted, 17, "field {field}");
        assert_eq!(losses.len(), 1, "field {field}: {losses:#?}");
        assert_eq!(
            code_count(&losses, IgesLossCode::GlobalSemanticContextSubstituted),
            1,
            "field {field}"
        );
        assert!(
            losses[0].message.contains(&format!("field {field}")),
            "{}",
            losses[0].message
        );
    }
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
    for (resolution, expected) in [("", 0_usize), ("-0.001", 1)] {
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
        assert_eq!(
            code_count(&losses, IgesLossCode::GlobalSemanticContextSubstituted),
            expected,
            "{resolution:?}"
        );
    }
}

#[test]
fn global_hollerith_values_reject_non_printable_ascii() {
    for byte in [0x00, 0x1f, 0x7f, 0x80, 0xff] {
        let mut bytes = point_file();
        let product = bytes
            .windows(9)
            .position(|window| window == b"7Hproduct")
            .expect("sender product");
        bytes[product + 5] = byte;

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(!result
            .ir()
            .source
            .as_ref()
            .unwrap()
            .attributes
            .contains_key("sender_product"));
        assert_eq!(result.ir().model.points.len(), 1, "{byte:#04x}");
        assert_eq!(
            report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
            1,
            "{byte:#04x}"
        );
    }
}

#[test]
fn a_forbidden_delimiter_payload_still_refuses_the_file() {
    for field in [b"1H,".as_slice(), b"1H;".as_slice()] {
        let mut bytes = point_file();
        let position = bytes
            .windows(3)
            .position(|window| window == field)
            .expect("delimiter declaration");
        bytes[position + 2] = 0x01;

        assert!(
            matches!(
                IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()),
                Err(CodecError::Malformed(_))
            ),
            "{field:?}"
        );
    }
}

#[test]
fn fixed_ascii_5_1_and_5_2_decode_under_the_supported_profile() {
    for (encoded_version, version_name) in [(b"09", "5.1"), (b"10", "5.2")] {
        let mut bytes = point_file();
        let version = bytes
            .windows(b",11,0,".len())
            .position(|window| window == b",11,0,")
            .unwrap();
        bytes[version + 1..version + 3].copy_from_slice(encoded_version);

        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(bytes.clone()),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(summary
            .notes
            .contains(&format!("iges_version={version_name}")));
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["iges_version"],
            version_name
        );
        assert_eq!(result.ir().model.points.len(), 1);
        assert!(
            result.report().losses.is_empty(),
            "{version_name}: {:#?}",
            result.report().losses
        );
        assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn declared_versions_outside_the_verified_set_decode_with_a_dialect_loss() {
    for (flag, version_name) in [("6", "4.0"), ("3", "2.0")] {
        let bytes = point_file_with_version_flag(flag);

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap();
        let source = result.ir().source.as_ref().unwrap();
        assert_eq!(source.attributes["iges_version"], version_name);
        assert_eq!(source.attributes["iges_version_flag"], flag);
        assert_eq!(result.ir().model.points.len(), 1);
        assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
        assert_eq!(
            result.report().losses[0].code,
            IgesLossCode::SourceDialectUnverified.kind()
        );

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &strict_options(false))
            .unwrap_err();
        match error {
            CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
                loss_code,
                IgesLossCode::SourceDialectUnverified.kind().as_str()
            ),
            other => panic!("expected a shared-gate strict refusal, got {other:?}"),
        }
    }
}

#[test]
fn a_clamped_version_flag_is_recorded_verbatim_and_charges_the_dialect_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_version_flag("99")),
            &DecodeOptions::default(),
        )
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "5.3");
    assert_eq!(source.attributes["iges_version_flag"], "99");
    assert_eq!(dialect_losses(result.report()), 1);
}

#[test]
fn a_verified_declared_version_charges_no_dialect_loss() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_version_flag("11")),
            &DecodeOptions::default(),
        )
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "5.3");
    assert_eq!(source.attributes["iges_version_flag"], "11");
    assert_eq!(dialect_losses(result.report()), 0);
}

#[test]
fn a_container_only_decode_reports_the_dialect_loss_and_strict_admits_it() {
    let bytes = point_file_with_version_flag("6");
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &options)
        .unwrap();
    assert_eq!(dialect_losses(result.report()), 1);

    let strict = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(true))
        .unwrap();
    assert_eq!(dialect_losses(strict.report()), 1);
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
    assert!(!result.report().geometry_transferred);
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
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
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

#[test]
fn an_absent_maximum_line_width_decodes_in_salvage_and_strict_modes() {
    let bytes = point_file_with_field(16, "");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::LineWeightScaleUnavailable),
        1
    );

    let strict = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap();
    assert_eq!(
        report_code_count(strict.report(), IgesLossCode::LineWeightScaleUnavailable),
        1
    );
}

#[test]
fn a_legacy_version_with_no_maximum_line_width_decodes_and_strict_names_the_dialect() {
    let mut fields = valid_global_fields();
    fields[16] = String::new();
    fields[22] = "6".into();
    let mut global = fields.join(",");
    global.push(';');
    let bytes = point_file_with_global(global.as_bytes());

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 2, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 1);
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::LineWeightScaleUnavailable),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::SourceDialectUnverified.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn a_malformed_single_precision_significance_decodes_and_strict_refuses() {
    for value in ["-3", "1Hx"] {
        let bytes = point_file_with_field(8, value);

        let mut fields = valid_global_fields();
        fields[8] = value.to_owned();
        let (parsed, _) = resolve_global_fields(&fields);
        assert_eq!(parsed.real_precision().single_significance, 17, "{value}");
        assert_eq!(parsed.real_precision().double_significance, 15, "{value}");

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap();
        assert_eq!(result.ir().model.points.len(), 1, "{value}");
        assert_eq!(
            report_code_count(
                result.report(),
                IgesLossCode::GlobalSemanticContextSubstituted
            ),
            1,
            "{value}: {:#?}",
            result.report().losses
        );

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &strict_options(false))
            .unwrap_err();
        match error {
            CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
                loss_code,
                IgesLossCode::GlobalSemanticContextSubstituted
                    .kind()
                    .as_str()
            ),
            other => panic!("expected a shared-gate strict refusal, got {other:?}"),
        }
    }
}

#[test]
fn a_negative_minimum_resolution_decodes_with_the_semantic_context_loss() {
    let bytes = point_file_with_field(18, "-0.001");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.ir().tolerances.linear, 0.0);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(
        report_code_count(
            result.report(),
            IgesLossCode::GlobalSemanticContextSubstituted
        ),
        1
    );
}

#[test]
fn a_twenty_seventh_global_field_decodes_with_the_noncanonical_framing_loss() {
    let mut fields = valid_global_fields();
    fields.push("0H".into());
    let mut global = fields.join(",");
    global.push(';');
    let bytes = point_file_with_global(global.as_bytes());

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalNoncanonicalFraming),
        1
    );

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::GlobalNoncanonicalFraming.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn a_malformed_generation_date_decodes_in_salvage_and_strict_modes() {
    let bytes = point_file_with_field(17, "15H20261314.000000");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
        1
    );

    let strict = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap();
    assert_eq!(
        report_code_count(strict.report(), IgesLossCode::GlobalMetadataFieldUnusable),
        1
    );
}

#[test]
fn a_malformed_version_flag_clamps_to_the_default_and_charges_the_dialect_loss() {
    let bytes = point_file_with_field(22, "1Hx");

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "2.0");
    assert_eq!(source.attributes["iges_version_flag"], "3");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 1);
    assert!(result.report().losses[0].message.contains("field 23"));

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap_err();
    match error {
        CodecError::StrictRefusal { loss_code, .. } => assert_eq!(
            loss_code,
            IgesLossCode::SourceDialectUnverified.kind().as_str()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn a_prohibited_delimiter_declaration_is_honored_and_charges_noncanonical_framing() {
    for (parameter, record, expected) in [
        (',', ';', 0_usize),
        ('+', ';', 1),
        (',', 'D', 1),
        ('+', 'D', 2),
    ] {
        let bytes = point_file_with_delimiters(parameter, record);

        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        assert_eq!(
            result.ir().model.points.len(),
            1,
            "{parameter}{record}: {:#?}",
            result.report().losses
        );
        let position = &result.ir().model.points[0].position;
        assert_eq!(
            (position.x, position.y, position.z),
            (1.0, 2.0, 3.0),
            "{parameter}{record}"
        );
        assert_eq!(
            report_code_count(result.report(), IgesLossCode::GlobalNoncanonicalFraming),
            expected,
            "{parameter}{record}: {:#?}",
            result.report().losses
        );
    }
}

#[test]
fn omitted_delimiter_fields_select_the_specification_defaults() {
    for global in [
        b",,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
        b"1H,,,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
        b",1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;".as_slice(),
    ] {
        let (parsed, losses) =
            crate::global::parse(&crate::card::scan(&fixed_ascii_with_global(global)).unwrap())
                .unwrap();

        assert_eq!(parsed.parameter_delimiter, b',');
        assert_eq!(parsed.record_delimiter, b';');
        assert_eq!(parsed.sender_product().as_deref(), Some("product"));
        assert!(losses.is_empty(), "{losses:#?}");
    }
}

#[test]
fn an_absent_version_flag_reports_the_default_dialect() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(point_file_with_field(22, "")),
            &DecodeOptions::default(),
        )
        .unwrap();

    let source = result.ir().source.as_ref().unwrap();
    assert_eq!(source.attributes["iges_version"], "2.0");
    assert_eq!(source.attributes["iges_version_flag"], "3");
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 1);
}

#[test]
fn inspect_reports_the_resolution_losses_it_charges_as_census_notes() {
    let mut fields = valid_global_fields();
    fields[16] = String::new();
    fields[22] = "6".into();
    let mut global = fields.join(",");
    global.push(';');

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_global(global.as_bytes())),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"iges_version=4.0".into()));
    assert!(summary
        .notes
        .contains(&"loss.iges/source.dialect-unverified=1".into()));
    assert!(summary
        .notes
        .contains(&"loss.iges/presentation.line-weight-scale-unavailable=1".into()));
}

#[test]
fn inspect_reports_the_declared_version_flag_only_when_the_clamp_changes_it() {
    for (flag, version) in [("12", "5.3"), ("0", "2.0")] {
        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(point_file_with_version_flag(flag)),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(
            summary.notes.contains(&format!("iges_version={version}")),
            "{flag}: {:#?}",
            summary.notes
        );
        assert!(
            summary.notes.contains(&format!("iges_version_flag={flag}")),
            "{flag}: {:#?}",
            summary.notes
        );
    }

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_version_flag("11")),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(
        !summary
            .notes
            .iter()
            .any(|note| note.starts_with("iges_version_flag=")),
        "{:#?}",
        summary.notes
    );
}
