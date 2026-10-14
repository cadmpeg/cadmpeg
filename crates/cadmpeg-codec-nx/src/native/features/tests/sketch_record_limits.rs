// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_input_blocks;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_sketch_construction_inputs;
use crate::native::features::feature_sketch_records;
use crate::native::features::feature_sketch_references;

#[derive(Clone, Copy)]
enum SketchRoute {
    Record,
    ConstructionInput,
}

fn assert_sketch_record_admits(route: SketchRoute) {
    let container = {
        crate::container::scan_bytes(crate::test_support::test_prt::composed_feature_history_prt())
    }
    .expect("composed feature-history container");
    let (labels, records, inputs, references) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_operation_labels(&container)?,
            feature_operation_records(ctx, &container)?,
            feature_input_blocks(&container)?,
            feature_sketch_references(&container)?,
        ))
    })
    .expect("sketch record inputs");
    let sketches = feature_sketch_records(&labels, &records, &inputs, &references);
    assert!(!sketches.is_empty());
    let decode = || match route {
        SketchRoute::Record => {
            Ok(feature_sketch_records(&labels, &records, &inputs, &references).len())
        }
        SketchRoute::ConstructionInput => {
            feature_sketch_construction_inputs(&sketches, &references).map(|values| values.len())
        }
    };
    assert!(decode().expect("admitted sketch record route") > 0);
}

macro_rules! sketch_record_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_sketch_record_admits($route);
        }
    };
}

sketch_record_admit_test!(sketch_record_admits_fixture, SketchRoute::Record);
sketch_record_admit_test!(
    sketch_construction_input_admits_fixture,
    SketchRoute::ConstructionInput
);
