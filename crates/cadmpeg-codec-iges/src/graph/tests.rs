// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    build, cyclic_transform_nodes, ParameterResolver, ReferenceEdge, ReferenceKind, Resolution,
    MAX_POINTER_SEQUENCE,
};
use crate::directory::{DirectoryEntry, Status};
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

fn directory_entry(sequence: u32, entity_type: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence,
        entity_type,
        parameter_start: 1,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag: 0,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 1,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn parameter_pointers_enforce_the_seven_digit_sequence_limit() {
    let maximum = u32::try_from(MAX_POINTER_SEQUENCE).unwrap();
    let directory = [directory_entry(maximum, 116)];
    let resolver = ParameterResolver::new(&directory);

    assert_eq!(
        resolver.resolve(1, 0, i64::from(maximum), "test", |_| true),
        Some(maximum)
    );
    assert_eq!(
        resolver.resolve(1, 1, i64::from(maximum) + 1, "test", |_| true),
        None
    );
    assert_eq!(
        resolver.resolve_negative(2, 0, -i64::from(maximum), "test", |_| true),
        Some(maximum)
    );
    assert_eq!(
        resolver.resolve_negative(2, 1, -i64::from(maximum) - 1, "test", |_| true),
        None
    );

    let mut graph = BTreeMap::new();
    resolver.append_to(&mut graph);
    assert_eq!(
        graph[&1]
            .iter()
            .map(|edge| edge.resolution)
            .collect::<Vec<_>>(),
        vec![Resolution::Resolved, Resolution::OutOfRange]
    );
    assert_eq!(
        graph[&2]
            .iter()
            .map(|edge| edge.resolution)
            .collect::<Vec<_>>(),
        vec![Resolution::Resolved, Resolution::OutOfRange]
    );
}

#[test]
fn directory_pointers_enforce_the_seven_digit_sequence_limit() {
    let maximum = u32::try_from(MAX_POINTER_SEQUENCE).unwrap();
    let mut source = directory_entry(1, 116);
    source.transform = i64::from(maximum);
    let graph = build(&[source, directory_entry(maximum, 124)]);
    let edge = graph[&1]
        .iter()
        .find(|edge| edge.kind == ReferenceKind::Transform)
        .unwrap();
    assert_eq!(edge.resolution, Resolution::Resolved);

    let mut source = directory_entry(1, 116);
    source.transform = i64::from(maximum) + 1;
    let graph = build(&[source]);
    let edge = graph[&1]
        .iter()
        .find(|edge| edge.kind == ReferenceKind::Transform)
        .unwrap();
    assert_eq!(edge.resolution, Resolution::OutOfRange);
    assert!(edge.target.is_none());
}

#[test]
fn transform_cycle_detection_does_not_rewalk_a_long_acyclic_prefix() {
    let chain_length = 100_000_u32;
    let edges = (1..=chain_length)
        .map(|source| {
            let target = source + 1;
            (
                source,
                vec![ReferenceEdge {
                    kind: ReferenceKind::Transform,
                    raw_pointer: i64::from(target),
                    target: Some(format!("iges:entity:directory#{target}")),
                    resolution: Resolution::Resolved,
                    expected: "type-124".into(),
                    parameter_index: None,
                }],
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert!(cyclic_transform_nodes(&edges).is_empty());
}

#[test]
fn inspect_preserves_transform_cycles_as_named_reference_states() {
    let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
    let mut bytes = fixed_ascii_with_global(global);
    bytes.truncate(bytes.len() - 81);
    bytes.extend(directory_card(
        ["124", "1", "0", "0", "0", "0", "3", "0", "00000000"],
        1,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "1"],
        2,
    ));
    bytes.extend(directory_card(
        ["124", "2", "0", "0", "0", "0", "1", "0", "00000000"],
        3,
    ));
    bytes.extend(directory_card(
        ["124", "0", "0", "1", "0", "", "", "XFORM", "2"],
        4,
    ));
    let matrix = b"124,1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.;";
    bytes.extend(parameter_card(matrix, 1, 1));
    bytes.extend(parameter_card(matrix, 3, 2));
    let global_cards = global_card_count(global);
    bytes.extend(card(
        format!("S0000001G{global_cards:07}D0000004P0000002").as_bytes(),
        b'T',
        1,
    ));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes.as_slice()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"references.cyclic=2".into()));

    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let cycle_losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::PointerUnresolved.kind())
        .collect::<Vec<_>>();
    assert_eq!(cycle_losses.len(), 2);
    assert!(cycle_losses
        .iter()
        .all(|loss| loss.message.contains("Cyclic resolution")));
}
