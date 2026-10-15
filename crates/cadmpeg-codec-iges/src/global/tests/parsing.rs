// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{point_file_with_field, report_code_count, strict_options, valid_global_fields};
use crate::loss::IgesLossCode;
use crate::test_support::test_cards::{
    card, directory_card, fixed_ascii_with_global, fixed_ascii_with_global_cards, parameter_card,
    CARD_DATA_COLUMNS, CARD_LINE_BYTES,
};
use crate::test_support::test_curves_and_surfaces::{point_file, point_file_with_global};
use crate::IgesCodec;

fn with_work_limit<T>(
    source: &[u8],
    max_work_units: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = max_work_units;
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy).unwrap();
    run(&ctx)
}

fn assert_work_limit(error: &CodecError, operation: &str, additional: u64) {
    assert!(matches!(error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.additional == additional
                && limit.operation == operation
    ));
}

#[test]
fn global_layout_hollerith_digit_scan_refuses_work_before_probe() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global layout Hollerith digits",
        |cap| {
            with_work_limit(b"1H,", cap, |ctx| {
                crate::global::layout_hollerith(b"1H,", 0, ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global layout Hollerith digits", 1);
}

#[test]
fn global_layout_hollerith_count_refuses_utf8_work() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global layout Hollerith count",
        |cap| {
            with_work_limit(b"1H,", cap, |ctx| {
                crate::global::layout_hollerith(b"1H,", 0, ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global layout Hollerith count", 1);
}

#[test]
fn global_layout_field_scan_refuses_work_before_field() {
    let bytes = b",,1;";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global layout fields",
        |cap| {
            with_work_limit(bytes, cap, |ctx| {
                crate::global::layout_global_cards(bytes, ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global layout fields", 1);
}

#[test]
fn global_layout_counts_leading_padding_once_when_fitting_hollerith_header() {
    let bytes = [b",,".as_slice(), &[b' '; 50], b"1Ha;"].concat();
    with_work_limit(&bytes, u64::MAX, |ctx| {
        let cards = crate::global::layout_global_cards(&bytes, ctx)
            .expect("the field and its Hollerith header fit in one card");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0], bytes);
    });
}

#[test]
fn global_layout_field_bytes_refuse_work_before_byte() {
    let bytes = b",,1;";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global layout field bytes",
        |cap| {
            with_work_limit(bytes, cap, |ctx| {
                crate::global::layout_global_cards(bytes, ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global layout field bytes", 1);
}

#[test]
fn global_hollerith_digit_scan_refuses_work_before_probe() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global Hollerith digits",
        |cap| with_work_limit(b"1H,", cap, |ctx| crate::global::hollerith(b"1H,", 0, ctx)),
    );
    assert_work_limit(&error, "iges global Hollerith digits", 1);
}

#[test]
fn global_hollerith_count_refuses_utf8_work() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global Hollerith count",
        |cap| with_work_limit(b"1H,", cap, |ctx| crate::global::hollerith(b"1H,", 0, ctx)),
    );
    assert_work_limit(&error, "iges global Hollerith count", 1);
}

#[test]
fn global_field_scan_refuses_work_before_value() {
    let bytes = fixed_ascii_with_global(b"1H,,1H;,;");
    let scan = crate::test_support::scan(&bytes).unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global fields",
        |cap| {
            with_work_limit(&bytes, cap, |ctx| {
                crate::global::parse(&scan, ctx).map(|_| ())
            })
        },
    );
    assert_work_limit(&error, "iges global fields", 1);
}

#[test]
fn global_numeric_text_refuses_utf8_work() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global numeric text",
        |cap| with_work_limit(b"42", cap, |ctx| crate::global::numeric_text(b"42", ctx)),
    );
    assert_work_limit(&error, "iges global numeric text", 2);
}

#[test]
fn global_normalized_real_refuses_utf8_work() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global numeric text",
        |cap| {
            with_work_limit(b"1D+0", cap, |ctx| {
                crate::global::parse_real_text("1D+0", ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global numeric text", 4);
}

#[test]
fn global_supplied_string_refuses_utf8_work() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global supplied string",
        |cap| {
            with_work_limit(b"abc", cap, |ctx| {
                let resolution = crate::global::Resolution {
                    ctx,
                    values: std::array::from_fn(|index| {
                        if index == 0 {
                            crate::global::Value::String(b"abc")
                        } else {
                            crate::global::Value::Omitted
                        }
                    }),
                    losses: Vec::new(),
                    loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
                };
                resolution.supplied_string(0).map(|_| ())
            })
        },
    );
    assert_work_limit(&error, "iges global supplied string", 3);
}

fn point_file_with_delimiters(parameter: char, record: char) -> Vec<u8> {
    let mut fields = valid_global_fields();
    fields[0] = format!("1H{parameter}");
    fields[1] = format!("1H{record}");
    let global = format!("{}{record}", fields.join(&parameter.to_string()));
    let mut bytes = fixed_ascii_with_global(global.as_bytes());
    bytes.truncate(bytes.len() - CARD_LINE_BYTES);
    bytes.extend(directory_card(
        ["116", "1", "0", "0", "0", "0", "0", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["116", "0", "0", "1", "0", "", "", "POINT", "0"],
        2,
    ));
    bytes.extend(parameter_card(
        format!("116{parameter}1.25{parameter}2.5{parameter}3.75{record}").as_bytes(),
        1,
        1,
    ));
    let global_cards = global.len().div_ceil(CARD_DATA_COLUMNS);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000002P0000001").as_bytes(),
        b'T',
        1,
    ));
    bytes
}

fn fixed_ascii_with_global_chunks(chunks: &[&[u8]]) -> Vec<u8> {
    fixed_ascii_with_global_cards(
        &chunks
            .iter()
            .flat_map(|chunk| chunk.chunks(CARD_DATA_COLUMNS))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn global_layout_card_refuses_retained_limit_before_allocation() {
    let bytes = b"1H,,1H;,;";
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "iges global layout card bytes",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
            crate::global::layout_global_cards(bytes, &ctx)
        },
    );
    assert!(matches!(
        result,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == 72
                && limit.operation == "iges global layout card bytes"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::global::layout_global_cards(bytes, &ctx)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn global_d_exponent_refuses_temporary_limit_before_normalization() {
    let text = "1D+0";
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges global numeric text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy).unwrap();
            crate::global::parse_real_text(text, &ctx)
        },
    );
    assert!(matches!(
        result,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.used == 0
                && limit.additional == 4
                && limit.operation == "iges global numeric text"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(text.as_bytes(), &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        crate::global::parse_real_text(text, &ctx)
            .unwrap()
            .unwrap()
            .get(),
        1.0
    );
}

#[test]
fn global_supplied_string_borrows_text_without_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
    let resolution = crate::global::Resolution {
        ctx: &ctx,
        values: std::array::from_fn(|index| {
            if index == 0 {
                crate::global::Value::String(b"abc")
            } else {
                crate::global::Value::Omitted
            }
        }),
        losses: Vec::new(),
        loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
    };
    assert!(matches!(
        resolution.supplied_string(0).unwrap(),
        crate::global::Supplied::Value(value) if value == "abc"
    ));
}

#[test]
fn global_loss_note_refuses_collection_limit_before_push() {
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges global loss notes",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut resolution = crate::global::Resolution {
                ctx: &ctx,
                values: [crate::global::Value::Omitted; 26],
                losses: Vec::new(),
                loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
            };
            let result = resolution.charge(
                IgesLossCode::GlobalMetadataFieldUnusable,
                2,
                crate::global::Defect::Malformed,
                "its value was not transferred",
            );
            assert!(resolution.losses.is_empty());
            result
        },
    );
    assert!(matches!(
        result,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges global loss notes"
    ));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut resolution = crate::global::Resolution {
        ctx: &ctx,
        values: [crate::global::Value::Omitted; 26],
        losses: Vec::new(),
        loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
    };
    resolution
        .charge(
            IgesLossCode::GlobalMetadataFieldUnusable,
            2,
            crate::global::Defect::Malformed,
            "its value was not transferred",
        )
        .unwrap();
    assert_eq!(resolution.losses.len(), 1);
}

#[test]
fn global_field_source_locations_follow_72_byte_card_boundaries() {
    let first = [b'A'; CARD_DATA_COLUMNS];
    let second = [b'B'; CARD_DATA_COLUMNS];
    let bytes = fixed_ascii_with_global_cards(&[&first, &second]);
    assert_eq!(
        crate::test_support::scan(&bytes)
            .unwrap()
            .section(crate::card::Section::Global)
            .len(),
        2
    );
    assert!(!crate::global::source_span_crosses_card(
        0,
        CARD_DATA_COLUMNS,
    ));
    assert!(crate::global::source_span_crosses_card(
        CARD_DATA_COLUMNS - 1,
        CARD_DATA_COLUMNS + 1,
    ));
    assert!(!crate::global::source_span_crosses_card(
        CARD_DATA_COLUMNS,
        2 * CARD_DATA_COLUMNS,
    ));
}

#[test]
fn global_stream_refuses_temporary_limit_before_copy() {
    let global = format!("{};", valid_global_fields().join(","));
    let bytes = fixed_ascii_with_global(global.as_bytes());
    let scan = crate::test_support::scan(&bytes).unwrap();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges_global_stream",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
            crate::global::parse(&scan, &ctx).map(|_| ())
        },
    );
    let arena = DecodeArena::new();

    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &service).unwrap();
    assert!(crate::global::parse(&scan, &ctx).is_ok());
}

