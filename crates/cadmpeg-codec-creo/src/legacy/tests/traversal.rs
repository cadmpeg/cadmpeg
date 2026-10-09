// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{decimal, line, numeric_array::NumericArray, NumericRun};

fn work_refusal<T>(
    operation: &'static str,
    run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> cadmpeg_core::decode::ResourceLimit {
    let CodecError::ResourceLimit(resource) =
        crate::test_support::last_refusal_at(&[], ResourceDimension::WorkUnits, operation, run)
    else {
        panic!("expected resource refusal");
    };
    resource
}

#[test]
fn legacy_line_search_does_not_visit_trailing_rows() {
    let short = b"row\n";
    let mut long = short.to_vec();
    long.extend_from_slice(&[b'x'; 256]);
    let boundary = work_refusal("creo legacy line scan", |ctx| line(ctx, short, 0));
    assert_eq!(
        work_refusal("creo legacy line scan", |ctx| line(ctx, &long, 0)),
        boundary
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| line(ctx, &long, 0)).expect("line admission"),
        Some((&b"row"[..], 4))
    );
}

#[test]
fn legacy_decimal_zero_run_is_charged_and_overflow_stops() {
    let zeros = [b'0'; 256];
    assert_eq!(
        crate::test_support::assert_work_boundaries(
            &["creo legacy decimal digits"],
            |ctx| decimal(ctx, &zeros, 0)
        ),
        Some((0, zeros.len()))
    );
    let overflow = b"4294967296";
    let mut tail = overflow.to_vec();
    tail.extend_from_slice(&zeros);
    let boundary = work_refusal("creo legacy decimal digits", |ctx| {
        decimal(ctx, overflow, 0)
    });
    assert_eq!(
        work_refusal("creo legacy decimal digits", |ctx| decimal(ctx, &tail, 0)),
        boundary
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| decimal(ctx, &tail, 0))
            .expect("overflow admission"),
        None
    );
}

#[test]
fn numeric_extent_overflow_does_not_visit_trailing_dimensions() {
    let extents = vec![u32::MAX; 3];
    let mut longer = extents.clone();
    longer.extend_from_slice(&[1; 256]);
    let boundary = work_refusal("creo numeric array extent validation", |ctx| {
        NumericArray::try_new(ctx, extents.clone(), Vec::<NumericRun<u32>>::new())
    });
    assert_eq!(
        work_refusal("creo numeric array extent validation", |ctx| {
            NumericArray::try_new(ctx, longer.clone(), Vec::<NumericRun<u32>>::new())
        }),
        boundary
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| NumericArray::try_new(
            ctx,
            longer.clone(),
            Vec::<NumericRun<u32>>::new()
        ))
        .expect("extent admission")
        .is_none()
    );
}

#[test]
fn compact_real_is_bounded_and_preserves_bits() {
    use super::super::{compact_real, Real};
    assert_eq!(
        compact_real(b"3FF0000000000000"),
        Some(Real(0x3ff0_0000_0000_0000))
    );
    assert_eq!(compact_real(b"3FFR"), Some(Real(0x3fff_ffff_ffff_ffff)));
    assert_eq!(compact_real(b"FFFFFFFFFFFFFFFF"), None);
    assert_eq!(compact_real(b"00000000000000000"), None);
    assert_eq!(compact_real(b"3ff"), None);
}

#[test]
fn integer_grammar_rejects_plus_and_nondecimal_text() {
    for bytes in [
        b"+0".as_slice(),
        b"+1",
        b"--1",
        b" 1",
        b"1 ",
        b"1_0",
        b"",
        b"-",
        b"\xff",
        "١".as_bytes(),
    ] {
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::super::signed_integer(ctx, bytes))
                .expect("signed grammar"),
            None
        );
        assert_eq!(
            crate::decode::with_test_decode_ctx(|ctx| super::super::unsigned_integer(ctx, bytes))
                .expect("unsigned grammar"),
            None
        );
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::signed_integer(ctx, b"-0"))
            .expect("signed zero"),
        Some(0)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::unsigned_integer(ctx, b"-0"))
            .expect("unsigned sign"),
        None
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::signed_integer(ctx, b"0001"))
            .expect("leading zeros"),
        Some(1)
    );
}

#[test]
fn object_completeness_overflow_does_not_visit_trailing_dimensions() {
    let mut dimensions = vec![u32::MAX; 3];
    let short = super::super::ObjectPayload::Array {
        dimensions: dimensions.clone(),
        elements: Vec::new(),
        complete: false,
    };
    dimensions.extend_from_slice(&[1; 256]);
    let long = super::super::ObjectPayload::Array {
        dimensions,
        elements: Vec::new(),
        complete: false,
    };
    let boundary = work_refusal("creo object array extent traversal", |ctx| {
        short.is_complete(ctx)
    });
    assert_eq!(
        work_refusal("creo object array extent traversal", |ctx| long
            .is_complete(ctx)),
        boundary
    );
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| long.is_complete(ctx))
            .expect("extent admission")
    );
}

