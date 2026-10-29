// SPDX-License-Identifier: Apache-2.0
use super::{RegionFields, RegionType};
use crate::test_support::{owned_deltas, test_pmarks, with_decode_context};

#[test]
fn complete_region_layouts_preserve_source_fields_and_reject_truncation() {
    for layout in [7, 8, 9] {
        for region_type in *b"SV" {
            let prefix = test_pmarks::prefix(1);
            let region = owned_deltas::region(layout, 52, region_type);
            let mut bytes = prefix.clone();
            bytes.extend(&region);
            for end in prefix.len() + 1..bytes.len() {
                let owner =
                    with_decode_context(|_ctx| crate::records::DeltasRecords::read(&bytes[..end]))
                        .unwrap();
                assert_eq!(owner.census().instance_count(), 0);
                assert_eq!(owner.suffix(), prefix.len()..end);
            }
            let bytes = test_pmarks::complete(bytes);
            let owner =
                with_decode_context(|_ctx| crate::records::DeltasRecords::read(&bytes)).unwrap();
            assert_eq!(owner.census().instance_count(), 1);
            assert!(
                super::RegionLayout::from_schema(owner.definitions().last().unwrap().plan())
                    .is_some()
            );
            assert_eq!(owner.suffix().start, bytes.len());
        }
    }
}

#[test]
fn region_types_and_reference_cardinality_are_exact() {
    for byte in 0..=u8::MAX {
        let value = RegionType::try_from(byte);
        assert_eq!(value.is_ok(), matches!(byte, b'S' | b'V'));
        if let Ok(value) = value {
            assert_eq!(u8::from(value), byte);
        }
    }
    for count in 0..10 {
        assert_eq!(
            RegionFields::from_wire(u32::MAX, vec![1; count], RegionType::Solid).is_some(),
            matches!(count, 5..=7)
        );
    }
    for count in [5, 6, 7] {
        for slot in 0..count {
            let mut pointers = vec![1; count];
            pointers[slot] = 0;
            assert!(RegionFields::from_wire(1, pointers, RegionType::Void).is_none());
        }
    }
}

#[test]
fn retained_fitting_and_shcs_bytes_keep_first_and_ordinary_region_fields() {
    let fixture_0: &[u8] = &[
        0x00, 0x13, 0x09, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x49, 0x05, 0x66, 0x72, 0x61, 0x6d,
        0x65, 0x00, 0xe6, 0x00, 0x01, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e, 0x65, 0x72, 0x00, 0x0c,
        0x00, 0x01, 0x5a, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x01, 0x00, 0x03, 0x01,
        0x00, 0x34, 0x01, 0x00, 0x01, 0x01, 0x00, 0x39, 0x01, 0x00, 0x01, 0x01, 0x56, 0x00, 0x01,
        0x01, 0x00, 0x13, 0x00, 0x34, 0x00, 0x00, 0x00, 0x31, 0x00, 0x01, 0x01, 0x00, 0x03, 0x01,
        0x00, 0x01, 0x01, 0x00, 0x0c, 0x01, 0x00, 0x08, 0x01, 0x00, 0x01, 0x01, 0x53, 0x00, 0x01,
        0x01,
    ];
    let fixture_1: &[u8] = &[
        0x00, 0x13, 0x08, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x43, 0x41, 0x05, 0x6f, 0x77, 0x6e,
        0x65, 0x72, 0x00, 0x0c, 0x00, 0x01, 0x5a, 0x00, 0x53, 0x00, 0x00, 0x00, 0x1e, 0x00, 0x01,
        0x01, 0x00, 0x03, 0x01, 0x00, 0x01, 0x01, 0x00, 0x0d, 0x01, 0x00, 0x09, 0x01, 0x53, 0x00,
        0x01, 0x01, 0x00, 0x0d, 0xff, 0x00, 0x09, 0x00, 0x00, 0x00, 0x1d, 0x00, 0x01, 0x01, 0x00,
        0x03, 0x01, 0x00, 0x01, 0x01, 0x00, 0xe0, 0x01, 0x00, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00,
        0x53, 0x01, 0x00, 0x01, 0x01, 0x00, 0x13, 0x00, 0x0d, 0x00, 0x00, 0x00, 0x07, 0x00, 0x01,
        0x01, 0x00, 0x03, 0x01, 0x00, 0x53, 0x01, 0x00, 0x01, 0x01, 0x00, 0x59, 0x01, 0x56, 0x00,
        0x01, 0x01,
    ];
    for (fixture, first_xmt, ordinary_xmt, first_type, ordinary_type, references, offset) in [
        (fixture_0, 12, 52, 86, 83, vec![1, 3, 1, 12, 8, 1, 1], 61),
        (fixture_1, 83, 13, 83, 86, vec![1, 3, 83, 1, 89, 1], 80),
    ] {
        let mut bytes = test_pmarks::prefix(1);
        let prefix = bytes.len();
        bytes.extend(fixture);
        let instance_end = bytes.len();
        let bytes = test_pmarks::complete(bytes);
        with_decode_context(|_ctx| {
            let owner = crate::records::DeltasRecords::read(&bytes).unwrap();
            assert_eq!(owner.suffix().start, bytes.len());
            let regions: Vec<_> = owner
                .events()
                .iter()
                .filter_map(|event| match event.content() {
                    crate::records::deltas::EventContent::Instance(instance)
                        if instance.definition().plan().kind == 19 =>
                    {
                        Some((event, instance))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(regions.len(), 2);
            assert_eq!(u32::from(regions[0].1.xmt()), first_xmt);
            assert_eq!(u32::from(regions[1].1.xmt()), ordinary_xmt);
            assert_eq!(regions[1].0.range(), prefix + offset..instance_end);
            for ((_, instance), expected) in regions.iter().zip([first_type, ordinary_type]) {
                let type_index = instance
                    .definition()
                    .plan()
                    .fields
                    .iter()
                    .position(|field| field.name.as_str() == "type")
                    .unwrap();
                assert_eq!(bytes[instance.fields()[type_index].start], expected);
            }
            let instance = regions[1].1;
            let actual: Vec<_> = instance
                .definition()
                .plan()
                .fields
                .iter()
                .zip(instance.fields())
                .filter_map(|(field, span)| {
                    if field.data_type != crate::records::DataType::Pointer {
                        return None;
                    }
                    let mut view = cadmpeg_core::decode::View::over_retained(&bytes[span.clone()]);
                    match crate::records::instance::FieldValue::read(field.data_type, &mut view)
                        .unwrap()
                    {
                        crate::records::instance::FieldValue::Pointer { target, .. } => {
                            Some(target.get())
                        }
                        _ => unreachable!(),
                    }
                })
                .collect();
            assert_eq!(actual, references);
        });
    }
}
