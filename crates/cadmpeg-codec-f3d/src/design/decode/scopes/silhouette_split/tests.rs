// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use crate::records::identity::ReferenceRun;
use crate::test_support::indexed_header;

const SCOPE: u32 = 1;
const CARRIER: u32 = 2;
const SELECTOR: u32 = 3;
const GROUP: u32 = 4;
const BODY: u32 = 5;
const EPS_CARRIER_COORDINATE: f64 = 1.0e-12;

fn push_local(bytes: &mut Vec<u8>, index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(index).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

fn fixture(
    mode_word: u32,
    expanded: bool,
    prefix_group: u32,
    carrier_input: u32,
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"256", SCOPE);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(u8::from(expanded));
    if expanded {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
    push_local(&mut bytes, CARRIER);
    bytes.extend_from_slice(&mode_word.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::from(expanded).to_le_bytes());
    if expanded {
        push_local(&mut bytes, 6);
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_local(&mut bytes, prefix_group);
    let guid = "00000000-0000-0000-0000-000000000000";
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    for unit in guid.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 3]);

    let table_at = u64::try_from(bytes.len()).unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:test/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::SilhouetteSplit,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            draft.reference_count_offset = table_at;
            draft.reference_members = ReferenceRun::unlocated(vec![CARRIER, SELECTOR, GROUP, BODY]);
            draft.layout_fixture_references();
            let kind_bytes =
                cadmpeg_core::decode::u64_from_index("SilhouetteSplit".encode_utf16().count()) * 2;
            draft.paired_byte_offset = draft.kind_offset + kind_bytes + 72;
            draft.frame_length = draft.paired_byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    bytes.resize(usize::try_from(scope.paired_byte_offset()).unwrap(), 0);
    indexed_header(&mut bytes, *b"257", SCOPE);
    indexed_header(&mut bytes, *b"328", CARRIER);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    for value in [5.0_f64, -2.0, 0.0, 2.5, 0.0, 0.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    bytes.push(0);
    for value in [-1.0_f64; 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_local(&mut bytes, carrier_input);
    indexed_header(&mut bytes, *b"257", CARRIER);
    (bytes, scope)
}

fn types() -> HashMap<u64, (&'static str, u32)> {
    HashMap::from([
        (u64::from(SCOPE), (TYPE_GUID, 0)),
        (u64::from(CARRIER), (POINT_DIRECTION_TYPE_GUID, 4)),
    ])
}

fn read(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    types: &HashMap<u64, (&str, u32)>,
    losses: &mut Vec<LossNote>,
) -> Option<DesignSilhouetteSplit> {
    crate::test_support::with_decode_context(|ctx| {
        decode(ctx, bytes, records, scope, types, losses)
    })
    .unwrap()
}

#[test]
fn silhouette_split_keeps_the_mode_and_solved_carrier_in_both_prefixes() {
    for (mode_word, expanded) in [(0, false), (1, true), (2, true)] {
        let (bytes, mut scope) = fixture(mode_word, expanded, GROUP, SELECTOR);
        let records = IndexedRecordOffsets::build(&bytes);
        let mut losses = Vec::new();
        let operation = read(&bytes, &records, &scope, &types(), &mut losses).unwrap();
        assert!(losses.is_empty());
        assert_eq!(operation.mode_word, mode_word);
        let carrier = operation.carrier.as_ref().unwrap();
        for (value, expected) in carrier.position.iter().zip([5.0, -2.0, 0.0]) {
            assert!((value.get() - expected).abs() < EPS_CARRIER_COORDINATE);
        }
        for (value, expected) in carrier.direction.iter().zip([2.5, 0.0, 0.0]) {
            assert!((value.get() - expected).abs() < EPS_CARRIER_COORDINATE);
        }
        assert_eq!(carrier.reference_type, 7);
        let DesignScopePayloadMut::SilhouetteSplit(slot) = scope.payload_mut() else {
            panic!("Silhouette Split scope");
        };
        *slot = Some(operation);
        let wire = cadmpeg_core::serde_erase::json::to_value(&scope).unwrap();
        assert_eq!(wire["silhouette_split_operation"]["mode_word"], mode_word);
        assert_eq!(
            cadmpeg_core::serde_erase::json::from_value::<DesignParameterScope>(wire).unwrap(),
            scope
        );
    }
}

#[test]
fn silhouette_split_requires_scope_registration_and_matching_references() {
    let (bytes, scope) = fixture(2, true, GROUP, BODY);
    let records = IndexedRecordOffsets::build(&bytes);
    let mut losses = Vec::new();
    assert!(read(&bytes, &records, &scope, &HashMap::new(), &mut losses).is_none());
    let operation = read(&bytes, &records, &scope, &types(), &mut losses).unwrap();
    assert_eq!(operation.mode_word, 2);
    assert!(operation.carrier.is_none());
    assert!(losses.is_empty());

    let (bytes, scope) = fixture(1, true, BODY, SELECTOR);
    assert!(read(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &types(),
        &mut losses,
    )
    .is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureSilhouetteSplitPrefixUnparsed.kind()
    );
}

#[test]
fn silhouette_split_keeps_the_mode_when_the_carrier_input_is_cross_segment() {
    let (mut bytes, scope) = fixture(1, true, GROUP, SELECTOR);
    let records = IndexedRecordOffsets::build(&bytes);
    let (_, paired_at) = records.frames(CARRIER).next().unwrap();
    let segment_flag = paired_at - 1;
    bytes[segment_flag] = 1;
    let segment_at = segment_flag + 1;
    bytes.splice(segment_at..segment_at, 42_u32.to_le_bytes());
    let records = IndexedRecordOffsets::build(&bytes);
    let mut losses = Vec::new();
    let operation = read(&bytes, &records, &scope, &types(), &mut losses).unwrap();
    assert_eq!(operation.mode_word, 1);
    assert!(operation.carrier.is_none());
}
