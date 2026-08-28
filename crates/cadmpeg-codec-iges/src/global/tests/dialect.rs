// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    point_file_with_field, point_file_with_version_flag, report_code_count, resolve_global_fields,
    strict_options, valid_global_fields,
};
use crate::loss::IgesLossCode;
use crate::test_support::point_file_with_global;
use crate::IgesCodec;

fn dialect_losses(report: &cadmpeg_ir::report::DecodeReport) -> usize {
    report_code_count(report, IgesLossCode::SourceDialectUnverified)
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

// IGES 5.3 §2.2.4.3.23 lists eleven version-flag values and clamps a
// declaration below 1 to 3 and above 11 to 11. The rows cover all eleven
// plus both clamp directions. Classes 2, 5, and 7 name ANSI/ASME drafting
// standards rather than IGES releases; the codec renders them with hyphens
// where the specification table uses spaces and a slash.
#[test]
fn every_version_flag_class_maps_to_its_specification_version() {
    for (value, declared, effective, version) in [
        ("1", 1, 1, "1.0"),
        ("2", 2, 2, "ANSI-Y14.26M-1981"),
        ("3", 3, 3, "2.0"),
        ("4", 4, 4, "3.0"),
        ("5", 5, 5, "ASME-ANSI-Y14.26M-1987"),
        ("6", 6, 6, "4.0"),
        ("7", 7, 7, "ASME-Y14.26M-1989"),
        ("8", 8, 8, "5.0"),
        ("9", 9, 9, "5.1"),
        ("10", 10, 10, "5.2"),
        ("11", 11, 11, "5.3"),
        ("0", 0, 3, "2.0"),
        ("12", 12, 11, "5.3"),
    ] {
        let mut fields = valid_global_fields();
        fields[22] = value.into();
        let (parsed, _) = resolve_global_fields(&fields);
        assert_eq!(parsed.declared_version_flag(), declared, "{value}");
        assert_eq!(parsed.effective_version_flag(), effective, "{value}");
        assert_eq!(parsed.version_name(), version, "{value}");
    }
}

#[test]
fn fixed_ascii_verified_versions_decode_under_their_versioned_profiles() {
    for (encoded_version, version_name) in [
        ("6", "4.0"),
        ("8", "5.0"),
        ("9", "5.1"),
        ("10", "5.2"),
        ("11", "5.3"),
    ] {
        let bytes = point_file_with_version_flag(encoded_version);

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
    for (flag, version_name) in [
        ("1", "1.0"),
        ("2", "ANSI-Y14.26M-1981"),
        ("3", "2.0"),
        ("4", "3.0"),
        ("5", "ASME-ANSI-Y14.26M-1987"),
        ("7", "ASME-Y14.26M-1989"),
    ] {
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
fn a_container_only_decode_preserves_verified_v4_and_strict_admits_it() {
    let bytes = point_file_with_version_flag("6");
    let options = DecodeOptions {
        container_only: true,
        ..DecodeOptions::default()
    };

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &options)
        .unwrap();
    assert_eq!(dialect_losses(result.report()), 0);

    let strict = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(true))
        .unwrap();
    assert_eq!(dialect_losses(strict.report()), 0);
}

#[test]
fn a_verified_v4_version_with_no_maximum_line_width_names_the_line_weight_loss() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[17] = "13H260714.000000".into();
    fields.truncate(24);
    fields[16] = String::new();
    fields[22] = "6".into();
    let mut global = fields.join(",");
    global.push(';');
    let bytes = point_file_with_global(global.as_bytes());

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(result.report().losses.len(), 1, "{:#?}", result.report());
    assert_eq!(dialect_losses(result.report()), 0);
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::LineWeightScaleUnavailable),
        1
    );

    let strict = IgesCodec
        .decode(&mut Cursor::new(bytes), &strict_options(false))
        .unwrap();
    assert_eq!(dialect_losses(strict.report()), 0);
    assert_eq!(
        report_code_count(strict.report(), IgesLossCode::LineWeightScaleUnavailable),
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
fn the_4_0_global_contract_accepts_twenty_four_fields_and_the_short_date() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[17] = "13H260714.000000".into();
    fields[22] = "6".into();
    fields.truncate(24);

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.version(), Some(crate::IgesVersion::V4_0));
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn the_4_0_string_contract_allows_ascii_control_bytes() {
    let mut bytes = point_file_with_version_flag("6");
    let product = bytes
        .windows(9)
        .position(|window| window == b"7Hproduct")
        .expect("sender product");
    bytes[product + 2] = 0x01;

    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();

    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["sender_product"],
        "\u{1}roduct"
    );
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(dialect_losses(result.report()), 0);
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
        0
    );
}