#[test]
fn global_excess_fields_are_counted_without_retaining_values() {
    let mut fields = valid_global_fields();
    fields.extend(std::iter::repeat_n(String::new(), 1_000));
    let global = format!("{};", fields.join(","));
    let bytes = fixed_ascii_with_global(global.as_bytes());
    let scan = crate::test_support::scan(&bytes).unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges global loss notes",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)?;
            crate::global::parse(&scan, &ctx).map(|_| ())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.used == 0 && limit.additional == 1
            && limit.operation == "iges global loss notes"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (_, (losses, _loss_storage), _global_storage) = crate::global::parse(&scan, &ctx).unwrap();
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("1026 fields")));
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
fn global_hollerith_header_split_across_cards_is_a_field_defect() {
    let product = "p".repeat(70);
    let tail = format!(
        "0H{product},8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;"
    );
    let bytes = fixed_ascii_with_global_chunks(&[b"1H,,1H;,7", tail.as_bytes()]);
    let (parsed, losses) =
        crate::test_support::parse_global(&crate::test_support::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product(), None);
    assert_eq!(parsed.native_file_name(), Some("part.igs"));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(
        losses[0].code,
        IgesLossCode::GlobalMetadataFieldUnusable.kind()
    );
    assert!(losses[0].message.contains("field 3"));
}

