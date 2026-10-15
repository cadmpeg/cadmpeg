// SPDX-License-Identifier: Apache-2.0

use crate::native::features::{
    feature_input_blocks, feature_operation_labels, feature_operation_records,
    feature_sketch_construction_inputs, feature_sketch_construction_payloads,
    feature_sketch_fixed_points, feature_sketch_payload_fixed_pairs,
    feature_sketch_payload_mixed_pairs, feature_sketch_payload_named_records,
    feature_sketch_payload_names, feature_sketch_payload_scalars, feature_sketch_point_groups,
    feature_sketch_point_uses, feature_sketch_points, feature_sketch_records,
    feature_sketch_references,
};

#[derive(Clone, Copy)]
enum SketchPointRoute {
    Floating,
    Fixed,
}

fn assert_sketch_point_admits(route: SketchPointRoute) {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let (records, names, scalars, pairs) = crate::test_support::with_decode_context(|ctx| {
        let labels = feature_operation_labels(&container)?;
        let operations = feature_operation_records(ctx, &container)?;
        let inputs = feature_input_blocks(&container)?;
        let references = feature_sketch_references(&container)?;
        let sketches = feature_sketch_records(&labels, &operations, &inputs, &references);
        let constructions = feature_sketch_construction_inputs(&sketches, &references)?;
        let payloads = feature_sketch_construction_payloads(ctx, &container, &constructions)?;
        let names = feature_sketch_payload_names(&container, &constructions)?;
        let scalars = feature_sketch_payload_scalars(&container, &constructions)?;
        let pairs = feature_sketch_payload_fixed_pairs(&container, &payloads)?;
        let mixed = feature_sketch_payload_mixed_pairs(&container, &payloads)?;
        let records =
            feature_sketch_payload_named_records(&payloads, &names, &scalars, &pairs, &mixed);
        Ok::<_, cadmpeg_core::CodecError>((records, names, scalars, pairs))
    })
    .expect("sketch point input records");
    let decode = || match route {
        SketchPointRoute::Floating => feature_sketch_points(&records, &names, &scalars).len(),
        SketchPointRoute::Fixed => feature_sketch_fixed_points(&records, &names, &pairs).len(),
    };
    assert!(decode() > 0);
}

macro_rules! sketch_point_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_sketch_point_admits($route);
        }
    };
}

sketch_point_admit_test!(sketch_point_admits_fixture, SketchPointRoute::Floating);
sketch_point_admit_test!(sketch_fixed_point_admits_fixture, SketchPointRoute::Fixed);

#[test]
fn sketch_point_use_admits_fixture() {
    let point = crate::native::features::FeatureSketchPoint {
        id: "payload-point".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        named_record: "named-record".into(),
        name: "Point1".into(),
        coordinates: cadmpeg_ir::units::FiniteVector::new([1.0, 2.0]).expect("finite coordinates"),
        scalar_fields: ["scalar-1".into(), "scalar-2".into()],
    };
    let groups = feature_sketch_point_groups(&[point]);
    let named_point = crate::native::features::OffsetStoreNamedPoint {
        id: "named-point".into(),
        name: "Point1".into(),
        data_blocks: vec!["block-10".into()],
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
    let block_use = crate::native::features::FeatureSketchNamedPointBlockUse {
        id: "nx:feature-history:sketch-named-point-block-use#1-4-0".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        sketch_reference: "reference".into(),
        reference_ordinal: 0,
        named_point: named_point.id.clone(),
        data_block: "block-10".into(),
        point_block_ordinal: 0,
        source_offset: 300,
    };
    let decode = || {
        feature_sketch_point_uses(
            &groups,
            std::slice::from_ref(&named_point),
            std::slice::from_ref(&block_use),
        )
    };
    assert_eq!(decode().len(), 1);
}
