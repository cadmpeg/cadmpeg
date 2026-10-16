// SPDX-License-Identifier: Apache-2.0
//! Schema ownership selects WORLD layout even when a different width can parse.

use super::{WorldLayout, WorldReferences};
use crate::deltas::census::walk;
use crate::records::instance::SchemaNode;
use crate::test_support::{with_decode_context, world};

#[test]
fn all_declared_world_layouts_align_with_the_independent_first_node_reader() {
    for (count, layout, length, references) in [
        (15, WorldLayout::Legacy, 48, 11),
        (18, WorldLayout::Mesh, 57, 14),
        (19, WorldLayout::Lattice, 60, 15),
    ] {
        let first = world::first_node(count);
        let schema_len = world::schema(count).len();
        with_decode_context(|ctx| {
            let (node, end) = SchemaNode::read(ctx, &first, 0).unwrap().unwrap();
            assert_eq!(WorldLayout::from_schema(node.definition()), Some(layout));
            let (xmt, targets, repeated_end) = layout.read(&first, schema_len).unwrap();
            assert_eq!(xmt, 2);
            assert_eq!(end, repeated_end);
            assert_eq!(end - schema_len, length);
            assert_eq!(targets.values(), (3..references + 3).collect::<Vec<_>>());
            let mut followed = world::record(count, false);
            followed.extend_from_slice(&[0, 1, 0, 1]);
            assert_eq!(layout.read(&followed, 2).unwrap().2, 2 + length);
            for len in 0..length {
                assert!(layout.read(&followed[..2 + len], 2).is_none());
            }
        });
    }
}

#[test]
fn unowned_world_candidates_do_not_select_a_layout() {
    for count in [15, 18, 19] {
        let bytes = world::record(count, false);
        let census = with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
        assert!(census.records.iter().all(|record| record.kind() != 101));
    }
}

#[test]
fn declared_worlds_use_one_layout_for_direct_and_escaped_repetitions() {
    for count in [15, 18, 19] {
        for escaped in [false, true] {
            let mut bytes = world::first_node(count);
            let at = bytes.len();
            bytes.extend_from_slice(&world::record(count, escaped));
            let end = bytes.len();
            bytes.extend_from_slice(&[0xde, 0xad]);
            let census = with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
            assert_eq!(census.schema_nodes.len(), 1);
            let records: Vec<_> = census.records.iter().filter(|r| r.kind() == 101).collect();
            assert_eq!(records.len(), 1);
            assert_eq!((records[0].offset, records[0].end), (at, end));
            let width = match count {
                15 => 11,
                18 => 14,
                _ => 15,
            };
            assert_eq!(
                records[0].family.references(),
                (3..width + 3).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn world_wire_cardinality_and_null_targets_are_rejected() {
    for count in 0..18 {
        let values = vec![1; count];
        assert_eq!(
            WorldReferences::try_from(values).is_ok(),
            matches!(count, 11 | 14 | 15)
        );
    }
    for count in [11, 14, 15] {
        let mut values = vec![1; count];
        values[count - 1] = 0;
        assert!(WorldReferences::try_from(values).is_err());
    }
}

#[test]
fn world_references_require_canonical_positive_integer_storage() {
    let valid = world::record(19, false);
    assert_eq!(WorldLayout::Lattice.read(&valid, 2).unwrap().0, 2);
    // The primary writer uses a compact positive short for index two.
    // A negative remainder with quotient zero is not its extended encoding.
    let mut zero_quotient = valid.clone();
    zero_quotient.splice(4..6, [0xff, 0xfd, 0, 0]);
    assert!(WorldLayout::Lattice.read(&zero_quotient, 2).is_none());
    let mut zero_target = valid;
    zero_target[4..6].copy_from_slice(&[0, 0]);
    assert!(WorldLayout::Lattice.read(&zero_target, 2).is_none());
}

#[test]
fn duplicate_declarations_do_not_override_the_first_layout() {
    let mut bytes = world::first_node(18);
    bytes.extend_from_slice(&world::first_node(19));
    let at = bytes.len();
    bytes.extend_from_slice(&world::record(18, false));
    let census = with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
    assert_eq!(census.schema_nodes.len(), 1);
    let repeated = census
        .records
        .iter()
        .find(|record| record.offset == at)
        .unwrap();
    assert_eq!(repeated.kind(), 101);
    assert_eq!(repeated.family.references(), (3..17).collect::<Vec<_>>());
}

#[test]
fn an_explicit_other_base_version_cannot_be_interpreted_as_v13() {
    let mut bytes = crate::test_support::test_pmarks::header();
    let version = bytes.windows(5).position(|w| w == b"13006").unwrap();
    bytes[version..version + 5].copy_from_slice(b"11004");
    bytes.extend_from_slice(&world::first_node(19));
    bytes.extend_from_slice(&world::record(19, false));
    let census = with_decode_context(|ctx| walk(ctx, &bytes)).unwrap();
    assert!(census.transmit_header.is_some());
    assert!(census.schema_nodes.is_empty());
    assert!(census.records.iter().all(|record| record.kind() != 101));
}
