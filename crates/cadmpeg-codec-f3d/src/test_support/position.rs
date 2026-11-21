use crate::design::decode::scopes::position::TYPE_GUID;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::u64_from_index;
use std::collections::HashMap;

pub(crate) const OCCURRENCE_KEY: &str = "11111111-2222-4333-8444-555555555555";

pub(crate) fn fixture(
    matrices: &[Option<[[f64; 4]; 4]>],
) -> (
    Vec<u8>,
    DesignParameterScope,
    HashMap<u64, (&'static str, u32)>,
) {
    let mut bytes = Vec::new();
    super::indexed_header(&mut bytes, *b"257", 500);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&u32::try_from(matrices.len()).unwrap().to_le_bytes());
    for (ordinal, matrix) in matrices.iter().enumerate() {
        bytes.push(0);
        super::push_marked_reference(&mut bytes, 700 + u32::try_from(ordinal).unwrap() * 3);
        bytes.push(u8::from(matrix.is_none()));
        if let Some(matrix) = matrix {
            for value in matrix.iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    super::lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    let reference_count_offset = bytes.len();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    super::push_marked_reference(&mut bytes, 600);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    super::lp_utf16(&mut bytes, "Posición");
    let tail = bytes.len();
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.resize(tail + 72, 0);
    bytes[tail + 30..tail + 34].copy_from_slice(&u32::MAX.to_le_bytes());
    super::indexed_header(&mut bytes, *b"258", 500);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::native("Position"),
        500,
    );
    scope
        .try_edit(|draft| {
            draft.class_tag = "257".to_owned().try_into().unwrap();
            draft.paired_class_tag = "258".to_owned().try_into().unwrap();
            draft.reference_count_offset = u64_from_index(reference_count_offset);
            draft.reference_members = ReferenceRun::unlocated(vec![600]);
            draft.layout_fixture_references();
            draft.frame_length = u64_from_index(tail + 72);
            draft.paired_byte_offset = draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.source_kind = Some("Posición".into());
    (bytes, scope, HashMap::from([(500, (TYPE_GUID, 6))]))
}

pub(crate) fn append_locator(
    bytes: &mut Vec<u8>,
    types: &mut HashMap<u64, (&'static str, u32)>,
    backlink: u32,
) {
    append_locator_path(bytes, types, backlink, &[OCCURRENCE_KEY], &[]);
}

pub(crate) fn append_locator_path(
    bytes: &mut Vec<u8>,
    types: &mut HashMap<u64, (&'static str, u32)>,
    backlink: u32,
    keys: &[&str],
    lists: &[&[&str]],
) {
    let mut locator = Vec::new();
    super::indexed_header(&mut locator, *b"259", 700);
    locator.extend_from_slice(&[0; 28]);
    locator.extend_from_slice(&3_u32.to_le_bytes());
    locator.extend_from_slice(&[0; 13]);
    super::push_marked_reference(&mut locator, 900);
    locator.extend_from_slice(&[1, 0]);
    super::push_marked_reference(&mut locator, backlink);
    super::push_marked_reference(&mut locator, 702);
    locator.extend_from_slice(&2_u32.to_le_bytes());
    locator.extend_from_slice(&[0; 2]);
    assert_eq!(locator.len(), 97);
    bytes.extend_from_slice(&locator);
    super::lp_ascii(bytes, "260");
    bytes.extend_from_slice(&701_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&u32::try_from(keys.len()).unwrap().to_le_bytes());
    for key in keys {
        super::lp_utf16(bytes, key);
    }
    for _ in 0..2 {
        super::lp_utf16(bytes, OCCURRENCE_KEY);
    }
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    for _ in 0..2 {
        super::lp_utf16(bytes, OCCURRENCE_KEY);
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(lists.len()).unwrap().to_le_bytes());
    for list in lists {
        bytes.extend_from_slice(&u32::try_from(list.len()).unwrap().to_le_bytes());
        for key in *list {
            super::lp_utf16(bytes, key);
        }
    }
    super::indexed_header(bytes, *b"261", 702);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    super::push_marked_reference(bytes, 701);
    super::indexed_header(bytes, *b"262", 703);
    types.insert(700, ("2E3AC990-483D-4A55-8662-C064B976A957", 5));
    types.insert(
        701,
        (
            crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID,
            1,
        ),
    );
}
