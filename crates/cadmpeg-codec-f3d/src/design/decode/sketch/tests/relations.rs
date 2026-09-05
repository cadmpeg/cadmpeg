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

#[test]
fn variable_width_relation_uses_counted_runs_and_next_record_boundary() {
    // The eleven-byte reference form puts each pair at fifteen bytes: the
    // reference at `marker`, its relation ordinal four bytes later.
    let mut record = vec![0u8; 127];
    record[0..4].copy_from_slice(&3u32.to_le_bytes());
    record[4..7].copy_from_slice(b"286");
    record[7..11].copy_from_slice(&1239u32.to_le_bytes());
    record[19] = 1;
    record[20..24].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(24, 1224u32), (39, 1228), (54, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 9].copy_from_slice(&u64::from(reference).to_le_bytes());
    }
    record[35..39].copy_from_slice(&3u32.to_le_bytes());
    record[50..54].copy_from_slice(&1u32.to_le_bytes());
    // Offset 69 is the base level's property-block presence byte; the
    // `ParentNode` reference follows it.
    record[70] = 1;
    record[71..79].copy_from_slice(&1041u64.to_le_bytes());
    record[81..89].copy_from_slice(&4u64.to_le_bytes());
    record[89..93].copy_from_slice(&3u32.to_le_bytes());
    for (marker, reference) in [(93, 1224u32), (104, 1228), (115, 1236)] {
        record[marker] = 1;
        record[marker + 1..marker + 9].copy_from_slice(&u64::from(reference).to_le_bytes());
    }
    let mut bytes = record.clone();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"277");
    bytes.extend_from_slice(&1240u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 11), Some(127));
    let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::Plain).unwrap();
    assert_eq!(parsed.members.iter().map(|row| row.reference.value).collect::<Vec<_>>(), [1224, 1228, 1236]);
    assert_eq!(parsed.members.iter().map(|row| row.relation_ordinal).collect::<Vec<_>>(), [3, 1, 0]);
    assert_eq!(parsed.auxiliary_references.iter().map(|row| row.value).collect::<Vec<_>>(), [] as [u32; 0]);
    assert_eq!(parsed.owner_reference, 1041);
    assert_eq!(parsed.state, 4);
    assert_eq!(parsed.state_offset, 81);
    assert_eq!(parsed.entity_genesis, None);
    assert_eq!(parsed.return_members.iter().map(|row| row.value).collect::<Vec<_>>(), [1224, 1228, 1236]);
    assert_eq!(parsed.parsed_end, 127);
}

#[test]
fn indexed_record_search_requires_the_expected_identity() {
    let mut bytes = vec![0xaa; 9];
    let decoy = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"278");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0xbb; 7]);
    let expected = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"306");
    bytes.extend_from_slice(&42u32.to_le_bytes());

    assert_eq!(next_indexed_record_offset(&bytes, 0), Some(decoy));
    assert_eq!(
        next_indexed_record_offset_with_index(&bytes, 0, 42),
        Some(expected)
    );
}

#[test]
fn genesis_relation_parses_u64_text_frame_mask_and_relation_ordinals() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 2394);
    auxiliary.extend_from_slice(&[0u8; 6]);
    // The second text-frame reference is absent.
    auxiliary.push(0);
    let record = genesis_relation_record(
        &[(2394, 0), (2403, 0), (2404, 0)],
        2,
        &auxiliary,
        1425,
        0x100_0000_0000,
        &[2403, 2404],
    );
    let parsed = parse_classed_sketch_relation(&record, SketchRelationClass::TextFrame).unwrap();
    assert_eq!(parsed.members.iter().map(|row| row.reference.value).collect::<Vec<_>>(), [2394, 2403, 2404]);
    assert_eq!(parsed.members.iter().map(|row| row.relation_ordinal).collect::<Vec<_>>(), [0, 0, 0]);
    assert_eq!(parsed.entity_genesis, Some(2));
    assert_eq!(parsed.auxiliary_references.iter().map(|row| row.value).collect::<Vec<_>>(), [2394]);
    assert_eq!(parsed.owner_reference, 1425);
    assert_eq!(parsed.state, 0x100_0000_0000);
    assert_eq!(parsed.return_members.iter().map(|row| row.value).collect::<Vec<_>>(), [2403, 2404]);
    assert_eq!(
        crate::records::constraint_kinds_from_state(parsed.state),
        (vec![SketchConstraintKind::TextFrame], 0)
    );
    assert_eq!(
        decode_pattern_definition(&record, &parsed),
        Some(crate::records::SketchPatternDefinition::TextFrame {
            text_reference: 2394
        })
    );
}

