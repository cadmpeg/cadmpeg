// SPDX-License-Identifier: Apache-2.0
//! Indexed store envelopes and product containment boundaries.

use crate::om::indexed_sections;
use crate::om::ProductRecordRange;
use crate::test_support::test_om::control_root_offset_only_indexed_om_section;
use crate::test_support::test_om::indexed_om_section;
use crate::test_support::test_om::offset_only_indexed_om_section;

fn product_record_count_within(
    ranges: &[crate::om::ProductRecordRange],
    lower: usize,
    upper: usize,
) -> usize {
    crate::test_support::with_decode_context(|ctx| {
        crate::om::product_record_count_within(ctx, ranges, lower, upper)
    })
    .unwrap()
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections =
        crate::test_support::with_decode_context(|ctx| indexed_sections(ctx, &bytes)).unwrap();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    let (control, column_storage, records) =
        sections[0].as_offset_only().expect("offset-only store");
    assert_eq!(control.bytes, &[0, 0, 0, 0, 0, 1, 0, 0]);
    assert_eq!(records.len(), 2);
    assert_eq!(
        column_storage,
        [records[0].bytes, records[1].bytes].concat()
    );
    assert!(records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert!(records[1].bytes.ends_with(b"\0"));
    let expressions =
        crate::test_support::with_decode_context(|ctx| sections[0].numeric_expressions(ctx))
            .unwrap();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name.as_str(), "length");
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| expressions[0].constant_value(ctx))
            .unwrap()
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(25.0)
    );
}

#[test]
fn om_indexed_layout_materializes_both_store_forms_without_semantic_drift() {
    for bytes in [indexed_om_section(), offset_only_indexed_om_section()] {
        let source = std::sync::Arc::<[u8]>::from(bytes.as_slice());
        crate::test_support::with_decode_context(|ctx| {
            let section = indexed_sections(ctx, &bytes)
                .unwrap()
                .into_iter()
                .next()
                .expect("indexed fixture has one section");
            let (layout, storage) =
                crate::om::cache::IndexedSectionLayout::from_section(ctx, &section, &source)
                    .unwrap();
            let layout = layout.expect("validated indexed section has a cache layout");
            assert_eq!(layout.materialize(ctx).unwrap(), section);
            drop(layout);
            drop(storage);
        });
    }
}

#[test]
fn om_offset_only_index_accepts_one_root_record_inside_control_block() {
    let bytes = control_root_offset_only_indexed_om_section();
    let sections =
        crate::test_support::with_decode_context(|ctx| indexed_sections(ctx, &bytes)).unwrap();

    assert_eq!(sections.len(), 1);
    let (control, _, records) = sections[0].as_offset_only().expect("offset-only store");
    assert!(control
        .bytes
        .windows(b"NX 2027.3102".len())
        .any(|window| window == b"NX 2027.3102"));
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].bytes, &[0; 32]);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| sections[0].numeric_expressions(ctx))
            .unwrap()[0]
            .name
            .as_str(),
        "length"
    );
}

#[test]
fn om_offset_only_index_ignores_product_marker_crossing_record_boundary() {
    use cadmpeg_core::decode::View;

    let mut bytes = control_root_offset_only_indexed_om_section();
    let class_name = b"UGS::ModlFeature";
    let class_start = bytes
        .windows(class_name.len())
        .position(|window| window == class_name)
        .expect("class declaration");
    let index_start = class_start + class_name.len() + 1;
    let first = usize::try_from(View::u32_le_at(&bytes, index_start + 4).unwrap()).unwrap();
    let product = b"\x04\x01\x0eNX 2027.3102\0";
    let split = 3;
    bytes[first - split..first].copy_from_slice(&product[..split]);
    bytes[first..first + product.len() - split].copy_from_slice(&product[split..]);

    assert_eq!(
        crate::test_support::with_decode_context(|ctx| indexed_sections(ctx, &bytes))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn om_product_record_count_respects_containment_boundaries() {
    let ranges = [
        ProductRecordRange { start: 10, end: 20 },
        ProductRecordRange { start: 30, end: 40 },
        ProductRecordRange { start: 50, end: 60 },
    ];

    assert_eq!(product_record_count_within(&ranges, 10, 20), 1);
    assert_eq!(product_record_count_within(&ranges, 11, 20), 0);
    assert_eq!(product_record_count_within(&ranges, 10, 19), 0);
    assert_eq!(product_record_count_within(&ranges, 20, 60), 2);
    assert_eq!(product_record_count_within(&ranges, 20, 50), 1);
}

#[test]
fn om_offset_only_index_requires_one_supported_product_record() {
    let mut duplicate = control_root_offset_only_indexed_om_section();
    let first_column = duplicate
        .windows(32)
        .position(|window| window == [0; 32])
        .expect("zero first column");
    let duplicate_product = b"\x04\x01\x0eNX 2027.3102\0";
    duplicate[first_column..first_column + duplicate_product.len()]
        .copy_from_slice(duplicate_product);
    assert!(
        crate::test_support::with_decode_context(|ctx| indexed_sections(ctx, &duplicate))
            .unwrap()
            .is_empty()
    );

    let mut unsupported = control_root_offset_only_indexed_om_section();
    let product = unsupported
        .windows(b"\x05\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x05\x01\x0eNX 2027.3102\0")
        .expect("product record");
    unsupported[product] = 0x03;
    assert!(
        crate::test_support::with_decode_context(|ctx| indexed_sections(ctx, &unsupported))
            .unwrap()
            .is_empty()
    );
}