#[test]
fn global_numeric_field_and_delimiter_must_share_a_card() {
    let prefix = b"1H,,1H;,1Hp,1Hf,1Hs,1Hv,";
    let padding = 71 - prefix.len();
    let mut global = prefix.to_vec();
    global.extend(std::iter::repeat_n(b' ', padding));
    global.extend_from_slice(b"1,;");
    let cards = global.chunks(CARD_DATA_COLUMNS).collect::<Vec<_>>();
    let (parsed, losses) = crate::test_support::parse_global(
        &crate::test_support::scan(&fixed_ascii_with_global_cards(&cards)).unwrap(),
    )
    .unwrap();

    assert_eq!(parsed.sender_product(), Some("p"));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::GlobalMetadataFieldUnusable.kind())
            .count(),
        1,
        "{losses:#?}"
    );
    assert!(losses.iter().any(|loss| loss.message.contains("field 7")));
}

#[test]
fn global_card_padding_is_ignored_outside_hollerith_values() {
    let bytes = fixed_ascii_with_global_chunks(&[
        b"1H,,1H;,7Hproduct,8Hpart.igs,",
        b"7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ]);
    let (parsed, _) =
        crate::test_support::parse_global(&crate::test_support::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product(), Some("product"));
    assert_eq!(parsed.native_file_name(), Some("part.igs"));
}

#[test]
fn global_card_padding_does_not_remove_hollerith_payload_spaces() {
    let bytes = fixed_ascii_with_global_chunks(&[
        b"1H,,1H;,3Hab ",
        b",8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
    ]);
    let (parsed, _) =
        crate::test_support::parse_global(&crate::test_support::scan(&bytes).unwrap()).unwrap();

    assert_eq!(parsed.sender_product(), Some("ab "));
}

#[test]
fn global_numeric_fields_reject_embedded_and_trailing_blanks() {
    for value in ["3 2", "32 "] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(point_file_with_field(6, value)),
                &DecodeOptions::default(),
            )
            .unwrap();

        assert_eq!(result.ir().model.points.len(), 1, "{value:?}");
        assert_eq!(
            report_code_count(result.report(), IgesLossCode::GlobalMetadataFieldUnusable),
            1,
            "{value:?}: {:#?}",
            result.report().losses
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("field 7")));
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
                Err(cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(_)))
            ),
            "{field:?}"
        );
    }
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
        cadmpeg_ir::codec::DecodeFailure::StrictRejected { rejection } => assert_eq!(
            rejection.loss().code.to_string(),
            IgesLossCode::GlobalNoncanonicalFraming.kind().to_string()
        ),
        other => panic!("expected a shared-gate strict refusal, got {other:?}"),
    }
}

