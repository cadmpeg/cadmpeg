use crate::design::decode::scopes::component_constructions::unresolved_component_insert_placement;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::u64_from_index;

fn fixture(frame: usize, localized: bool) -> (Vec<u8>, DesignParameterScope) {
    let name = if localized {
        "Komponente einfügen"
    } else {
        "Component Insert"
    };
    let stored = frame + 2 * (name.encode_utf16().count() - 16);
    let delta = usize::from(matches!(frame, 276 | 404)) * 4;
    let explicit = frame >= 400;
    let mut bytes = vec![0; stored + 11];
    for at in [0, stored] {
        bytes[at..at + 4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(if at == 0 { b"789" } else { b"234" });
        bytes[at + 7..at + 11].copy_from_slice(&30_u32.to_le_bytes());
    }
    if delta != 0 {
        bytes[20] = 1;
    }
    bytes[25 + delta..33 + delta].copy_from_slice(&19_u64.to_le_bytes());
    bytes[37 + delta] = 1;
    bytes[38 + delta..42 + delta].copy_from_slice(&20_u32.to_le_bytes());
    bytes[48 + delta] = 1;
    bytes[49 + delta] = u8::from(!explicit);
    let matrix: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    if explicit {
        for (i, value) in matrix.into_iter().flatten().enumerate() {
            let at = 50 + delta + i * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let guid_at = if explicit { 178 + delta } else { 50 + delta };
    bytes[guid_at..guid_at + 76].copy_from_slice(
        &crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000").unwrap(),
    );
    let table = guid_at + 79;
    bytes[table..table + 4].copy_from_slice(&1_u32.to_le_bytes());
    bytes[table + 4] = 1;
    bytes[table + 5..table + 9].copy_from_slice(&20_u32.to_le_bytes());
    bytes[table + 19..table + 23].copy_from_slice(
        &u32::try_from(name.encode_utf16().count())
            .unwrap()
            .to_le_bytes(),
    );
    bytes[table + 23..table + 23 + name.encode_utf16().count() * 2].copy_from_slice(
        &name
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        DesignFeatureKind::ComponentInsert,
        30,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.frame_length = u64_from_index(stored);
            draft.class_tag = "789".to_owned().try_into().unwrap();
            draft.paired_class_tag = "234".to_owned().try_into().unwrap();
            draft.paired_byte_offset = u64_from_index(stored);
            draft.reference_count_offset = u64_from_index(table);
            draft.reference_members = ReferenceRun::from_columns(
                vec![20],
                vec![u64_from_index(table + 5)],
                "reference_members",
            )
            .unwrap();
            draft.kind_offset = u64_from_index(table + 23);
            draft.feature_ordinal_offset =
                u64_from_index(table + 23 + name.encode_utf16().count() * 2);
        })
        .unwrap();
    if localized {
        scope.source_kind = Some(name.to_owned());
    }
    (bytes, scope)
}

#[test]
fn scope_placements_decode_without_relation_or_carrier_records() {
    for frame in [272, 276, 400, 404] {
        for localized in [false, true] {
            let (bytes, mut scope) = fixture(frame, localized);
            let value = unresolved_component_insert_placement(&bytes, &scope).unwrap();
            assert_eq!(value.occurrence_identity, 19);
            if frame >= 400 {
                assert_eq!(
                    value.transform().rows(),
                    [
                        [0.0, -1.0, 0.0, 2.5],
                        [1.0, 0.0, 0.0, -4.0],
                        [0.0, 0.0, 1.0, 7.0],
                        [0.0, 0.0, 0.0, 1.0]
                    ]
                );
                assert_eq!(
                    value.matrix.as_ref().unwrap().offset,
                    if frame == 404 { 54 } else { 50 }
                );
            } else {
                assert_eq!(
                    value.transform().rows(),
                    [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0]
                    ]
                );
                assert!(value.matrix.is_none());
            }
            if let DesignScopePayloadMut::ComponentInsert(payload) = scope.payload_mut() {
                payload.unresolved_placement = Some(value);
            }
            let encoded = serde_json::to_vec(&scope).unwrap();
            let decoded: DesignParameterScope = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(decoded, scope);
            assert!(decoded.component_insert_construction().is_none());
        }
    }
}

#[test]
fn partial_placements_reject_wrong_markers_references_guids_and_truncation() {
    for frame in [272, 276, 400, 404] {
        let (bytes, scope) = fixture(frame, false);
        let delta = usize::from(matches!(frame, 276 | 404)) * 4;
        for at in [
            11,
            33 + delta,
            37 + delta,
            38 + delta,
            42 + delta,
            48 + delta,
            49 + delta,
            if frame >= 400 {
                178 + delta
            } else {
                50 + delta
            },
        ] {
            let mut changed = bytes.clone();
            changed[at] ^= 2;
            assert!(
                unresolved_component_insert_placement(&changed, &scope).is_none(),
                "frame={frame}, offset={at}"
            );
        }
        assert!(unresolved_component_insert_placement(&bytes[..frame - 1], &scope).is_none());
        if frame >= 400 {
            let mut changed = bytes;
            changed[50 + delta..58 + delta].copy_from_slice(&f64::NAN.to_le_bytes());
            assert!(unresolved_component_insert_placement(&changed, &scope).is_none());
        }
    }
}

#[test]
fn partial_insert_payload_cannot_coexist_with_a_complete_or_other_family_payload() {
    let (bytes, mut scope) = fixture(404, false);
    let value = unresolved_component_insert_placement(&bytes, &scope).unwrap();
    if let DesignScopePayloadMut::ComponentInsert(payload) = scope.payload_mut() {
        payload.unresolved_placement = Some(value);
    }
    let mut encoded = serde_json::to_value(&scope).unwrap();
    encoded["component_insert_construction"] = serde_json::json!({
        "relation_record_index":20,
        "carrier_record_index":10,
        "neutron_role":"11111111-2222-3333-4444-555555555555",
        "neutron_role_offset":0
    });
    assert!(serde_json::from_value::<DesignParameterScope>(encoded).is_err());
    let mut encoded = serde_json::to_value(scope).unwrap();
    encoded["kind"] = serde_json::json!("WorkPoint");
    assert!(serde_json::from_value::<DesignParameterScope>(encoded).is_err());
}
