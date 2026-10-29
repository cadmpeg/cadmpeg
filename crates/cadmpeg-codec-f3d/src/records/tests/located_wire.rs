use serde_json::json;

use crate::records::entity_header::{BaseTypeGuid, SegmentType};
use crate::records::recipes::ConstructionRecipe;
use crate::records::references::{DesignMaterialAssignment, LostEdgeReference};
use crate::records::sketch_relations::SketchRelation;

#[test]
fn lost_edge_reference_derives_offsets_and_rejects_inconsistent_wire() {
    let wire = json!({"id":"edge","record_byte_offset":152,"class_tag":"419","record_index":299,"next_class_tag":"326","next_record_index":300});
    let record: LostEdgeReference = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&record).unwrap(), wire);
    assert_eq!(record.class_tag_offset(), Some(156));
    assert_eq!(record.record_index_offset(), Some(159));
    assert_eq!(record.byte_offset(), Some(181));
    assert_eq!(record.next_byte_offset(), Some(200));
    for field in [
        "class_tag_offset",
        "record_index_offset",
        "byte_offset",
        "next_byte_offset",
    ] {
        let mut invalid = serde_json::to_value(&record).unwrap();
        invalid[field] = json!(0);
        assert!(serde_json::from_value::<LostEdgeReference>(invalid)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let mut invalid = serde_json::to_value(&record).unwrap();
    invalid["record_byte_offset"] = json!(u64::MAX - 47);
    assert!(serde_json::from_value::<LostEdgeReference>(invalid)
        .unwrap_err()
        .to_string()
        .contains("record_byte_offset"));
    let last = LostEdgeReference::new(
        "edge".into(),
        u64::MAX - 48,
        "419".into(),
        299,
        "326".into(),
        300,
    )
    .unwrap();
    assert_eq!(last.next_byte_offset(), Some(u64::MAX));
}

#[test]
fn material_assignment_preserves_located_and_authored_token_wire() {
    let base = json!({"id":"material#0","body_binding_id":"f3d:native:design-body-binding#0",
        "asm_body_key":42,"asm_body_key_offset":10,"entity_suffix_offset":20,"entity_id":"0_985",
        "entity_id_offset":30,"visual_guid":"11111111-2222-3333-4444-555555555555",
        "visual_guid_offset":40});
    for field in ["physical_token", "visual_preset"] {
        for value in ["", "Prism-002"] {
            for offset in [0, 50] {
                let mut wire = base.clone();
                wire[field] = value.into();
                wire[format!("{field}_offset")] = offset.into();
                let parsed: DesignMaterialAssignment =
                    serde_json::from_value(wire.clone()).expect("material token");
                assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
            }
        }
        let mut wire = base.clone();
        wire[format!("{field}_offset")] = 50.into();
        let error = serde_json::from_value::<DesignMaterialAssignment>(wire)
            .expect_err("orphan material offset")
            .to_string();
        assert!(error.contains(field));
        assert!(error.contains(&format!("{field}_offset")));
    }
    let parsed: DesignMaterialAssignment =
        serde_json::from_value(base.clone()).expect("absent tokens");
    assert_eq!(serde_json::to_value(parsed).unwrap(), base);
    let mut padded = base;
    padded["entity_id"] = "0_+00985".into();
    let parsed: DesignMaterialAssignment =
        serde_json::from_value(padded.clone()).expect("preserved numeric spelling");
    assert_eq!(serde_json::to_value(parsed).unwrap(), padded);
}

#[test]
fn recipe_design_id_preserves_source_and_authored_wire() {
    let base = json!({"id":"recipe#0","record_index_offset":11,"kind":"body","recipe_index":0,"record_index":12});
    for value in ["", "301"] {
        for offset in [0, 4] {
            let mut wire = base.clone();
            wire["design_id"] = value.into();
            wire["design_id_offset"] = offset.into();
            let parsed: ConstructionRecipe =
                serde_json::from_value(wire.clone()).expect("recipe id");
            assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
        }
    }
    let parsed: ConstructionRecipe =
        serde_json::from_value(base.clone()).expect("body-less recipe");
    assert_eq!(serde_json::to_value(parsed).unwrap(), base);
    let mut wire = base;
    wire["design_id_offset"] = 4.into();
    let error = serde_json::from_value::<ConstructionRecipe>(wire)
        .expect_err("orphan design id offset")
        .to_string();
    assert!(error.contains("design_id_offset"));
}

fn segment_type_wire() -> serde_json::Value {
    json!({"id":"f3d:native/MetaStream.dat:design-type#0","class_tag":"256","byte_offset":0,
        "type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4,"version":1,
        "version_offset":80,"module":"Fusion","entity_ids":[1]})
}

#[test]
fn segment_base_guid_preserves_source_and_authored_wire() {
    for value in ["", "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"] {
        for offset in [0, 44] {
            let mut wire = segment_type_wire();
            wire["base_type_guid"] = value.into();
            wire["base_type_guid_offset"] = offset.into();
            let parsed: SegmentType = serde_json::from_value(wire.clone()).expect("base GUID");
            let expected = if value.is_empty() {
                BaseTypeGuid::EmptyRoot {
                    offset: Some(offset),
                }
            } else {
                BaseTypeGuid::Guid {
                    value: value.to_owned().try_into().unwrap(),
                    offset: Some(offset),
                }
            };
            assert_eq!(parsed.base_type_guid, expected);
            assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
        }
    }
    let wire = segment_type_wire();
    let parsed: SegmentType = serde_json::from_value(wire.clone()).expect("root type");
    assert_eq!(parsed.base_type_guid, BaseTypeGuid::Absent);
    assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    let mut wire = segment_type_wire();
    wire["base_type_guid_offset"] = 44.into();
    let error = serde_json::from_value::<SegmentType>(wire)
        .expect_err("orphan base GUID offset")
        .to_string();
    assert!(error.contains("base_type_guid_offset"));
}

#[test]
fn segment_entity_runs_preserve_authored_and_located_wire() {
    let wire = |ids: serde_json::Value, offsets: serde_json::Value| {
        let mut wire = segment_type_wire();
        wire["version_offset"] = 44.into();
        wire["entity_ids"] = ids;
        if offsets
            .as_array()
            .is_some_and(|offsets| !offsets.is_empty())
        {
            wire["entity_id_offsets"] = offsets;
        }
        wire
    };
    for (ids, offsets) in [
        (json!([]), json!([])),
        (json!([10, 11]), json!([])),
        (json!([10, 11]), json!([0, 0])),
        (json!([10, 11]), json!([80, 88])),
    ] {
        let wire = wire(ids, offsets);
        let entry: SegmentType = serde_json::from_value(wire.clone()).expect("type entity run");
        assert_eq!(serde_json::to_value(entry).unwrap(), wire);
    }
    for (ids, offsets) in [
        (json!([]), json!([80])),
        (json!([10, 11]), json!([80])),
        (json!([10]), json!([80, 88])),
    ] {
        let error = serde_json::from_value::<SegmentType>(wire(ids, offsets))
            .expect_err("partial entity locations")
            .to_string();
        assert!(error.contains("entity_ids/entity_id_offsets"), "{error}");
    }
}

#[test]
fn sketch_auxiliary_rows_preserve_absent_and_complete_offset_runs() {
    let base = json!({"id":"relation","record_index":1,"class_tag":"000","byte_offset":0,
        "state_offset":0,"owner_reference":1,"owner_reference_offset":0,"owner_entity_id":"owner",
        "auxiliary_references":[],"members":[],"resolved_members":[],"state":0,
        "member_relation_ordinals":[],"entity_genesis":null,"pattern":null,"return_members":[],
        "resolved_return_members":[]});
    for (values, offsets) in [
        (vec![], vec![]),
        (vec![2], vec![0]),
        (vec![2, 3], vec![0, 10]),
    ] {
        let mut expected = base.clone();
        expected["auxiliary_references"] = json!(values);
        if !offsets.is_empty() {
            expected["auxiliary_reference_offsets"] = json!(offsets);
        }
        let relation: SketchRelation = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(
            relation
                .auxiliary_references()
                .values()
                .copied()
                .collect::<Vec<_>>(),
            values
        );
        assert_eq!(
            relation
                .auxiliary_references()
                .offsets()
                .copied()
                .collect::<Vec<_>>(),
            offsets
        );
        assert_eq!(serde_json::to_value(&relation).unwrap(), expected);
    }
    for (values, offsets) in [
        (vec![], vec![10]),
        (vec![2], vec![10, 20]),
        (vec![2, 3], vec![10]),
    ] {
        let mut wire = base.clone();
        wire["auxiliary_references"] = json!(values);
        wire["auxiliary_reference_offsets"] = json!(offsets);
        assert!(serde_json::from_value::<SketchRelation>(wire)
            .unwrap_err()
            .to_string()
            .contains("auxiliary_reference"));
    }
}