#[test]
fn genesis_relation_parses_text_path_glyph_run() {
    let glyphs: [[[f64; 4]; 4]; 2] = [
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, -5.0627],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.0, 0.6216],
            [0.0, 1.0, 0.0, -5.0627],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ];
    let mut auxiliary = vec![1u8];
    push_reference(&mut auxiliary, 304);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&2u32.to_le_bytes());
    for transform in &glyphs {
        auxiliary.extend_from_slice(&16u32.to_le_bytes());
        for value in transform.iter().flatten() {
            auxiliary.extend_from_slice(&value.to_le_bytes());
        }
    }
    let record = genesis_relation_record(
        &[(237, 1), (304, 0)],
        2,
        &auxiliary,
        201,
        0x200_0000_0000,
        &[237],
    );
    let parsed = parse_classed_sketch_relation(
        &record,
        SketchRelationClass::TextPath { leading_flag: true },
    )
    .unwrap();
    assert_eq!(parsed.members.iter().map(|row| row.reference.value).collect::<Vec<_>>(), [237, 304]);
    assert_eq!(parsed.members.iter().map(|row| row.relation_ordinal).collect::<Vec<_>>(), [1, 0]);
    assert_eq!(parsed.entity_genesis, Some(2));
    assert_eq!(parsed.auxiliary_references.iter().map(|row| row.value).collect::<Vec<_>>(), [304]);
    assert_eq!(parsed.owner_reference, 201);
    assert_eq!(parsed.state, 0x200_0000_0000);
    assert_eq!(parsed.return_members.iter().map(|row| row.value).collect::<Vec<_>>(), [237]);
    assert_eq!(parsed.text_glyph_transforms.as_deref(), Some(&glyphs[..]));
    assert_eq!(
        crate::records::constraint_kinds_from_state(parsed.state),
        (vec![SketchConstraintKind::TextPath], 0)
    );
    assert_eq!(
        decode_pattern_definition(&record, &parsed),
        Some(crate::records::SketchPatternDefinition::TextPath {
            text_reference: 304,
            glyph_transforms: glyphs.to_vec(),
        })
    );
}

#[test]
fn genesis_relation_parses_circular_pattern_auxiliary_run() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 336);
    auxiliary.extend_from_slice(&[0u8; 6]);
    push_reference(&mut auxiliary, 333);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
    auxiliary.extend_from_slice(&3u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0u8; 9]);
    let record = genesis_relation_record(
        &[(280, 1), (291, 1), (327, 0), (330, 0)],
        2,
        &auxiliary,
        201,
        0x1000_0000,
        &[291, 327, 330, 280],
    );
    let parsed =
        parse_classed_sketch_relation(&record, SketchRelationClass::CircularPattern).unwrap();
    assert_eq!(parsed.members.iter().map(|row| row.relation_ordinal).collect::<Vec<_>>(), [1, 1, 0, 0]);
    assert_eq!(parsed.auxiliary_references.iter().map(|row| row.value).collect::<Vec<_>>(), [336, 333]);
    assert_eq!(parsed.state, 0x1000_0000);
    assert_eq!(
        decode_pattern_definition(&record, &parsed),
        Some(crate::records::SketchPatternDefinition::Circular {
            angle_parameter: 336,
            count_parameter: 333,
            evaluated_angle: std::f64::consts::TAU,
            evaluated_count: 3,
        })
    );
}

