// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::metastream::RecordIndexEntry;
use crate::records::entity_header::BaseTypeGuid;
use crate::records::identity::ReferenceRun;

fn row(guid: &str, base: &str, version: u32, entity: u64) -> SegmentTypeData {
    SegmentTypeData {
        byte_offset: 0,
        type_guid: guid.to_owned().try_into().unwrap(),
        type_guid_offset: 0,
        base_type_guid: BaseTypeGuid::Guid {
            value: base.to_owned().try_into().unwrap(),
            offset: 0,
        },
        version,
        version_offset: 0,
        module: "EntityTracking".into(),
        entities: ReferenceRun::unlocated(vec![entity]),
    }
}

fn fixture() -> (MetaStream, DesignConstructionTrackingPath) {
    let meta = MetaStream {
        types: vec![
            row(
                "335CC7BD-BBE5-40AA-9E8C-5EC99D427A3A",
                "5C069ACB-169F-4555-9792-815B4671DDA0",
                0,
                300,
            ),
            row(
                "2DEE34BE-FA70-4D54-979C-164D86FDA786",
                "70F5E801-1E8E-436E-8A09-36487F8A2859",
                2,
                301,
            ),
        ],
        records: vec![
            RecordIndexEntry {
                entity_id: 300,
                bulk_offset: 0,
            },
            RecordIndexEntry {
                entity_id: 301,
                bulk_offset: 33,
            },
        ],
        secondary_records: Vec::new(),
    };
    let path = serde_json::from_value(serde_json::json!({
        "wrapper_record_index":300,"wrapper_byte_offset":0,"wrapper_class_tag":"256",
        "carrier_record_index":301,"carrier_byte_offset":33,"carrier_class_tag":"257",
        "primary_identity":268,"primary_identity_offset":70,"selector":1,"selector_offset":90,
        "kind":1,"kind_offset":94,"following_record_index":302,"following_byte_offset":114,
        "following_class_tag":"258"
    }))
    .unwrap();
    (meta, path)
}

#[test]
fn registration_follows_dynamic_classes_and_case_insensitive_guids() {
    let (mut meta, mut path) = fixture();
    assert!(Registry::new(Rc::new(meta.clone())).supports(&path));
    meta.types.swap(0, 1);
    path.wrapper_class_tag = "257".to_owned().try_into().unwrap();
    path.carrier_class_tag = "256".to_owned().try_into().unwrap();
    for row in &mut meta.types {
        row.type_guid = row
            .type_guid
            .as_str()
            .to_ascii_lowercase()
            .try_into()
            .unwrap();
        let base = row
            .base_type_guid
            .value()
            .unwrap()
            .as_str()
            .to_ascii_lowercase();
        row.base_type_guid = BaseTypeGuid::Guid {
            value: base.try_into().unwrap(),
            offset: 0,
        };
    }
    assert!(Registry::new(Rc::new(meta)).supports(&path));
}

#[test]
fn registration_rejects_wrong_family_version_membership_and_physical_offsets() {
    let (original, path) = fixture();
    for index in 0..2 {
        for mutation in 0..9 {
            let mut meta = original.clone();
            match mutation {
                0 => meta.types[index].version += 1,
                1 => meta.types[index].module = "Fusion".into(),
                2 => meta.types[index].base_type_guid = BaseTypeGuid::Absent,
                3 => {
                    meta.types[index].type_guid = "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .unwrap()
                }
                4 => meta.types[index].entities = ReferenceRun::unlocated(vec![999]),
                5 => meta.records[index].entity_id = 999,
                6 => meta.records[index].bulk_offset += 1,
                7 => {
                    meta.types[index].base_type_guid = BaseTypeGuid::Guid {
                        value: "11111111-2222-3333-4444-555555555555"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        offset: 0,
                    }
                }
                8 => meta.types[index].version = if index == 0 { 2 } else { 1 },
                _ => unreachable!(),
            }
            assert!(!Registry::new(Rc::new(meta)).supports(&path));
        }
    }
    let mut wrong_class = path;
    wrong_class.wrapper_class_tag = "257".to_owned().try_into().unwrap();
    assert!(!Registry::new(Rc::new(original)).supports(&wrong_class));
}
