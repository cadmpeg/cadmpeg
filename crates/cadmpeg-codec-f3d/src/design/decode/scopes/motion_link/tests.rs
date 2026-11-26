// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::identity::ReferenceRun;
use cadmpeg_core::serde_erase::json;

#[test]
fn motion_link_reads_and_retains_two_ordered_rotations() {
    const FIRST: u32 = 20;
    const SECOND: u32 = 23;
    const RELATION: u32 = 35;
    const LINK: u32 = RELATION + 1;
    let start = relation::LEN;
    let end = start + envelope::LEN;
    let mut bytes = vec![0; end];
    bytes[..7].copy_from_slice(b"\x03\x00\x00\x00408");
    bytes[7..11].copy_from_slice(&RELATION.to_le_bytes());
    for (offset, index) in [
        (relation::FIRST_PARENT, FIRST),
        (relation::SECOND_PARENT, SECOND),
        (relation::FIRST_JOINT, FIRST),
        (relation::SECOND_JOINT, SECOND),
        (start + envelope::RELATION, RELATION),
    ] {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&index.to_le_bytes());
    }
    for (offset, value) in [
        (relation::PARENT_COUNT, 2),
        (relation::FIRST_AXIS, ROTATE_Z),
        (relation::SECOND_AXIS, ROTATE_Z),
    ] {
        bytes[offset..][..size_of::<u32>()].copy_from_slice(&value.to_le_bytes());
    }
    bytes[relation::FIRST_ANGLE..][..size_of::<f64>()].copy_from_slice(&0.5_f64.to_le_bytes());
    bytes[relation::SECOND_ANGLE..][..size_of::<f64>()].copy_from_slice(&1.25_f64.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#166",
        DesignFeatureKind::MotionLink,
        LINK,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = start.try_into().unwrap();
            draft.frame_length = envelope::LEN.try_into().unwrap();
            draft.reference_count_offset = draft.byte_offset + 165;
            draft.reference_members = ReferenceRun::unlocated(vec![42, 44]);
            draft.paired_byte_offset = end.try_into().unwrap();
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut types = HashMap::from([
        (u64::from(FIRST), (JOINT_TYPE_GUID, 5)),
        (u64::from(SECOND), (JOINT_TYPE_GUID, 5)),
        (u64::from(RELATION), (RELATION_TYPE, 2)),
        (u64::from(LINK), (SCOPE_TYPE, 3)),
    ]);

    let records = IndexedRecordOffsets::build(&bytes);
    let parse = |bytes: &[u8], types: &_| read(View::over_retained(bytes), &records, &scope, types);
    let link = parse(&bytes, &types).unwrap();
    assert_eq!(link.joints, [FIRST, SECOND]);
    assert!((link.ratio().unwrap().get() - 2.5).abs() < f64::EPSILON);
    assert!(parse(&bytes[..end - 1], &types).is_none());
    for invalid_byte in [
        relation::PARENT_COUNT,
        relation::SECOND_JOINT + 1,
        relation::SECOND_AXIS,
        relation::PHASE,
        relation::FLAGS,
        start + envelope::RELATION,
    ] {
        bytes[invalid_byte] ^= 1;
        assert!(parse(&bytes, &types).is_none());
        bytes[invalid_byte] ^= 1;
    }
    types.remove(&u64::from(SECOND));
    assert!(parse(&bytes, &types).is_none());

    let DesignScopePayloadMut::MotionLink(slot) = scope.payload_mut() else {
        panic!("MotionLink scope");
    };
    *slot = Some(link);
    let saved = json::to_value(&scope).unwrap();
    assert_eq!(
        json::from_value::<DesignParameterScope>(saved.clone()).unwrap(),
        scope
    );
    let mut malformed = saved;
    malformed["linked_rotations"]["joints"] = cadmpeg_core::json!([FIRST, FIRST]);
    assert!(json::from_value::<DesignParameterScope>(malformed).is_err());
}