#[test]
fn genesis_relation_parses_rectangular_pattern_auxiliary_run() {
    let mut auxiliary = Vec::new();
    push_reference(&mut auxiliary, 0);
    auxiliary.extend_from_slice(&[0u8; 10]);
    auxiliary.extend_from_slice(&3u32.to_le_bytes());
    push_reference(&mut auxiliary, 464);
    auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [1.0f64, 0.0, 0.0, 3.0] {
        auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut auxiliary, 470);
    auxiliary.extend_from_slice(&[0u8; 6]);
    auxiliary.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut auxiliary, 467);
    auxiliary.extend_from_slice(&[0u8; 6]);
    for value in [0.0f64, 1.0, 0.0, 0.5] {
        auxiliary.extend_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut auxiliary, 473);
    auxiliary.extend_from_slice(&[0u8; 6]);
    let record = genesis_relation_record(
        &[(352, 3), (353, 1), (442, 0), (445, 0)],
        2,
        &auxiliary,
        201,
        0x2000_0000,
        &[353, 352, 442, 445],
    );
    let parsed =
        parse_classed_sketch_relation(&record, SketchRelationClass::RectangularPattern).unwrap();
    assert_eq!(parsed.members.iter().map(|row| row.relation_ordinal).collect::<Vec<_>>(), [3, 1, 0, 0]);
    assert_eq!(parsed.auxiliary_references.iter().map(|row| row.value).collect::<Vec<_>>(), [464, 470, 467, 473]);
    assert_eq!(parsed.rectangular_reference_count, Some(0));
    assert_eq!(parsed.state, 0x2000_0000);
    let Some(crate::records::SketchPatternDefinition::Rectangular { directions }) =
        decode_pattern_definition(&record, &parsed)
    else {
        panic!("expected rectangular pattern definition");
    };
    assert_eq!(directions[0].evaluated_count, 3);
    assert_eq!(directions[0].count_parameter, 464);
    assert_eq!(directions[0].direction, [1.0, 0.0, 0.0]);
    assert_eq!(directions[0].evaluated_distance, 3.0);
    assert_eq!(directions[0].distance_parameter, 470);
    assert_eq!(directions[1].evaluated_count, 1);
    assert_eq!(directions[1].count_parameter, 467);
    assert_eq!(directions[1].direction, [0.0, 1.0, 0.0]);
    assert_eq!(directions[1].evaluated_distance, 0.5);
    assert_eq!(directions[1].distance_parameter, 473);
}

#[test]
fn genesis_entity_header_variant_resolves_suffix_and_id() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 10]);
    push_genesis_block(&mut bytes, 4);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    for unit in "0_201".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let (entity_id, optional_slot_present, end) =
        parse_genesis_entity_header(&bytes, 0).unwrap();
    assert_eq!(entity_id.suffix(), 201);
    assert_eq!(entity_id.as_str(), "0_201");
    assert!(!optional_slot_present);
    assert_eq!(end, bytes.len());
    assert!(parse_settled_entity_header(&bytes, 0).is_none());
}

fn genesis_relation_record(
    members: &[(u32, u32)],
    genesis: u64,
    auxiliary: &[u8],
    owner: u32,
    mask: u64,
    returns: &[u32],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"298");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.push(1);
    out.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for (reference, relation_ordinal) in members {
        push_reference(&mut out, *reference);
        out.extend_from_slice(&[0u8; 6]);
        out.extend_from_slice(&relation_ordinal.to_le_bytes());
    }
    push_genesis_block(&mut out, genesis);
    out.extend_from_slice(auxiliary);
    push_reference(&mut out, owner);
    out.extend_from_slice(&[0u8; 6]);
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(&u32::try_from(returns.len()).unwrap().to_le_bytes());
    for reference in returns {
        push_reference(&mut out, *reference);
        out.extend_from_slice(&[0u8; 6]);
    }
    out.extend_from_slice(&[0u8; 4]);
    out
}
