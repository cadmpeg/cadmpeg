// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container;
use crate::native::om::{operation_state_counters, OmOperationStateCounter};
use crate::test_support::{prt_with_named_payloads, segment_om_record_area_with_state_counter_map};
use crate::NxCodec;

#[test]
fn native_catalog_emits_feature_history_state_counter_rows() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_state_counter_map(),
    )]);
    let container = container::scan_bytes(file).expect("required invariant");

    let rows = operation_state_counters(&container);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].row_kind, 1);
    assert_eq!(rows[0].object_index, 0x320);
    assert_eq!(rows[0].raw_object_index, [0x83, 0x20]);
    assert_eq!(rows[0].introduced_state, 1);
    assert_eq!(rows[0].modified_state, 2);
    assert!(rows[0].object_index_source_offset > rows[0].source_offset);
    assert_eq!(rows[1].row_kind, 2);
    assert_eq!(rows[1].object_index, 0x1234);
    assert_eq!(rows[1].ordinal, 1);
    assert_eq!(rows[0].section_link, rows[1].section_link);

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_state_counter_map(),
            )])),
            &DecodeOptions::default(),
        )
        .expect("native decode");
    let emitted = result
        .ir()
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<OmOperationStateCounter>("om_operation_state_counters")
        .expect("state-counter arena");
    assert_eq!(emitted, rows.as_slice());
}