#[test]
fn the_4_0_global_contract_rejects_the_four_digit_date_and_later_fields() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[17] = "15H20260714.000000".into();
    fields[22] = "6".into();
    fields.truncate(24);

    let (_, losses) = resolve_global_fields(&fields);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalMetadataFieldUnusable),
        1
    );

    let mut extended = fields;
    extended.extend(["0H".into(), "0H".into()]);
    let (_, losses) = resolve_global_fields(&extended);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalNoncanonicalFraming),
        1
    );
}

#[test]
fn the_5_0_global_contract_stops_at_model_date_and_keeps_the_short_date() {
    let mut fields = valid_global_fields();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.version(), Some(crate::IgesVersion::V5_0));
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn the_5_0_global_contract_rejects_the_four_digit_date_and_later_fields() {
    let mut fields = valid_global_fields();
    fields[17] = "15H20260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);

    let (_, losses) = resolve_global_fields(&fields);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalMetadataFieldUnusable),
        1
    );

    let mut extended = fields;
    extended.push("0H".into());
    let (_, losses) = resolve_global_fields(&extended);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalNoncanonicalFraming),
        1
    );
}

#[test]
fn the_5_0_model_scale_default_is_not_the_4_0_implicit_zero() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[12].clear();
    fields[13] = "2".into();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.length_context().unwrap().length_factor_mm(), 1.0);
    assert!(losses.is_empty(), "{losses:#?}");

    fields[22] = "6".into();
    fields[17] = "13H260714.000000".into();
    fields.truncate(24);
    let (parsed, losses) = resolve_global_fields(&fields);
    assert!(parsed.length_context().is_none());
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalLengthUnitUnresolved),
        1
    );
}

#[test]
fn the_5_0_global_defaults_resolve_receiver_units_and_coordinate_metadata() {
    let mut fields = valid_global_fields();
    fields[11].clear();
    fields[14].clear();
    fields[19].clear();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.receiver_product().as_deref(), Some("product"));
    assert_eq!(parsed.units_name().as_deref(), Some("MM"));
    assert_eq!(parsed.maximum_coordinate_mm(), None);
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn the_5_0_required_global_fields_report_absence_without_later_defaults() {
    for (index, code) in [
        (2, IgesLossCode::GlobalMetadataFieldUnusable),
        (4, IgesLossCode::GlobalMetadataFieldUnusable),
        (5, IgesLossCode::GlobalMetadataFieldUnusable),
        (6, IgesLossCode::GlobalMetadataFieldUnusable),
        (7, IgesLossCode::GlobalMetadataFieldUnusable),
        (8, IgesLossCode::GlobalSemanticContextSubstituted),
        (17, IgesLossCode::GlobalMetadataFieldUnusable),
        (18, IgesLossCode::GlobalSemanticContextSubstituted),
    ] {
        let mut fields = valid_global_fields();
        fields[index].clear();
        if index != 17 {
            fields[17] = "13H260714.000000".into();
        }
        fields[22] = "8".into();
        fields.truncate(25);

        let (_, losses) = resolve_global_fields(&fields);

        assert_eq!(
            report_code_count_from_losses(&losses, code),
            1,
            "field {index}: {losses:#?}"
        );
    }

    let mut fields = valid_global_fields();
    fields[13].clear();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);
    let (_, losses) = resolve_global_fields(&fields);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalLengthUnitUnresolved),
        1
    );
}