#[test]
fn prohibited_delimiter_declarations_refuse_before_parameter_decode() {
    let prohibited = [
        ' ', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '+', '-', '.', 'D', 'E', 'H',
    ];
    for delimiter in prohibited {
        for (parameter, record) in [(delimiter, ';'), (',', delimiter)] {
            let bytes = point_file_with_delimiters(parameter, record);
            assert!(
                matches!(
                    IgesCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()),
                    Err(cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(_)))
                ),
                "{parameter}{record}"
            );
        }
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
            crate::test_support::parse_global(&crate::test_support::scan(&fixed_ascii_with_global(global)).unwrap())
                .unwrap();

        assert_eq!(parsed.parameter_delimiter, b',');
        assert_eq!(parsed.record_delimiter, b';');
        assert_eq!(parsed.sender_product(), Some("product"));
        assert!(losses.is_empty(), "{losses:#?}");
    }
}

#[test]
fn global_layout_hollerith_parse_refuses_after_utf8_admission() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global layout Hollerith number",
        |cap| {
            with_work_limit(b"1H,", cap, |ctx| {
                crate::global::layout_hollerith(b"1H,", 0, ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global layout Hollerith number", 1);
}

#[test]
fn global_hollerith_parse_refuses_after_utf8_admission() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global Hollerith number",
        |cap| with_work_limit(b"1H,", cap, |ctx| crate::global::hollerith(b"1H,", 0, ctx)),
    );
    assert_work_limit(&error, "iges global Hollerith number", 1);
}

#[test]
fn global_plain_real_parse_refuses_work_before_conversion() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global numeric real",
        |cap| with_work_limit(b"42", cap, |ctx| crate::global::parse_real_text("42", ctx)),
    );
    assert_work_limit(&error, "iges global numeric real", 2);
}

#[test]
fn global_normalized_real_parse_refuses_after_utf8_admission() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global numeric real",
        |cap| {
            with_work_limit(b"1D+0", cap, |ctx| {
                crate::global::parse_real_text("1D+0", ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global numeric real", 4);
}

#[test]
fn global_integer_parse_refuses_after_utf8_admission() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global integer value",
        |cap| {
            with_work_limit(b"42", cap, |ctx| {
                let resolution = crate::global::Resolution {
                    ctx,
                    values: std::array::from_fn(|index| {
                        if index == 0 {
                            crate::global::Value::Atom(b"42")
                        } else {
                            crate::global::Value::Omitted
                        }
                    }),
                    losses: Vec::new(),
                    loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
                };
                resolution.supplied_integer(0)
            })
        },
    );
    assert_work_limit(&error, "iges global integer value", 2);
}

#[test]
fn global_variable_scans_refuse_at_their_own_boundaries() {
    for operation in [
        "iges global value leading spaces",
        "iges global atom delimiter",
        "iges global numeric leading spaces",
        "iges global numeric whitespace",
        "iges global numeric exponent",
        "iges global exponent normalization",
        "iges global recovered exponent",
        "iges global string policy",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                with_work_limit(b" 42,1D+0.20260714.000000abc", cap, |ctx| match operation {
                    "iges global value leading spaces" | "iges global atom delimiter" => {
                        crate::global::delimited_value(b" 42,", 0, b',', Some(b';'), true, ctx)
                            .map(|_| ())
                    }
                    "iges global numeric leading spaces" | "iges global numeric whitespace" => {
                        crate::global::numeric_text(b" 42", ctx).map(|_| ())
                    }
                    "iges global numeric exponent" | "iges global exponent normalization" => {
                        crate::global::parse_real_text("1D+0", ctx).map(|_| ())
                    }
                    "iges global recovered exponent" => {
                        crate::global::recovered_real_text("1D+0.", ctx).map(|_| ())
                    }
                    _ => {
                        let mut resolution = crate::global::Resolution {
                            ctx,
                            values: std::array::from_fn(|index| {
                                if index == 0 {
                                    crate::global::Value::String(b"abc")
                                } else {
                                    crate::global::Value::Omitted
                                }
                            }),
                            losses: Vec::new(),
                            loss_storage: ctx.reserve_scoped(0, "iges global loss notes")?,
                        };
                        resolution.apply_string_policy(crate::global::GlobalTable::V5Later)
                    }
                })
            },
        );
    }
}

