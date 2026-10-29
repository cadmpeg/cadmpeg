// SPDX-License-Identifier: Apache-2.0

use super::super::ParasolidDeltasRecord;

fn wire(references: &[u32]) -> serde_json::Value {
    serde_json::json!({
        "id":"nx:test:region#0", "stream_ordinal":0, "family":"REGION", "kind":19,
        "xmt":52, "node_id":49, "region_type":83,
        "byte_len":9+references.len()*3, "references":references,
        "inflated_offset":61
    })
}

#[test]
fn region_wire_preserves_complete_layouts_and_borrowed_serialization() {
    for references in [
        vec![1, 3, 1, 12, 8],
        vec![1, 3, 1, 12, 8, 90],
        vec![1, 3, 1, 12, 8, 89, 90],
    ] {
        for region_type in [83, 86] {
            let mut expected = wire(&references);
            expected["region_type"] = region_type.into();
            let record: ParasolidDeltasRecord = serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(serde_json::to_value(&record).unwrap(), expected);
            let restored: ParasolidDeltasRecord =
                serde_json::from_slice(&serde_json::to_vec(&record).unwrap()).unwrap();
            assert_eq!(restored, record);
            cadmpeg_test_support::native_serialization::assert_native_limit(&record, expected);
        }
    }
}

#[test]
fn region_wire_requires_the_type_and_exact_nonzero_pointer_cardinality() {
    let good = wire(&[1, 3, 1, 12, 8]);
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("region_type");
    assert!(serde_json::from_value::<ParasolidDeltasRecord>(missing).is_err());
    for invalid in [serde_json::Value::Null, 0.into(), 84.into(), "S".into()] {
        let mut bad = good.clone();
        bad["region_type"] = invalid;
        assert!(serde_json::from_value::<ParasolidDeltasRecord>(bad).is_err());
    }
    for count in [0, 1, 2, 3, 4, 8, 9] {
        assert!(serde_json::from_value::<ParasolidDeltasRecord>(wire(&vec![1; count])).is_err());
    }
    for count in [5, 6, 7] {
        let nulls = wire(&vec![1; count]);
        assert!(serde_json::from_value::<ParasolidDeltasRecord>(nulls.clone()).is_ok());
        for slot in 0..count {
            let mut bad = nulls.clone();
            bad["references"][slot] = 0.into();
            assert!(serde_json::from_value::<ParasolidDeltasRecord>(bad).is_err());
        }
    }
    for (key, value) in [
        ("node_id", serde_json::Value::Null),
        ("kind", 18.into()),
        ("family", "SHELL".into()),
        ("position", serde_json::json!([0.0, 0.0, 0.0])),
        ("group_selector", 1.into()),
        ("group_linked_reference_status", 1.into()),
    ] {
        let mut bad = good.clone();
        bad[key] = value;
        assert!(serde_json::from_value::<ParasolidDeltasRecord>(bad).is_err());
    }
}
