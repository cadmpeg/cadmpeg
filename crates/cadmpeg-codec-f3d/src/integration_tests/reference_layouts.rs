// SPDX-License-Identifier: Apache-2.0
//! Persistent references, lost-edge references and material assignments
//! keep their source offsets as `record_layout` pointers (#225 comment
//! 6358238990 item 4; #214).

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

use crate::F3dCodec;

/// The byte length of a lost-edge record (header and marker) and of the
/// class tag and index of the following header (docs/formats/f3d.md).
const LOST_EDGE_READ: u64 = 48 + 11;

#[test]
fn reference_records_keep_their_offsets_in_record_layout_and_none_in_metadata() {
    let source = include_bytes!("../../tests/golden/fixtures/design_appearances_protein.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source[..]), &DecodeOptions::default())
        .expect("design fixture decodes");
    let native = decoded.ir().native.namespace("f3d").expect("f3d native");
    let fidelity = decoded.source_fidelity();
    let ids = |arena: &str| -> Vec<String> {
        native
            .arenas()
            .get(arena)
            .into_iter()
            .flatten()
            .map(|record| record.id().to_owned())
            .collect()
    };
    let layout = |id: &str| {
        fidelity
            .record_layout()
            .get(id)
            .unwrap_or_else(|| panic!("{id} has a record layout"))
    };
    let reads = |id: &str| -> Vec<std::ops::Range<u64>> {
        let layout = layout(id);
        layout.reads()[layout.stream()].clone()
    };
    let (persistent, lost_edge, materials) = (
        ids("persistent_references"),
        ids("lost_edge_references"),
        ids("design_material_assignments"),
    );
    assert!(!persistent.is_empty());
    assert!(!materials.is_empty());
    for id in persistent.iter().chain(&lost_edge).chain(&materials) {
        let prefix = format!("{id}.");
        assert!(
            !fidelity.metadata.keys().any(|key| key.starts_with(&prefix)),
            "{id} keeps no metadata"
        );
    }
    // A persistent reference: the property starts at `byte_offset`, its u64
    // value is its last eight bytes, and the scope reads the property whole.
    for id in &persistent {
        let layout = layout(id);
        let pointers = layout.pointers();
        assert_eq!(pointers.len(), 2);
        assert_eq!(pointers["byte_offset"], layout.offset());
        assert_eq!(pointers["value_offset"] + 8, layout.byte_len());
        assert_eq!(
            reads(id),
            [layout.offset()..layout.offset() + layout.byte_len()]
        );
    }
    // A lost-edge reference: the class tag follows its u32 length, the record
    // index follows the three digits, the marker follows the 29-byte header
    // and the next header follows the marker.
    for id in &lost_edge {
        let layout = layout(id);
        let pointers = layout.pointers();
        let start = pointers["record_byte_offset"];
        assert_eq!(start, layout.offset());
        assert_eq!(pointers["class_tag_offset"], start + 4);
        assert_eq!(pointers["record_index_offset"], start + 7);
        assert_eq!(pointers["byte_offset"], start + 29);
        assert_eq!(pointers["next_byte_offset"], start + 48);
        assert!(layout.byte_len() >= 48);
        assert_eq!(reads(id), [start..start + LOST_EDGE_READ]);
    }
    // A material assignment: the extent is its body-presentation frame, which
    // holds the entity ID, the visual token and the material values; the
    // scope reads the frame whole and the 16-byte body-map pair it joins.
    for id in &materials {
        let layout = layout(id);
        let pointers = layout.pointers();
        let frame = layout.offset()..layout.offset() + layout.byte_len();
        for field in [
            "entity_id_offset",
            "visual_guid_offset",
            "physical_token_offset",
        ] {
            assert!(frame.contains(&pointers[field]), "{id} {field}");
        }
        if let Some(preset) = pointers.get("visual_preset_offset") {
            assert!(frame.contains(preset));
        }
        let pair = pointers["asm_body_key_offset"];
        assert_eq!(pointers["entity_suffix_offset"], pair + 8);
        let mut expected = vec![frame.clone(), pair..pair + 16];
        expected.sort_by_key(|range| range.start);
        let actual = reads(id);
        let covered = |range: &std::ops::Range<u64>| {
            actual
                .iter()
                .any(|read| read.start <= range.start && range.end <= read.end)
        };
        assert!(expected.iter().all(covered), "{id} reads {actual:?}");
        let total: u64 = actual.iter().map(|read| read.end - read.start).sum();
        let disjoint = pair + 16 <= frame.start || frame.end <= pair;
        assert_eq!(
            total,
            if disjoint {
                layout.byte_len() + 16
            } else {
                layout.byte_len()
            }
        );
    }
}
