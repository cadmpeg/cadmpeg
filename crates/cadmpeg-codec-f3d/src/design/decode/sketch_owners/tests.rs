// SPDX-License-Identifier: Apache-2.0

use super::{assign, parse_collection};
use crate::loss::F3dLossCode;
use cadmpeg_core::decode::View;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::sketches::SketchId;

fn lp(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend(u32::try_from(value.len()).unwrap().to_le_bytes());
    bytes.extend(value.as_bytes());
}

fn reference(bytes: &mut Vec<u8>, target: u64, typed: bool) {
    bytes.push(1);
    bytes.extend(target.to_le_bytes());
    if typed {
        lp(bytes, "11111111-2222-4333-8444-555555555555");
    }
    bytes.extend([0, 0]);
}

#[test]
fn collection_links_support_both_versions_and_local_reference_forms() {
    for version in [3, 4] {
        for typed in [false, true] {
            let mut bytes = Vec::new();
            bytes.extend(3u32.to_le_bytes());
            bytes.extend(b"256");
            bytes.extend(2u64.to_le_bytes());
            bytes.extend([0; 5]);
            bytes.push(1);
            bytes.extend(1u32.to_le_bytes());
            lp(&mut bytes, "crv_tag_id_idx");
            lp(&mut bytes, "IntrinsicMetaTypeuint64");
            bytes.extend(0u64.to_le_bytes());
            if version == 3 {
                bytes.extend([0; 6]);
            }
            bytes.extend(2u32.to_le_bytes());
            reference(&mut bytes, 1, false);
            reference(&mut bytes, 2, false);
            reference(&mut bytes, 7, typed);
            let collection = parse_collection(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                View::over_retained(&bytes),
                version,
            )
            .unwrap();
            assert_eq!(collection.members, [1, 2]);
            assert_eq!(collection.owner, Some(7));
            bytes.extend([0xff; 3]);
            let collection = parse_collection(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                View::over_retained(&bytes),
                version,
            )
            .unwrap();
            assert_eq!(collection.members, [1, 2]);
            assert_eq!(collection.owner, None);
        }
    }
}

fn typed_member_collection(version: u32, member_types: [Option<&str>; 2]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(3u32.to_le_bytes());
    bytes.extend(b"256");
    bytes.extend(2u64.to_le_bytes());
    bytes.extend([0; 5]);
    bytes.push(1);
    bytes.extend(1u32.to_le_bytes());
    lp(&mut bytes, "crv_tag_id_idx");
    lp(&mut bytes, "IntrinsicMetaTypeuint64");
    bytes.extend(0u64.to_le_bytes());
    if version == 3 {
        bytes.extend([2, 0, 0, 0, 1, 0]);
    }
    bytes.extend(2u32.to_le_bytes());
    for (target, member_type) in [5u64, 6].into_iter().zip(member_types) {
        bytes.push(1);
        bytes.extend(target.to_le_bytes());
        if let Some(member_type) = member_type {
            lp(&mut bytes, member_type);
        }
        bytes.extend([0, 0]);
    }
    reference(&mut bytes, 7, true);
    bytes
}

#[test]
fn version_three_run_references_carry_the_sketch_container_type() {
    let container = super::SKETCH_CONTAINER_TYPE_GUID;
    let parse = |version, member_types| {
        parse_collection(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            View::over_retained(&typed_member_collection(version, member_types)),
            version,
        )
    };
    let collection = parse(3, [Some(container), Some(container)]).expect("typed version-3 run");
    assert_eq!(collection.members, [5, 6]);
    assert_eq!(collection.owner, Some(7));
    assert!(parse(4, [Some(container), Some(container)]).is_err());
    assert!(parse(3, [Some(container), None]).is_err());
    assert!(parse(3, [None, Some(container)]).is_err());
    let other = "11111111-2222-4333-8444-555555555555";
    assert!(parse(3, [Some(other), Some(other)]).is_err());
}

#[test]
fn collection_ownership_assigns_a_component_and_reports_an_unresolved_link() {
    let product = ProductDefinitionId::mint("test:model:product#part").unwrap();
    let owned = SketchId::mint("test:model:sketch#owned").unwrap();
    let unresolved = SketchId::mint("test:model:sketch#unresolved").unwrap();
    let root = SketchId::mint("test:model:sketch#root").unwrap();
    let mut definitions = [json::from_value(cadmpeg_core::json!({
        "id": product, "kind": "part", "bom_properties": {}, "storage": {"kind": "embedded"}, "occurrences": [], "instance_bodies": [], "representations": [], "bodies": []
    }))
    .unwrap()];
    let owners = [(owned.clone(), Some(product)), (unresolved.clone(), None)]
        .into_iter()
        .collect();
    let sketches = [owned.clone(), unresolved, root].into_iter().collect();
    let mut losses = Vec::new();
    assign(
        &owners,
        &sketches,
        &mut definitions,
        &std::collections::BTreeSet::default(),
        &mut losses,
    );
    assert_eq!(definitions[0].sketches, [owned]);
    assert_eq!(losses.len(), 1);
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SketchOwnerUnresolved.kind()));
}

fn visibility(collection: u64) -> crate::records::sketch_placement::DesignSketchVisibility {
    let mut visibility = crate::records::sketch_placement::DesignSketchVisibility::new(
        std::num::NonZeroU32::new(1).unwrap(),
        0,
        true,
    )
    .unwrap();
    visibility.ordinal_collection = std::num::NonZeroU64::new(collection);
    visibility
}

#[test]
fn unlisted_and_folder_sketches_use_collection_owner() {
    let product = ProductDefinitionId::mint("test:model:product#part").unwrap();
    let collection_owners = [(300, Some(product.clone()))].into_iter().collect();
    let entities = std::collections::BTreeMap::new();
    for sketch in [201, 202] {
        let owner =
            super::placement_owner(Some(visibility(300)), sketch, &collection_owners, &entities);
        assert_eq!(owner, Some(&Some(product.clone())));
    }
}

#[test]
fn direct_collection_takes_precedence_over_conflicting_member_run() {
    let first = ProductDefinitionId::mint("test:model:product#first").unwrap();
    let second = ProductDefinitionId::mint("test:model:product#second").unwrap();
    let collections = [(300, Some(first.clone())), (400, Some(second.clone()))]
        .into_iter()
        .collect();
    let members = [(201, Some(first.clone()))].into_iter().collect();
    assert_eq!(
        super::placement_owner(Some(visibility(400)), 201, &collections, &members),
        Some(&Some(second))
    );
    assert_eq!(
        super::placement_owner(Some(visibility(0)), 201, &collections, &members),
        None
    );
    assert_eq!(
        super::placement_owner(None, 201, &collections, &members),
        Some(&Some(first))
    );
}
