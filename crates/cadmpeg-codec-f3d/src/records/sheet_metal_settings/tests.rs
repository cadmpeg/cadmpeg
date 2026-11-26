// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;

#[test]
fn settings_wire_couples_version_arity_and_source_identity() {
    use super::DesignSheetMetalSettings;
    for (version, count, tail) in [
        ("v2", 7, None),
        ("v3", 9, Some("0000")),
        ("v3", 9, Some("010000")),
    ] {
        let mut wire = cadmpeg_core::json!({
            "id": "f3d:Design1/BulkStream.dat:design-sheet-metal-settings#20",
            "record_index": 50, "byte_offset": 20, "paired_byte_offset": 100,
            "mask": 240, "inputs": {"version": version, "slots": vec![None::<u64>; count]},
            "rule_record_index": 51, "presentation_record_index": 52,
            "owner_record_index": 53, "context_record_index": 54,
        });
        if let Some(tail) = tail {
            wire["inputs"]["rule_reference_tail"] = cadmpeg_core::json!(tail);
        }
        let record: DesignSheetMetalSettings = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(record).unwrap(), wire);
        let mut invalid = wire.clone();
        invalid["inputs"]["rule_reference_tail"] = cadmpeg_core::json!("000000");
        assert!(json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
        if tail.is_some() {
            let mut invalid = wire.clone();
            invalid["inputs"]
                .as_object_mut()
                .unwrap()
                .remove("rule_reference_tail");
            assert!(json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
        }
        for (field, value) in [
            ("byte_offset", cadmpeg_core::json!(21)),
            ("paired_byte_offset", cadmpeg_core::json!(20)),
            ("unexpected", cadmpeg_core::json!(0)),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
        }
        let mut invalid = wire;
        invalid["inputs"]["slots"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::Null);
        assert!(json::from_value::<DesignSheetMetalSettings>(invalid).is_err());
    }
}

#[test]
fn leading_reference_form_has_no_mask() {
    use super::DesignSheetMetalSettings;
    let wire = cadmpeg_core::json!({
        "id": "f3d:Design1/BulkStream.dat:design-sheet-metal-settings#20",
        "record_index": 50, "byte_offset": 20, "paired_byte_offset": 100,
        "inputs": {"version": "v3_leading_reference", "first": 55, "rule_reference_tail": "0000"},
        "rule_record_index": 51, "presentation_record_index": 52,
        "owner_record_index": 53, "context_record_index": 54,
    });
    let record: DesignSheetMetalSettings = json::from_value(wire.clone()).unwrap();
    assert_eq!(json::to_value(&record).unwrap(), wire);
    assert_eq!(record.mask, None);
    assert_eq!(
        record.record_references().collect::<Vec<_>>(),
        [50, 55, 51, 52, 53, 54]
    );
    let mut with_mask = wire.clone();
    with_mask["mask"] = cadmpeg_core::json!(0);
    assert!(json::from_value::<DesignSheetMetalSettings>(with_mask).is_err());
    let mut slots = wire;
    slots["inputs"] = cadmpeg_core::json!({
        "version": "v3", "slots": vec![None::<u64>; 9], "rule_reference_tail": "0000",
    });
    assert!(json::from_value::<DesignSheetMetalSettings>(slots).is_err());
}
