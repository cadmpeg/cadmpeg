// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    code_count, point_file_with_field, report_code_count, resolve_global_fields, strict_options,
    valid_global_fields,
};
use crate::loss::IgesLossCode;
use crate::test_support::fixed_ascii_with_global;
use crate::IgesCodec;

#[test]
fn global_defaults_apply_only_to_omitted_fields() {
    let global =
        b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,32,38,6,308,15,0H,,,2HIN,1,1.0,15H20260714.000000,0,1,1Ha,1Ho,,0,0H,0H;";
    let bytes = fixed_ascii_with_global(global);
    let scan = crate::card::scan(&bytes).unwrap();
    let (parsed, losses) = crate::global::parse(&scan).unwrap();
    let context = parsed.length_context().unwrap();

    assert_eq!(context.length_factor_mm(), 25.4);
    assert_eq!(parsed.declared_version_flag(), 3);
    assert_eq!(context.minimum_resolution_mm(), 0.0);
    assert_eq!(parsed.real_precision().single_significance, 6);
    assert_eq!(parsed.real_precision().double_significance, 15);
    assert_eq!(parsed.sender_product().as_deref(), Some("p"));
    assert!(losses.is_empty(), "{losses:#?}");
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
    assert_eq!(parsed.declared_version_flag(), 3);
    assert!((context.minimum_resolution_mm() - 0.0254).abs() <= f64::EPSILON * 64.0);
    assert!(losses.is_empty(), "{losses:#?}");

    for (index, expected) in [
        (3, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (4, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (5, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (6, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (7, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (8, Some(IgesLossCode::GlobalSemanticContextSubstituted)),
        (9, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (10, Some(IgesLossCode::GlobalSemanticContextSubstituted)),
        (16, Some(IgesLossCode::LineWeightScaleUnavailable)),
        (17, Some(IgesLossCode::GlobalMetadataFieldUnusable)),
        (18, Some(IgesLossCode::GlobalSemanticContextSubstituted)),
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
    assert_eq!(
        code_count(&losses, IgesLossCode::GlobalMetadataFieldUnusable),
        1,
        "{losses:#?}"
    );
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
fn readable_numeric_capabilities_are_retained_separately_from_projection_precision() {
    let (parsed, losses) = resolve_global_fields(&valid_global_fields());

    assert_eq!(
        parsed.numeric_limits().integer_bits,
        Some(32),
        "{losses:#?}"
    );
    assert_eq!(parsed.numeric_limits().single_magnitude, Some(38));
    assert_eq!(parsed.numeric_limits().double_magnitude, Some(308));

    let mut fields = valid_global_fields();
    fields[6].clear();
    fields[7].clear();
    fields[8].clear();
    fields[9].clear();
    fields[10].clear();
    let (parsed, _) = resolve_global_fields(&fields);
    assert_eq!(
        parsed.numeric_limits(),
        crate::global::NumericLimits::default()
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
            cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => assert_eq!(
                rejection.loss().code.as_str(),
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
