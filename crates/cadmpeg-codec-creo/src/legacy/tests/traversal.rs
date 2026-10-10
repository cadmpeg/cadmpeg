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
        super::super::object_array_is_complete(ctx,
            match &short { super::super::ObjectPayload::Array { dimensions, .. } => dimensions, _ => unreachable!() },
            &[])
    });
    assert_eq!(
        work_refusal("creo object array extent traversal", |ctx| super::super::object_array_is_complete(ctx,
            match &long { super::super::ObjectPayload::Array { dimensions, .. } => dimensions, _ => unreachable!() },
            &[])),
        boundary
    );
    assert!(
        !crate::decode::with_test_decode_ctx(|ctx| super::super::object_array_is_complete(ctx,
            match &long { super::super::ObjectPayload::Array { dimensions, .. } => dimensions, _ => unreachable!() }, &[]))
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
        complete: false,
        accepted_value_indices: Vec::new(),
    };
    values.extend(std::iter::repeat_n(Ok(StringValue::Null), 256));
    let long = StringPayload::Array {
        dimensions: vec![257],
        values,
        complete: false,
        accepted_value_indices: (1..257).collect(),
    };
    for (payload, count) in [(&short, 0), (&long, 256)] {
        let wire = serde_json::to_value(payload).expect("supported string array wire");
        assert_eq!(wire["complete"], false);
        assert_eq!(wire["values"].as_array().expect("supported values").len(), count);
        assert!(wire["values"].as_array().expect("values").iter()
            .all(|value| value == &serde_json::json!({"form": "null"})));
    }
    for count in [1, 257] {
        let mut bytes = format!("@names 1 10\n0 1 [{count}]\n1 1 skipped\n$continued\n").into_bytes();
        for _ in 1..count {
            bytes.extend_from_slice(b"1 1 NULL\n");
        }
        let persistence = crate::decode::with_test_decode_ctx(|ctx|
            super::super::scan(ctx, &bytes, std::iter::once(0..bytes.len())))
            .expect("unsupported-first-value parser");
        assert_eq!(persistence.incomplete_string_array_count, 1);
        assert_eq!(persistence.unresolved_string_value_count, 1);
        let wire = serde_json::to_value(&persistence.string_values[0].payload).expect("parsed wire");
        assert_eq!(wire["complete"], false);
        assert_eq!(wire["values"].as_array().expect("supported values").len(), count - 1);
        assert!(wire["values"].as_array().expect("values").iter()
            .all(|value| value == &serde_json::json!({"form": "null"})));
    }
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
    assert_eq!(serde_json::to_value(&strings).expect("fixed string wire"),
        serde_json::json!({"form": "scalar", "value": {"form": "null"}}));
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
    assert!(matches!(Persistence::default().principal_unit_system(&ctx),
        Err(CodecError::ResourceLimit(r)) if r == original));
    let mut source = " pending";
    assert!(matches!(text_field(&ctx, &mut source, false),
        Err(CodecError::ResourceLimit(r)) if r == original));
    assert_eq!(source, " pending");
}

#[test]
fn legacy_line_and_decimal_admit_only_present_source_bytes() {
    for (source, expected) in [(b"ab".as_slice(), b"ab".as_slice()), (b"a\nTAIL".as_slice(), b"a".as_slice())] {
        let result = crate::test_support::assert_work_boundaries(&["creo legacy line scan"], |ctx| line(ctx, source, 0));
        assert_eq!(result, Some((expected, 2)));
    }
    for (source, expected) in [(b"007".as_slice(), (7, 3)), (b"7xTAIL".as_slice(), (7, 1))] {
        let result = crate::test_support::assert_work_boundaries(&["creo legacy decimal digits"], |ctx| decimal(ctx, source, 0));
        assert_eq!(result, Some(expected));
    }
}

#[test]
fn legacy_text_fields_admit_present_characters_and_preserve_unicode_boundaries() {
    use super::super::text_field;
    for (source, remainder, operations) in [
        ("é", "", ["creo text field whitespace"].as_slice()),
        (" \u{2003}é next", "next", ["creo text field whitespace", "creo text field boundary"].as_slice()),
    ] {
        let actual = crate::test_support::assert_work_boundaries(
            operations, |ctx| {
                let mut pending = source;
                let result = text_field(ctx, &mut pending, false);
                if result.is_err() { assert_eq!(pending, source); }
                result.map(|value| (value, pending))
            });
        assert_eq!(actual, (Some("é"), remainder));
    }
}

#[test]
fn legacy_scope_extent_visits_exclude_terminal_probe_and_preserve_empty_output() {
    use super::super::{scan, Persistence};
    let actual = crate::test_support::assert_work_boundaries(&["creo legacy scope extent traversal"],
        |ctx| scan(ctx, &[], [0..0, 0..0]));
    assert_eq!(actual, Persistence::default());
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
fn object_array_complete_shape_uses_stored_completeness() {
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    assert!(payload.is_complete(&ctx).expect("stored completeness is free"));
}
