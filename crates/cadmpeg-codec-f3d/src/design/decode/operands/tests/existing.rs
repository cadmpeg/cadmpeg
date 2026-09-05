// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

use super::{
    body_recipe_operand_end, body_recipe_prologue_end, parse_sketch_profile_region_selection,
    unique_body_recipe,
};
use crate::records::{ConstructionRecipe, ConstructionRecipeKind, DesignRecordHeader};

fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

fn region_member(bytes: &mut Vec<u8>, curve_primary_id: u32, incidence: [u32; 3]) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&curve_primary_id.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    for word in incidence {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
}

fn region_selection_frame() -> (Vec<u8>, usize, usize, usize) {
    let profile_record_index = 100;
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"266", profile_record_index);
    indexed_header(&mut bytes, *b"263", profile_record_index + 1);
    indexed_header(&mut bytes, *b"259", profile_record_index + 2);
    let selection_at = bytes.len();
    indexed_header(&mut bytes, *b"327", profile_record_index + 3);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&profile_record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    region_member(&mut bytes, 70, [1, 1, 1]);
    let second_region_marker = bytes.len();
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    region_member(&mut bytes, 80, [0, 1, 2]);
    region_member(&mut bytes, 81, [0, 2, 2]);
    let terminator = bytes.len();
    bytes.extend_from_slice(&[0; 5]);
    indexed_header(&mut bytes, *b"261", profile_record_index + 3);
    (bytes, selection_at, second_region_marker, terminator)
}

#[test]
fn body_recipe_envelope_uses_its_structural_record_boundary() {
    const RECORD_INDEX: u32 = 100;
    const NEXT_AT: usize = 70_000;
    const EARLY_RECIPE_AT: usize = 100;
    const LATE_RECIPE_AT: usize = NEXT_AT - 32;

    let mut bytes = Vec::new();
    for record_index in [
        RECORD_INDEX,
        RECORD_INDEX,
        RECORD_INDEX + 1,
        RECORD_INDEX + 2,
        RECORD_INDEX + 3,
    ] {
        indexed_header(&mut bytes, *b"365", record_index);
    }
    let prologue_end = bytes.len();
    bytes.resize(NEXT_AT, 0xaa);
    for recipe_at in [EARLY_RECIPE_AT, LATE_RECIPE_AT] {
        bytes[recipe_at..recipe_at + b"body_recipe_data".len()]
            .copy_from_slice(b"body_recipe_data");
    }
    indexed_header(&mut bytes, *b"311", RECORD_INDEX + 4);

    let header = DesignRecordHeader {
        id: "stream:record-100".into(),
        record_index: RECORD_INDEX,
        class_tag: "365".into(),
        byte_offset: 0,
    };
    let early = ConstructionRecipe {
        id: "stream:recipe-early".into(),
        byte_offset: EARLY_RECIPE_AT as u64,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Body,
        design_id: None,
        design_selector: None,
        recipe_index: 0,
        record_index: 0,
    };
    let late = ConstructionRecipe {
        id: "stream:recipe-late".into(),
        byte_offset: LATE_RECIPE_AT as u64,
        recipe_index: 1,
        ..early.clone()
    };

    assert_eq!(
        body_recipe_prologue_end(&bytes, 0, RECORD_INDEX),
        Some(prologue_end)
    );
    assert_eq!(
        body_recipe_operand_end(&bytes, prologue_end, RECORD_INDEX, EARLY_RECIPE_AT),
        Some(NEXT_AT)
    );
    assert_eq!(unique_body_recipe(&bytes, &header, &[&early]), Some(&early));
    assert_eq!(unique_body_recipe(&bytes, &header, &[&early, &late]), None);
}

#[test]
fn sketch_profile_region_selection_preserves_region_and_curve_order() {
    let (bytes, selection_at, _, _) = region_selection_frame();
    let selection =
        parse_sketch_profile_region_selection(&bytes, 100, 0).expect("profile-region selection");
    assert_eq!(selection.record_index, 103);
    assert_eq!(selection.byte_offset, selection_at as u64);
    assert_eq!(selection.class_tag, "327");
    assert_eq!(selection.companion_class_tag, "261");
    assert_eq!(
        selection
            .regions
            .iter()
            .map(|region| {
                region
                    .members
                    .iter()
                    .map(|member| member.curve_primary_id)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![70], vec![80, 81]]
    );
}

#[test]
fn sketch_profile_region_selection_requires_every_delimiter() {
    let (bytes, _, second_region_marker, terminator) = region_selection_frame();
    for offset in [second_region_marker, terminator] {
        let mut changed = bytes.clone();
        changed[offset] = 2;
        assert_eq!(
            parse_sketch_profile_region_selection(&changed, 100, 0),
            None
        );
    }
}

#[test]
fn sketch_profile_region_selection_derives_companion_after_header_shaped_member() {
    let (mut bytes, selection_at, _, _) = region_selection_frame();
    let curve_primary_id_offset = selection_at + 48;
    bytes[curve_primary_id_offset..curve_primary_id_offset + 4].copy_from_slice(b"123X");

    let selection =
        parse_sketch_profile_region_selection(&bytes, 100, 0).expect("profile-region selection");

    assert_eq!(
        selection.regions[0].members[0].curve_primary_id,
        u64::from(u32::from_le_bytes(*b"123X"))
    );
}
