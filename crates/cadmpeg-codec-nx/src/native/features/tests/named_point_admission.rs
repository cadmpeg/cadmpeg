// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_sketch_named_point_block_uses;
use crate::native::features::feature_sketch_preceding_named_point_uses;
use crate::native::features::feature_sketch_references;
use crate::native::features::offset_store_named_points;

#[test]
fn named_point_admits_fixture() {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let admitted = offset_store_named_points(&container).expect("admitted named point route");
    assert!(!admitted.is_empty());
    assert!(admitted[0].data_blocks.len() >= 2);
}

#[test]
fn named_point_block_use_admits_fixture() {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let references = feature_sketch_references(&container).expect("named point references");
    let points = offset_store_named_points(&container).expect("named point blocks");
    assert!(!feature_sketch_named_point_block_uses(&references, &points)
        .expect("admitted block use")
        .is_empty());
}

#[test]
fn preceding_named_point_admits_fixture() {
    let reference = |ordinal, block: &str| crate::native::features::FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        position: crate::om::sketch_references::SketchReferencePosition::new(
            crate::om::sketch_references::SketchReferenceCount::from_count_byte(2),
            ordinal,
        )
        .expect("sketch position"),
        token: crate::om::reference_index::ReferenceIndexToken::from_wire(
            12 + ordinal,
            &[
                0xf0,
                u8::try_from(12 + ordinal).expect("fixture value fits u8"),
            ],
        )
        .expect("reference token"),
        data_block: Some(block.into()),
        source_offset: 300 + u64::from(ordinal),
    };
    let references = [
        reference(0, "nx:om-data-blocks-2:block#12"),
        reference(1, "nx:om-data-blocks-2:block#13"),
    ];
    let point = crate::native::features::OffsetStoreNamedPoint {
        id: "nx:offset-store:named-point#2-10".into(),
        name: "Point1".into(),
        data_blocks: [
            "nx:om-data-blocks-2:block#10",
            "nx:om-data-blocks-2:block#11",
        ]
        .map(str::to_string)
        .to_vec(),
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(
                    crate::test_support::test_bytes::shifted_f64_bytes(value),
                )
                .expect("shifted scalar"),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let points = [point];
    let admitted = feature_sketch_preceding_named_point_uses(&references, &points)
        .expect("admitted preceding named-point route");
    assert!(!admitted.is_empty());
}