#[test]
fn the_5_0_double_precision_fields_are_conditional_on_parameter_syntax() {
    let mut fields = valid_global_fields();
    fields[9].clear();
    fields[10].clear();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);
    let mut global = fields.join(",");
    global.push(';');

    let bytes = point_file_with_global(global.as_bytes());
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
        0
    );
    assert_eq!(
        report_code_count(
            result.report(),
            IgesLossCode::GlobalSemanticContextSubstituted
        ),
        0
    );
    assert_eq!(dialect_losses(result.report()), 0);

    let mut double_bytes = point_file_with_global(global.as_bytes());
    let old = b"116,1.0,2.0,3.0;";
    let new = b"116,1D0,2.0,3.0;";
    assert_eq!(old.len(), new.len());
    let offset = double_bytes
        .windows(old.len())
        .position(|window| window == old)
        .expect("point Parameter Data");
    double_bytes[offset..offset + old.len()].copy_from_slice(new);

    let result = IgesCodec
        .decode(&mut Cursor::new(double_bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(
        report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
        1
    );
    assert_eq!(
        report_code_count(
            result.report(),
            IgesLossCode::GlobalSemanticContextSubstituted
        ),
        1
    );
    assert_eq!(dialect_losses(result.report()), 0);
}

#[test]
fn the_4_0_global_defaults_do_not_inherit_5_0_metadata_defaults() {
    let mut fields = valid_global_fields();
    fields[11].clear();
    fields[14].clear();
    fields[19].clear();
    fields[17] = "13H260714.000000".into();
    fields[22] = "6".into();
    fields.truncate(24);

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.receiver_product(), None);
    assert_eq!(parsed.units_name(), None);
    assert_eq!(parsed.maximum_coordinate_mm(), None);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalMetadataFieldUnusable),
        3,
        "{losses:#?}"
    );
}

#[test]
fn the_5_0_line_weight_fields_support_optional_and_relative_modes() {
    let mut fields = valid_global_fields();
    fields[15].clear();
    fields[16].clear();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);

    let (parsed, losses) = resolve_global_fields(&fields);
    assert!(losses.is_empty(), "{losses:#?}");
    let context = parsed.length_context().unwrap();
    assert!(context.line_weight_number_is_valid(0));
    assert!(!context.line_weight_number_is_valid(1));
    assert_eq!(context.line_weight_mm(1), None);

    fields[15] = "3".into();
    fields[16] = "0".into();
    let (parsed, losses) = resolve_global_fields(&fields);
    assert!(losses.is_empty(), "{losses:#?}");
    let context = parsed.length_context().unwrap();
    assert!(context.line_weight_number_is_valid(0));
    assert!(context.line_weight_number_is_valid(3));
    assert!(!context.line_weight_number_is_valid(4));
    assert_eq!(context.line_weight_mm(3), None);
}

#[test]
fn the_5_0_present_gradations_still_require_field_17() {
    let mut fields = valid_global_fields();
    fields[16].clear();
    fields[17] = "13H260714.000000".into();
    fields[22] = "8".into();
    fields.truncate(25);

    let (_, losses) = resolve_global_fields(&fields);
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::LineWeightScaleUnavailable),
        1
    );
}

#[test]
fn the_4_0_missing_numeric_context_uses_reported_recovery_fallbacks() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[8].clear();
    fields[10].clear();
    fields[15].clear();
    fields[18].clear();
    fields[22] = "6".into();
    fields.truncate(24);

    let (parsed, losses) = resolve_global_fields(&fields);

    assert_eq!(parsed.precision.single_significance, 17);
    assert_eq!(parsed.precision.double_significance, 17);
    assert_eq!(parsed.minimum_resolution, 0.0);
    assert!(parsed.line_weight_scale.is_none());
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::LineWeightScaleUnavailable),
        1
    );
    assert_eq!(
        report_code_count_from_losses(&losses, IgesLossCode::GlobalSemanticContextSubstituted),
        3
    );
}

fn report_code_count_from_losses(
    losses: &[cadmpeg_ir::report::LossNote],
    code: IgesLossCode,
) -> usize {
    losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
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
