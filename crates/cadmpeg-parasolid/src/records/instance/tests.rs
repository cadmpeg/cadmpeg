// SPDX-License-Identifier: Apache-2.0
use super::{FieldValue, RealBits};
use crate::records::DataType;
use cadmpeg_core::decode::View;

#[test]
fn every_real_payload_round_trips_without_finite_filtering() {
    for bits in [
        0u64,
        1,
        0x8000_0000_0000_0000,
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff8_1234_5678_9abc,
    ] {
        let bytes = bits.to_be_bytes();
        let value = FieldValue::read(DataType::Real, &mut View::over_retained(&bytes)).unwrap();
        assert_eq!(value, FieldValue::RealBits(RealBits(bits)));
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(wire["value"], format!("{bits:016x}"));
        assert_eq!(serde_json::from_value::<FieldValue>(wire).unwrap(), value);
    }
}

#[test]
fn pointer_status_is_independent_of_nullness_and_requires_canonical_positive_storage() {
    for target in [1u16, 12, 125, 32767] {
        for status in [0u8, 1] {
            let mut bytes = target.to_be_bytes().to_vec();
            bytes.push(status);
            assert_eq!(
                FieldValue::read(DataType::Pointer, &mut View::over_retained(&bytes)),
                Some(FieldValue::Pointer {
                    target: std::num::NonZeroU32::new(u32::from(target)).unwrap(),
                    status: status != 0
                })
            );
        }
    }
    for bytes in [&[0, 0, 1][..], &[0xff, 0xff, 0, 0, 1], &[0, 1, 2]] {
        assert!(FieldValue::read(DataType::Pointer, &mut View::over_retained(bytes)).is_none());
    }
}

#[test]
fn variable_hvec_spans_stop_at_the_last_declared_value_and_keep_nonfinite_bits() {
    let declaration = b"\0\x29\x03CI\x08term_use\0\0\0\x01\x01cCZ";
    crate::test_support::with_decode_context(|_ctx| {
        let (plan, schema_end) = crate::records::tests::read(declaration, 0)
            .unwrap()
            .unwrap();
        assert_eq!(schema_end, declaration.len());
        let mut bytes = 2u32.to_be_bytes().to_vec();
        bytes.extend([0, 86, b'T', b'?']);
        for value in 0..22 {
            bytes.extend(f64::from(value).to_bits().to_be_bytes());
        }
        let end = bytes.len();
        bytes[end - 8..].copy_from_slice(&0x7ff8_0000_0000_0001u64.to_be_bytes());
        bytes.extend([0, 40, 0, 1]);
        let mut view = View::over_retained(&bytes);
        let instance = super::read_status_instance(&mut view, &plan).unwrap();
        assert_eq!(instance.variable_length, Some(2));
        assert_eq!(view.position(), end);
        assert_eq!(
            instance
                .fields
                .iter()
                .map(std::iter::ExactSizeIterator::len)
                .collect::<Vec<_>>(),
            [1, 1, 176]
        );
        for length in 0..end {
            assert!(
                super::read_status_instance(&mut View::over_retained(&bytes[..length]), &plan)
                    .is_none()
            );
        }
    });
}

#[test]
fn nontransmitted_variable_fields_keep_the_count_and_zero_width_span() {
    let declaration = b"\0\x29\x02CDA\x04skip\0\0\0\x02\x01d\0Z";
    crate::test_support::with_decode_context(|_ctx| {
        let (plan, _) = crate::records::tests::read(declaration, 0)
            .unwrap()
            .unwrap();
        let bytes = [0, 0, 0, 4, 0, 9, b'T'];
        let mut view = View::over_retained(&bytes);
        let instance = super::read_status_instance(&mut view, &plan).unwrap();
        assert_eq!(instance.variable_length, Some(4));
        assert_eq!(instance.fields, [6..7, 7..7]);
        assert_eq!(view.position(), bytes.len());
    });
}
