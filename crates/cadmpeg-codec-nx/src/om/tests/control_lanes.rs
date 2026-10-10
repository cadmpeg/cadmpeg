// SPDX-License-Identifier: Apache-2.0
//! Offset-store control-lane grammar tests.

use crate::om::OffsetStoreControlForm;

fn offset_store_control_form_matches(
    control: &[u8],
    first_record: Option<&[u8]>,
    expected: Option<OffsetStoreControlForm>,
) -> bool {
    crate::test_support::with_decode_context(|ctx| -> Result<bool, cadmpeg_core::CodecError> {
        match crate::om::offset_store_control_form(ctx, control, first_record)? {
            Some((form, storage)) => {
                let matches = expected.as_ref() == Some(&form);
                drop(form);
                drop(storage);
                Ok(matches)
            }
            None => Ok(expected.is_none()),
        }
    })
    .unwrap()
}

#[test]
fn product_anchored_control_lane_crosses_the_first_column_boundary() {
    let control = [0x11, 0x01, 0x00, 0xe0];
    let mut first_record = vec![0x38, 0x01, 0x00];
    first_record.extend_from_slice(&7u32.to_le_bytes());
    first_record.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");

    assert!(offset_store_control_form_matches(
        &control,
        Some(&first_record),
        Some(OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(3, 0x111).unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([0x0001_38e0, 7]).unwrap(),
        }),
    ));

    let mut duplicate = control.to_vec();
    duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(offset_store_control_form_matches(
        &duplicate,
        Some(&first_record),
        None
    ));

    let aligned_control = 7u32.to_le_bytes();
    let anchored_first_record = b"\x04\x01\x0eNX 2027.3102\0";
    assert!(offset_store_control_form_matches(
        &aligned_control,
        Some(anchored_first_record),
        None
    ));

    let zero_prefixed_control = [0x00, 0x52, 0x02, 0x00];
    let mut continued_record = vec![0x00];
    continued_record.extend_from_slice(&7u32.to_le_bytes());
    continued_record.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(offset_store_control_form_matches(
        &zero_prefixed_control,
        Some(&continued_record),
        Some(OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(1, 0).unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([594, 7]).unwrap(),
        }),
    ));
}

fn control_form_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = [0, 1, 0, 0, 0, 2, 0, 0];

    crate::test_support::with_decode_context_over(
        &bytes,
        |policy| {
            configure(policy);
        },
        |ctx| crate::om::offset_store_control_form(ctx, &bytes, None).unwrap_err(),
    )
}

#[test]
fn offset_control_form_refuses_collection_limit() {
    let error = control_form_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn offset_control_form_scratch_does_not_charge_retained_limit() {
    let bytes = [0, 1, 0, 0, 0, 2, 0, 0];
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let Some((form, storage)) =
                crate::om::offset_store_control_form(ctx, &bytes, None).unwrap()
            else {
                panic!("complete control grammar must parse");
            };
            drop(form);
            drop(storage);
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn offset_control_form_refuses_materialized_scratch_limit() {
    let bytes = [0, 1, 0, 0, 0, 2, 0, 0];
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "NX zero-prefixed control values",
        |ctx| {
            if let Some((form, storage)) = crate::om::offset_store_control_form(ctx, &bytes, None)?
            {
                drop(form);
                drop(storage);
            }
            Ok(())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
            && limit.operation == "NX zero-prefixed control values")
    );
}

#[test]
fn point_ordinal_decimal_parse_preserves_values_and_refuses_work() {
    for (name, expected) in [
        ("Point123", Some(123)),
        ("Point0", None),
        ("Point", None),
        ("Point12x", None),
        ("Point4294967296", None),
        ("Other123", None),
    ] {
        assert_eq!(
            crate::test_support::with_decode_context(|ctx| {
                crate::om::parse_positive_decimal_suffix(ctx, name, "Point")
            })
            .unwrap(),
            expected
        );
    }
    for name in ["Point123", "Point0", "Point4294967296"] {
        let error = crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX point ordinal decimal parse",
            |ctx| crate::om::parse_positive_decimal_suffix(ctx, name, "Point"),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == cadmpeg_core::decode::u64_from_index(name.len() - "Point".len()))
        );
    }
}

#[test]
fn control_interpretations_do_not_double_the_live_vector_peak() {
    let zero = [0, 7, 0, 0, 0, 8, 0, 0];
    let mut product = vec![7, 0, 0, 0, 8, 0, 0, 0];
    product.extend_from_slice(b"\x04\x01\x05NX \0");
    let mut prefixed = zero.to_vec();
    prefixed.extend_from_slice(b"\x04\x01\x05NX \0\0");
    for (bytes, slots) in [
        (
            zero.as_slice(),
            std::mem::size_of::<crate::om::control_word::ControlWord24>(),
        ),
        (product.as_slice(), std::mem::size_of::<u32>()),
        (prefixed.as_slice(), std::mem::size_of::<u32>()),
    ] {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_materialized_bytes = u64::try_from(4 * slots).unwrap(),
            |ctx| {
                let Some((form, storage)) =
                    crate::om::offset_store_control_form(ctx, bytes, None).unwrap()
                else {
                    panic!("complete control grammar must parse");
                };
                drop(form);
                drop(storage);
                assert_eq!(ctx.resource_refusal(), None);
            },
        );
    }
}
