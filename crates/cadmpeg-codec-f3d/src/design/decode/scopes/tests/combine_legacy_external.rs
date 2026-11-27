// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::combine::{
    exact_combine_external_body_identity, CombineExternalSelector,
};
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use crate::test_support::push_reference_u64;

const ASSET: &str = "11111111-1111-4111-8111-111111111111";
const CONTEXT: &str = "22222222-2222-4222-8222-222222222222";
const SCOPE: u32 = 90;
const RECORD: u32 = 92;

/// A legacy cross-document tool selector record and the offset of its
/// same-index paired header.
fn legacy_selector(separator: &[u8], type_guid: &str, epoch_repeat: u32) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"312", RECORD);
    bytes.extend_from_slice(&[0; 14]);
    push_reference_u64(&mut bytes, u64::from(RECORD + 3));
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, ASSET);
    lp_utf16(&mut bytes, CONTEXT);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_reference_u64(&mut bytes, 5_001);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&6_001u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0x0123_4567_89ab_cdefu64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&u32::try_from(type_guid.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&11u64.to_le_bytes());
    bytes.extend_from_slice(&48u32.to_le_bytes());
    bytes.extend_from_slice(&0.375f64.to_le_bytes());
    bytes.extend_from_slice(&epoch_repeat.to_le_bytes());
    push_reference_u64(&mut bytes, u64::from(RECORD + 2));
    bytes.extend_from_slice(separator);
    push_reference_u64(&mut bytes, u64::from(RECORD + 1));
    bytes.push(0);
    push_reference_u64(&mut bytes, u64::from(SCOPE));
    let paired_at = bytes.len();
    indexed_header(&mut bytes, *b"261", RECORD);
    (bytes, paired_at)
}

fn parse(bytes: &[u8], paired_at: usize) -> Option<CombineExternalSelector> {
    exact_combine_external_body_identity(bytes, 0, paired_at, SCOPE, RECORD).unwrap()
}

#[test]
fn legacy_selector_names_the_browser_node_and_source_occurrence() {
    for separator in [&[0u8, 0][..], &[1, 0, 0][..]] {
        let (bytes, paired_at) = legacy_selector(
            separator,
            crate::design::presentation::BROWSER_NODE_TYPE_GUID,
            48,
        );
        let Some(CombineExternalSelector::Legacy(identity)) = parse(&bytes, paired_at) else {
            panic!("legacy selector with separator {separator:?}");
        };
        assert_eq!(identity.selector_asset_id.as_str(), ASSET);
        assert_eq!(identity.selector_context_id.as_str(), CONTEXT);
        assert_eq!(identity.occurrence_reference, 5_001);
        assert_eq!(identity.browser_node_entity, 6_001);
        assert_eq!(identity.source_occurrence_identity, 0x0123_4567_89ab_cdef);
        assert_eq!(identity.tail_values, [11, 0.375f64.to_bits()]);
        let node_at = usize::try_from(identity.browser_node_entity_offset).unwrap();
        assert_eq!(bytes[node_at - 1], 1);
        assert_eq!(
            identity.source_occurrence_identity_offset,
            identity.browser_node_entity_offset + 9
        );
        assert_eq!(
            identity.tail_value_offsets[1],
            identity.tail_value_offsets[0] + 12
        );
    }
}

#[test]
fn legacy_selector_requires_browser_node_type_repeated_epoch_and_separator() {
    let (bytes, paired_at) = legacy_selector(&[0, 0], "E03784ED-5E19-4E14-B9F2-3B07017018CD", 48);
    assert!(parse(&bytes, paired_at).is_none());
    let (bytes, paired_at) = legacy_selector(
        &[0, 0],
        crate::design::presentation::BROWSER_NODE_TYPE_GUID,
        49,
    );
    assert!(parse(&bytes, paired_at).is_none());
    let (bytes, paired_at) = legacy_selector(
        &[0, 1, 0],
        crate::design::presentation::BROWSER_NODE_TYPE_GUID,
        48,
    );
    assert!(parse(&bytes, paired_at).is_none());
}