#[test]
fn string_completeness_stops_at_first_unsupported_value() {
    use super::super::{Continuation, StringPayload, StringValue};
    let mut values = vec![Err(Continuation {
        rows: 0..0,
        count: std::num::NonZeroUsize::MIN,
    })];
    let short = StringPayload::Array {
        dimensions: vec![1],
        values: values.clone(),
        continuation: None,
        complete: false,
        accepted_value_indices: Vec::new(),
    };
    values.extend(std::iter::repeat_n(Ok(StringValue::Null), 256));
    let long = StringPayload::Array {
        dimensions: vec![257],
        values,
        continuation: None,
        complete: false,
        accepted_value_indices: (1..257).collect(),
    };
    let boundary = work_refusal("creo string array completeness traversal", |ctx| {
        short.is_complete(ctx)
    });
    assert_eq!(
        work_refusal("creo string array completeness traversal", |ctx| long
            .is_complete(ctx)),
        boundary
    );
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| long.is_complete(ctx))
            .expect("string admission")
    );
}


#[test]
fn legacy_empty_and_fixed_lanes_are_free_and_preserve_original_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use super::super::{array_dimensions, byte_string_value, NullToken, ObjectPayload,
        Persistence, StringPayload, StringValue, text_field};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert_eq!(line(&ctx, &[], 0).expect("empty line"), Some((&[][..], 0)));
    assert_eq!(line(&ctx, &[], 1).expect("missing line"), None);
    assert_eq!(decimal(&ctx, &[], 0).expect("empty decimal"), None);
    assert!(array_dimensions(&ctx, &[]).expect("empty dimensions").is_none());
    assert_eq!(byte_string_value(&ctx, b"NULL", NullToken::RepresentsNull)
        .expect("fixed null token"), StringValue::Null);
    assert!(!ObjectPayload::Null.is_complete(&ctx).expect("fixed object variant"));
    let strings = StringPayload::Scalar { value: StringValue::Null };
    assert!(!strings.is_complete(&ctx).expect("fixed string variant"));
    assert_eq!(strings.element_count(&ctx).expect("fixed scalar count"), 1);
    assert_eq!(strings.undecoded_encoding_count(&ctx).expect("fixed encoding count"), 0);
    assert_eq!(Persistence::default().principal_unit_system(&ctx).expect("no unit rows"), None);
    let mut empty = "";
    assert_eq!(text_field(&ctx, &mut empty, false).expect("empty field"), None);
    let original = ctx.charge_work_limit(1, "seed fixed legacy refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(line(&ctx, &[], 1), Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(decimal(&ctx, &[], 0), Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(array_dimensions(&ctx, &[]), Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(byte_string_value(&ctx, b"NULL", NullToken::RepresentsNull),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(ObjectPayload::Null.is_complete(&ctx),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(strings.is_complete(&ctx), Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(strings.element_count(&ctx), Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(strings.undecoded_encoding_count(&ctx),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert!(matches!(Persistence::default().principal_unit_system(&ctx),
        Err(CodecError::ResourceLimit(r)) if r == original));
    let mut source = " pending";
    assert!(matches!(text_field(&ctx, &mut source, false),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert_eq!(source, " pending");
}

#[test]
fn legacy_line_and_decimal_admit_only_present_source_bytes() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    for (source, expected) in [(b"ab".as_slice(), b"ab".as_slice()), (b"a\nTAIL".as_slice(), b"a".as_slice())] {
        for allowed in 0..=2 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let result = line(&ctx, source, 0);
            if allowed < 2 {
                let CodecError::ResourceLimit(r) = result.expect_err("next byte visit") else {
                    panic!("work refusal");
                };
                assert_eq!(r.dimension, ResourceDimension::WorkUnits);
                assert_eq!(r.operation, "creo legacy line scan");
                assert_eq!((r.used, r.additional), (allowed, 1));
            } else {
                assert_eq!(result.expect("two present bytes"), Some((expected, 2)));
                let r = ctx.charge_work_limit(1, "after two line visits").expect_err("exact cap");
                assert_eq!((r.used, r.additional), (2, 1));
            }
        }
    }
    for (source, expected, visits) in [
        (b"007".as_slice(), (7, 3), 3),
        (b"7xTAIL".as_slice(), (7, 1), 2),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = visits;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(decimal(&ctx, source, 0).expect("digits and first delimiter"), Some(expected));
        let r = ctx.charge_work_limit(1, "after decimal visits").expect_err("exact cap");
        assert_eq!((r.used, r.additional), (visits, 1));
    }
}