#[test]
fn global_resolved_text_has_scoped_storage_and_no_retained_copy() {
    let global = format!("{};", valid_global_fields().join(","));
    let bytes = fixed_ascii_with_global(global.as_bytes());
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (global, (losses, loss_storage), storage) = crate::global::parse(&scan, &ctx).unwrap();
    assert_eq!(global.sender_product(), Some("product"));
    assert!(losses.is_empty());
    assert!(ctx
        .reserve_scoped(policy.limits.max_materialized_bytes, "live Global storage")
        .is_err());
    drop(global);
    drop(losses);
    drop(loss_storage);
    drop(storage);
    // Use a fresh session for the release assertion because a refusal fuses its session.
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (global, (losses, loss_storage), storage) = crate::global::parse(&scan, &ctx).unwrap();
    assert!(losses.is_empty());
    drop(global);
    drop(losses);
    drop(loss_storage);
    drop(storage);
    assert!(ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "released Global storage"
        )
        .is_ok());
}

#[test]
fn recovered_real_declaration_refuses_temporary_storage() {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges global declaration text",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let mut resolution = crate::global::Resolution {
                ctx: &ctx,
                values: std::array::from_fn(|index| {
                    if index == 0 {
                        crate::global::Value::Atom(b"1D+0.")
                    } else {
                        crate::global::Value::Omitted
                    }
                }),
                losses: Vec::new(),
                loss_storage: ctx.reserve_scoped(0, "iges global loss notes")?,
            };
            resolution.charge_recovered_real(0, 1.0)
        },
    );
}

#[test]
fn global_date_validation_checks_fixed_components_and_version_width() {
    for (text, accepts_long, valid) in [
        ("010101.000000", true, true),
        ("010101.000000", false, true),
        ("010100.000000", true, false),
        ("20260714.235959", true, true),
        ("20260714.235959", false, false),
        ("20260014.000000", true, false),
        ("20261314.000000", true, false),
        ("20260700.000000", true, false),
        ("20260732.000000", true, false),
        ("20260714.240000", true, false),
        ("20260714.006000", true, false),
        ("20260714.000060", true, false),
        ("20260714x000000", true, false),
        ("20260714.00000x", true, false),
        ("20260714.00000", true, false),
        ("20260714.0000000", true, false),
    ] {
        assert_eq!(
            crate::global::date_value_is_valid(text.as_bytes(), accepts_long),
            valid,
            "{text}"
        );
    }
}

#[test]
fn global_layout_field_spans_charge_one_visited_field() {
    let bytes = format!(",,{}{};", "a".repeat(73), ",".repeat(1_000));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges global layout field spans",
        |cap| {
            with_work_limit(bytes.as_bytes(), cap, |ctx| {
                crate::global::layout_global_cards(bytes.as_bytes(), ctx)
            })
        },
    );
    assert_work_limit(&error, "iges global layout field spans", 1);
    with_work_limit(bytes.as_bytes(), u64::MAX, |ctx| {
        assert!(matches!(
            crate::global::layout_global_cards(bytes.as_bytes(), ctx),
            Err(CodecError::Malformed(_))
        ));
    });
}
