// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_input_blocks;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_sketch_construction_inputs;
use crate::native::features::feature_sketch_construction_payloads;
use crate::native::features::feature_sketch_payload_scalars;
use crate::native::features::feature_sketch_records;
use crate::native::features::feature_sketch_references;

#[derive(Clone, Copy)]
enum SketchPayloadRoute {
    Construction,
    Scalar,
}

#[test]
fn sketch_reference_admits_fixture() {
    let container = {
        crate::container::scan_bytes(
            crate::test_support::test_prt::composed_feature_history_prt_over_sort_scratch(),
        )
    }
    .expect("composed feature-history container");
    let decode = || feature_sketch_references(&container);
    assert!(!decode().expect("admitted sketch references").is_empty());
}

fn assert_sketch_payload_admits(route: SketchPayloadRoute) {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let constructions = crate::test_support::with_decode_context(|_| {
        let labels = feature_operation_labels(&container)?;
        let records = feature_operation_records(&container)?;
        let inputs = feature_input_blocks(&container)?;
        let references = feature_sketch_references(&container)?;
        let sketches = feature_sketch_records(&labels, &records, &inputs, &references);
        feature_sketch_construction_inputs(&sketches, &references)
    })
    .expect("sketch payload construction inputs");
    let decode = |_: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        SketchPayloadRoute::Construction => {
            feature_sketch_construction_payloads(&container, &constructions).map(|rows| rows.len())
        }
        SketchPayloadRoute::Scalar => {
            feature_sketch_payload_scalars(&container, &constructions).map(|rows| rows.len())
        }
    };
    assert!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted sketch payload route")
            > 0
    );
}

#[test]
fn sketch_payload_admits_fixture() {
    assert_sketch_payload_admits(SketchPayloadRoute::Construction);
}

#[test]
fn sketch_payload_scalar_admits_fixture() {
    assert_sketch_payload_admits(SketchPayloadRoute::Scalar);
}