#[test]
fn legacy_text_fields_admit_present_characters_and_preserve_unicode_boundaries() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use super::super::text_field;
    for (source, remainder, visits, first_visits) in [
        ("é", "", 1, 1),
        (" \u{2003}é next", "next", 4, 3),
    ] {
        for allowed in 0..=visits {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut pending = source;
            let result = text_field(&ctx, &mut pending, false);
            if allowed < visits {
                let CodecError::ResourceLimit(r) = result.expect_err("next character visit") else {
                    panic!("work refusal");
                };
                assert_eq!(r.dimension, ResourceDimension::WorkUnits);
                assert_eq!(r.operation, if allowed < first_visits {
                    "creo text field whitespace"
                } else {
                    "creo text field boundary"
                });
                assert_eq!((r.used, r.additional), (allowed, 1));
                assert_eq!(pending, source);
                let mut empty = "";
                assert!(matches!(text_field(&ctx, &mut empty, false),
                    Err(CodecError::ResourceLimit(original)) if original == r));
            } else {
                assert_eq!(result.expect("present characters"), Some("é"));
                assert_eq!(pending, remainder);
                let r = ctx.charge_work_limit(1, "after field character visits").expect_err("exact cap");
                assert_eq!((r.used, r.additional), (visits, 1));
            }
        }
    }
}

#[test]
fn legacy_scope_extent_visits_exclude_terminal_probe_and_preserve_empty_output() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use super::super::{scan, Persistence};
    for allowed in 0..=2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowed;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = scan(&ctx, &[], [0..0, 0..0]);
        if allowed < 2 {
            let CodecError::ResourceLimit(r) = result.expect_err("next extent") else {
                panic!("work refusal");
            };
            assert_eq!(r.dimension, ResourceDimension::WorkUnits);
            assert_eq!(r.operation, "creo legacy scope extent traversal");
            assert_eq!((r.used, r.additional), (allowed, 1));
            assert!(matches!(scan(&ctx, &[], std::iter::empty()),
                Err(CodecError::ResourceLimit(original)) if original == r));
        } else {
            assert_eq!(result.expect("two present extents"), Persistence::default());
            let r = ctx.charge_work_limit(1, "after two extent visits").expect_err("exact cap");
            assert_eq!((r.used, r.additional), (2, 1));
        }
    }
}

#[test]
fn legacy_pruning_visits_only_expired_rows_and_preserves_records() {
    use super::super::{parent_object_offsets, string_records, StringPayload, StringValue};
    let data = b"@root 1 0\n@child 2 0\n0 1 ->\n1 2 ->\n0 1 ->\n";
    let scopes = vec![super::scope_fixture(data, 0..data.len())];
    let parents = crate::test_support::assert_work_boundaries(
        &["creo legacy active object pruning"],
        |ctx| parent_object_offsets(ctx, &scopes),
    );
    assert_eq!(parents.len(), 1);
    assert_eq!(parents.get(&scopes[0].values[1].offset), Some(&scopes[0].values[0].offset));
    let data = b"@names 1 10\n@other 2 10\n0 1 [1]\n1 1 value\n0 2 tail\n";
    let scopes = vec![super::scope_fixture(data, 0..data.len())];
    let parents = crate::decode::with_test_decode_ctx(|ctx| parent_object_offsets(ctx, &scopes))
        .expect("fixture parents");
    let (records, incomplete, unresolved) = crate::test_support::assert_work_boundaries(
        &["creo legacy active string array pruning"],
        |ctx| string_records(ctx, data, &scopes, &parents),
    );
    assert_eq!((records.len(), incomplete, unresolved), (2, 0, 0));
    assert_eq!(records[0].name, "names");
    let StringPayload::Array { values, complete, .. } = &records[0].payload else {
        panic!("string array");
    };
    assert!(*complete);
    assert_eq!(values, &[Ok(StringValue::Utf8 { text: "value".to_owned() })]);
    assert_eq!(records[1].name, "other");
}


#[test]
fn object_array_complete_shape_admits_two_extent_visits() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use super::super::ObjectPayload;
    let payload = ObjectPayload::Array {
        dimensions: vec![2, 2],
        elements: [
            "creo:legacy_ascii:object#1",
            "creo:legacy_ascii:object#2",
            "creo:legacy_ascii:object#3",
            "creo:legacy_ascii:object#4",
        ].map(str::to_owned).to_vec(),
        complete: true,
    };
    for allowed in 0..=2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowed;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = payload.is_complete(&ctx);
        if allowed < 2 {
            let CodecError::ResourceLimit(r) = result.expect_err("next extent multiplication") else {
                panic!("work refusal");
            };
            assert_eq!(r.dimension, ResourceDimension::WorkUnits);
            assert_eq!(r.operation, "creo object array extent traversal");
            assert_eq!((r.used, r.additional), (allowed, 1));
        } else {
            assert!(result.expect("two extent multiplications"));
            let r = ctx.charge_work_limit(1, "after object extent visits").expect_err("exact cap");
            assert_eq!((r.used, r.additional), (2, 1));
        }
    }
}
