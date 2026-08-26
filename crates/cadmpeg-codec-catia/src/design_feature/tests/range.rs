// SPDX-License-Identifier: Apache-2.0
//! Range source-property transfer tests.

use super::*;

use crate::entity_table::{RangeInterval, RangeIntervalPrefix, RangeIntervalSlot};
use crate::native::{
    CatiaEntitySchemaValue, CatiaRangeInterval, CatiaRangeNominal, CatiaRangeNominalFraming,
};

#[test]
fn transfers_exact_range_fields_as_unresolved_operation_properties() {
    let mut operation = native_operation_object(
        "operation-object",
        None,
        1,
        "operation-record",
        "Prism_ThickThin1",
        "operation-entry",
    );
    operation.fields.push("range-record".to_string());
    let mut range_entity = entity_record("range-entity", "range-record", 30, 2);
    range_entity.range_interval = Some(CatiaRangeInterval {
        range: CatiaEntitySchemaValue {
            offset: 1,
            ordinal: 2,
            entry: "range-entry".to_string(),
            value: "Range".to_string(),
        },
        interval: RangeInterval {
            prefix: RangeIntervalPrefix::Compact { value: 7, width: 1 },
            slots: Some([
                RangeIntervalSlot::Binary64 {
                    bits: (-0.125_f64).to_bits(),
                    offset: 4,
                },
                RangeIntervalSlot::Unset { offset: 13 },
            ]),
        },
        nominal: Some(CatiaRangeNominal {
            framing: CatiaRangeNominalFraming::D8Token81DB,
            bits: 2.5_f64.to_bits(),
            evaluation_opcode_offset: 17,
        }),
        incoming_references: Vec::new(),
        incoming_storage_references: Vec::new(),
    });
    let mut range_record = object_record(
        "range-record",
        Some("operation-object"),
        Some(2),
        Some(1),
        None,
        None,
    );
    range_record.entity_record = Some("range-entity".to_string());
    let native = CatiaNative {
        design_objects: vec![operation],
        object_graphs: vec![CatiaObjectGraph {
            id: "graph".to_string(),
            byte_offset: 0,
            byte_len: 0,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: vec![
                object_record(
                    "operation-record",
                    None,
                    Some(1),
                    None,
                    Some("Prism_ThickThin1"),
                    Some("operation-entry"),
                ),
                range_record,
            ],
        }],
        entity_records: vec![range_entity],
        ..CatiaNative::default()
    };
    let mut ir = CadIr::empty(Units::default());

    let transfer = transfer_design_features(&mut ir, &native, None);

    let properties = &ir.model.features[0].source_properties;
    assert_eq!(properties["catia_range_0_entity"], "range-entity");
    assert_eq!(properties["catia_range_0_selector_value"], "Range");
    assert_eq!(properties["catia_range_0_prefix_kind"], "compact");
    assert_eq!(properties["catia_range_0_prefix_value"], "7");
    assert_eq!(properties["catia_range_0_prefix_width"], "1");
    assert_eq!(properties["catia_range_0_slots"], "two");
    assert_eq!(
        properties["catia_range_0_lower_bits"],
        format!("{:016x}", (-0.125_f64).to_bits())
    );
    assert_eq!(properties["catia_range_0_upper_kind"], "unset");
    assert_eq!(properties["catia_range_0_nominal_framing"], "D8Token81DB");
    assert_eq!(
        properties["catia_range_0_nominal_bits"],
        format!("{:016x}", 2.5_f64.to_bits())
    );
    assert_eq!(transfer.native_operation_range_count, 1);
    assert_eq!(
        transfer.native_operation_range_records,
        HashSet::from(["range-record".to_string()])
    );
    assert!(transfer.consumed_records().contains("range-record"));
}
