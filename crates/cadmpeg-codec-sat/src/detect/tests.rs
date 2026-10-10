// SPDX-License-Identifier: Apache-2.0
//! Detection and inspect tests for bare ASM streams.

use cadmpeg_ir::codec::{Codec, Confidence};
use std::io::Cursor;

use crate::test_support::test_streams::text_sphere_stream;
use crate::SatCodec;

#[test]
fn binary_classification_propagates_header_string_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut bytes = b"ASM BinaryFile4".to_vec();
    bytes.resize(31, 0);
    bytes.extend_from_slice(&[7, 3]);
    bytes.extend_from_slice(b"ASM");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (limited, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
    assert!(matches!(
        super::classify(&limited, &bytes),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain kernel header product string"
    ));
    let (service, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    assert!(matches!(
        super::classify(&service, &bytes).expect("header admitted"),
        Some(super::StreamKind::AsmBinary(_))
    ));
}

#[test]
fn detection_is_content_based() {
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&SatCodec, b"ASM BinaryFile8\x00"),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&SatCodec, b"ACIS BinaryFile\x00"),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &SatCodec,
            b"23200 0 2 2 \n16 Autodesk Neutron"
        ),
        Confidence::Medium
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &SatCodec,
            b"700 0 6 0           \n30 Autodesk"
        ),
        Confidence::Medium
    );
    // Numeric text without the four-word first line is not a stream.
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&SatCodec, b"123 456\n789"),
        Confidence::No
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&SatCodec, b"ISO-10303-21;\nHEADER;"),
        Confidence::No
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&SatCodec, b"{\"ir_version\":\"5\"}"),
        Confidence::No
    );
}

#[test]
fn text_header_line_holds_exactly_four_integer_fields_then_a_digit() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for (bytes, expected) in [
        (&b"700 0 6 0\n3"[..], true),
        (b"700 0 6 0\r\n3", true),
        (b"700 0\t6 0  \n3", true),
        (b"700 0 6\n3 4", false),
        (b"700 0 6 0 1\n3", false),
        (b"700 0 6 0", false),
        (b"700 0 6 0 ", false),
        (b"700 0 6 0\n", false),
        (b"700 0 6 0\nx", false),
        (b"700 0 x 0\n3", false),
    ] {
        assert_eq!(
            super::looks_like_text_stream(&ctx, bytes).expect("service admission"),
            expected,
            "{}",
            String::from_utf8_lossy(bytes)
        );
    }
}

#[test]
fn text_header_detection_visits_only_the_first_line() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut bytes = b"700 0 6 0\n3".to_vec();
    bytes.resize(1 << 20, b'7');
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 64;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    assert!(super::looks_like_text_stream(&ctx, &bytes).expect("the first line fits the budget"));
}

#[test]
fn inspect_reports_the_stream_kind_and_header_facts() {
    let summary = SatCodec
        .inspect(
            &mut Cursor::new(text_sphere_stream(1.0)),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.format(), "sat");
    assert_eq!(summary.entries.len(), 1);
    assert_eq!(summary.entries[0].role.as_str(), "brep-text");
    assert_eq!(
        summary.entries[0]
            .attributes
            .get("acis_save_format_version"),
        Some(&"23200".to_string())
    );
    assert_eq!(
        summary.entries[0].attributes.get("terminator"),
        Some(&"End-of-ASM-data".to_string())
    );
}

#[test]
fn text_inspect_propagates_framing_collection_limit() {
    use cadmpeg_core::decode::{InspectOptions, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = text_sphere_stream(1.0);
    let mut options = InspectOptions::default();
    options.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits: options.limits,
        ..cadmpeg_core::decode::DecodePolicy::default()
    };
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("borrowed input");
    let error = cadmpeg_ir::codec::CodecBackend::inspect_impl(&SatCodec, &ctx, root)
        .expect_err("text framing cannot create a primitive");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "frame SAT primitive"
    ));
}

#[test]
fn sat_header_attributes_admit_retained_text_and_collection_slots() {
    use crate::test_support::with_context;
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let source = text_sphere_stream(1.0);
    let header = with_context(&source, &DecodePolicy::service(), |ctx| {
        cadmpeg_asm::sat::parse(ctx, &source)
            .expect("text stream parses")
            .header
            .as_kernel_header(ctx)
            .expect("kernel header")
    });
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let error = with_context(&[], &policy, |ctx| {
            super::header_attributes(
                ctx,
                &header,
                crate::dialect::Family::Asm,
                &mut std::collections::BTreeMap::new(),
            )
        })
        .expect_err("attribute admission uses the caller budget");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }
}

#[test]
fn text_header_magic_uses_the_admitted_first_field_scan() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = vec![b'7'; 1 << 20];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input");
    let error = super::looks_like_text_stream(&ctx, &bytes)
        .expect_err("the digit run must be admitted before scanning");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal expected");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "SAT text header line");
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(super::looks_like_text_stream(&ctx, &bytes),
        Err(CodecError::ResourceLimit(original)) if original == limit));
    let service = cadmpeg_test_support::service_decode_context();
    for invalid in [
        &b"70 0 6 0\n3"[..],
        b"700\t0 6 0\n3",
        b"700x 0 6 0\n3",
        b" 700 0 6 0\n3",
    ] {
        assert!(!super::looks_like_text_stream(&service, invalid).expect("invalid discriminant"));
    }
    assert!(super::looks_like_text_stream(&service, b"000700 0 6 0\n3")
        .expect("leading zeroes preserve decimal magic"));
}

#[test]
fn text_header_detection_has_an_exact_first_line_work_boundary() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    // First field: one separator probe, four extent probes, three UTF-8
    // bytes, three integer bytes. Each later field: two separator probes,
    // two extent probes, one UTF-8 byte and one integer byte. Final newline:
    // one probe. The fixed three-digit discriminant requires no scan charge.
    const WORK: u64 = (1 + 4 + 3 + 3) + 3 * (2 + 2 + 1 + 1) + 1;
    let mut bytes = b"700 0 6 0\n3".to_vec();
    bytes.resize(1 << 20, b'7');
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = WORK;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input");
    assert!(super::looks_like_text_stream(&ctx, &bytes).expect("exact header work fits"));
    let error = ctx
        .charge_work(1, "after exact first line")
        .expect_err("no unused work");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.used == WORK));
    policy.limits.max_work_units = WORK - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input");
    let error = super::looks_like_text_stream(&ctx, &bytes).expect_err("newline probe cannot fit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "SAT text header line"
            && limit.used == WORK - 1 && limit.additional == 1));
}
