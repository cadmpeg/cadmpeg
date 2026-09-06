// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rm_face_colors_require_unique_palette_topology_and_stream_joins() {
    let definition = crate::native::om::PartColorDefinition {
        id: "nx:test:color#201".into(),
        color_table: "nx:test:table#0".into(),
        color_index: crate::om::color::PaletteIndex::new(201).unwrap(),
        name: "Iron Gray".into(),
        components: [(0.25_f64, 11), (0.5, 12), (0.75, 13)].map(|(value, offset)| {
            let mut raw = (value * 4.0).to_be_bytes();
            raw[0] -= 0x10;
            (
                crate::om::color::ColorComponent::read(&raw).unwrap(),
                offset,
            )
        }),
        source_offset: 10,
    };
    let assignment = crate::native::om::RmDisplayColorAssignment {
        id: "nx:test:assignment#0".into(),
        ordinal: 0,
        encoding: crate::native::om::RmDisplayColorAssignmentEncoding::Linked {
            object_index: LocatedCompactIndex {
                atom: CompactIndexAtom::read(&[42]).unwrap(),
                offset: 22,
            },
            discriminator: crate::om::discriminators::LinkedIndexDiscriminator::Form16,
            target_index: LocatedCompactIndex {
                atom: CompactIndexAtom::read(&[7]).unwrap(),
                offset: 23,
            },
            indices: [(1, 24), (2, 25), (3, 26)].map(|(value, offset)| LocatedCompactIndex {
                atom: CompactIndexAtom::read(&[value]).unwrap(),
                offset,
            }),
            flag: crate::om::discriminators::LinkedIndexFlag::Form03,
            mode: crate::om::discriminators::IndexRowMode::Form04,
        },
        target_object_id: Some("nx:test:object-id#7".into()),
        color_index: crate::om::color::PaletteIndex::new(201).unwrap(),
        color_definition: definition.id.clone(),
        source_entry: "/Root/FastLoad/RMFastLoad".into(),
        source_offset: 20,
        row_source_offset: 21,
    };
    let record = crate::native::parasolid::ParasolidDeltasRecord {
        id: "nx:test:deltas#0".into(),
        stream_ordinal: 1,
        family: crate::deltas::record_family::RecordFamily::Face {
            node_id: 42,
            references: [1; 11],
        },
        xmt: 99,
        byte_len: 1,
        inflated_offset: 0,
    };
    let face_ids = BTreeSet::from(["nx:s0:face#99".to_string()]);
    let pairs = BTreeMap::from([(0, vec![1])]);
    assert_eq!(
        resolve_rm_face_colors(
            &face_ids,
            std::slice::from_ref(&assignment),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        ),
        vec![(
            "nx:s0:face#99".into(),
            Color {
                r: 0.25,
                g: 0.5,
                b: 0.75,
                a: 1.0,
            },
        )]
    );

    assert_eq!(
        resolve_rm_face_color_bindings(
            &face_ids,
            std::slice::from_ref(&assignment),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        ),
        vec![RmFaceColorBinding {
            face_id: "nx:s0:face#99".into(),
            color_definition: definition.id.clone(),
            source_offset: 20,
        }]
    );

    let mut target_assignment = assignment.clone();
    target_assignment.encoding = crate::native::om::RmDisplayColorAssignmentEncoding::Target {
        target_index: LocatedCompactIndex {
            atom: CompactIndexAtom::read(&[7]).unwrap(),
            offset: 23,
        },
        indices: [(1, 24), (2, 25), (3, 26)].map(|(value, offset)| LocatedCompactIndex {
            atom: CompactIndexAtom::read(&[value]).unwrap(),
            offset,
        }),
        mode: crate::om::discriminators::IndexRowMode::Form04,
    };
    assert_eq!(
        resolve_rm_face_colors(
            &face_ids,
            &[assignment.clone(), target_assignment],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        ),
        vec![(
            "nx:s0:face#99".into(),
            Color {
                r: 0.25,
                g: 0.5,
                b: 0.75,
                a: 1.0,
            },
        )]
    );

    let mut conflicting = assignment;
    conflicting.color_definition = "nx:test:color#other".into();
    assert!(
        resolve_rm_face_colors(&face_ids, &[conflicting], &[definition], &[record], &pairs,)
            .is_empty()
    );
}

#[test]
fn rm_source_color_bindings_require_one_palette_per_source_identity() {
    let assignment = |id: &str, source_id: Option<&str>, color_definition: &str, offset| {
        crate::native::om::RmDisplayColorAssignment {
            id: id.into(),
            ordinal: 0,
            encoding: crate::native::om::RmDisplayColorAssignmentEncoding::Target {
                target_index: LocatedCompactIndex {
                    atom: CompactIndexAtom::read(&[7]).unwrap(),
                    offset,
                },
                indices: [(1, offset + 1), (2, offset + 2), (3, offset + 3)].map(
                    |(value, offset)| LocatedCompactIndex {
                        atom: CompactIndexAtom::read(&[value]).unwrap(),
                        offset,
                    },
                ),
                mode: crate::om::discriminators::IndexRowMode::Form04,
            },
            target_object_id: source_id.map(str::to_owned),
            color_index: crate::om::color::PaletteIndex::new(201).unwrap(),
            color_definition: color_definition.into(),
            source_entry: "/Root/FastLoad/RMFastLoad".into(),
            source_offset: offset,
            row_source_offset: offset,
        }
    };
    let assignments = [
        assignment("assignment-b", Some("source-a"), "color-a", 20),
        assignment("assignment-a", Some("source-a"), "color-a", 10),
        assignment("assignment-c", Some("source-b"), "color-a", 30),
        assignment("assignment-d", Some("source-c"), "color-a", 40),
        assignment("assignment-e", Some("source-c"), "color-b", 50),
        assignment("assignment-f", None, "color-a", 60),
    ];
    assert_eq!(
        resolve_rm_source_color_bindings(&assignments),
        vec![
            RmSourceColorBinding {
                source_id: "source-a".into(),
                color_definition: "color-a".into(),
                source_offset: 10,
            },
            RmSourceColorBinding {
                source_id: "source-b".into(),
                color_definition: "color-a".into(),
                source_offset: 30,
            },
        ]
    );
}
