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
    };
    dimensions.extend_from_slice(&[1; 256]);
    let long = super::super::ObjectPayload::Array {
        dimensions,
        elements: Vec::new(),
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
    };
    values.extend(std::iter::repeat_n(Ok(StringValue::Null), 256));
    let long = StringPayload::Array {
        dimensions: vec![257],
        values,
        continuation: None,
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
