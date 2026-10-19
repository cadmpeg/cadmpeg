// SPDX-License-Identifier: Apache-2.0

use crate::native::om::display_color::DisplayColorFrame;
use crate::native::om::display_color::RmDisplayColorAssignment;
use crate::native::om::display_color::RmDisplayColorAssignmentEncoding;
use crate::om::column_row::LinkedRow;
use crate::om::column_row::TargetRow;
use crate::om::compact::CompactIndexAtom;

use crate::native::attach::collect_rm_face_ids;
use crate::native::attach::ensure_rm_color_appearance;
use crate::native::attach::resolve_rm_face_color_bindings;
use crate::native::attach::resolve_rm_face_colors;
use crate::native::attach::resolve_rm_source_color_bindings;
use crate::native::attach::Color;
use crate::native::attach::RmFaceColorBinding;
use crate::native::attach::RmSourceColorBinding;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[test]
fn rm_appearance_is_created_once_per_definition() {
    ({
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
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let mut appearances = BTreeMap::new();
        let stream =
            cadmpeg_ir::annotations::StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
        for _ in 0..2 {
            ensure_rm_color_appearance(
                &mut ir,
                &mut annotations,
                &mut appearances,
                &definition,
                &stream,
            )
            .unwrap();
        }
        assert_eq!(ir.model.appearances.len(), 1);
    });
}

#[test]
fn rm_face_identity_lookup_deduplicates_faces() {
    let ids = collect_rm_face_ids(["nx:s0:face#99", "nx:s0:face#99"]);
    assert_eq!(ids.len(), 1);
}

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
    let assignment = RmDisplayColorAssignment {
        id: "nx:test:assignment#0".into(),
        ordinal: 0,
        frame: DisplayColorFrame::new(
            RmDisplayColorAssignmentEncoding::Linked(
                LinkedRow::<(), u64>::new(
                    CompactIndexAtom::read(&[42]).unwrap(),
                    crate::om::discriminators::LinkedIndexDiscriminator::Form16,
                    CompactIndexAtom::read(&[7]).unwrap().into(),
                    [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                    crate::om::discriminators::LinkedIndexFlag::Form03,
                    crate::om::discriminators::IndexRowMode::Form04,
                    22,
                )
                .unwrap(),
            ),
            crate::om::color::PaletteIndex::new(201).unwrap(),
        )
        .unwrap(),
        target_object_id: Some("nx:test:object-id#7".into()),
        color_definition: definition.id.clone(),
        source_entry: "/Root/FastLoad/RMFastLoad".into(),
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
        )
        .expect("valid colors"),
        vec![(
            "nx:s0:face#99".into(),
            Color::new(0.25, 0.5, 0.75, 1.0).expect("valid color"),
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
    target_assignment.frame = DisplayColorFrame::new(
        RmDisplayColorAssignmentEncoding::Target(
            TargetRow::<(), u64>::new(
                CompactIndexAtom::read(&[7]).unwrap().into(),
                [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                crate::om::discriminators::IndexRowMode::Form04,
                22,
            )
            .unwrap(),
        ),
        crate::om::color::PaletteIndex::new(201).unwrap(),
    )
    .unwrap();
    assert_eq!(
        resolve_rm_face_colors(
            &face_ids,
            &[assignment.clone(), target_assignment],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        )
        .expect("valid colors"),
        vec![(
            "nx:s0:face#99".into(),
            Color::new(0.25, 0.5, 0.75, 1.0).expect("valid color"),
        )]
    );

    let mut conflicting = assignment;
    conflicting.color_definition = "nx:test:color#other".into();
    assert!(
        resolve_rm_face_colors(&face_ids, &[conflicting], &[definition], &[record], &pairs,)
            .expect("valid colors")
            .is_empty()
    );
}

/// The pairwise join the indexed resolution replaced: each linked assignment
/// rescans the assignments, the definitions, the deltas records and every
/// paired stream.
fn pairwise_rm_face_color_bindings(
    face_ids: &BTreeSet<String>,
    assignments: &[RmDisplayColorAssignment],
    definitions: &[crate::native::om::PartColorDefinition],
    records: &[crate::native::parasolid::ParasolidDeltasRecord],
    delta_pairs: &BTreeMap<usize, Vec<usize>>,
) -> Vec<RmFaceColorBinding> {
    use crate::native::attach::RmColorChoice;

    let linked_index = |assignment: &RmDisplayColorAssignment| match assignment.frame.encoding() {
        RmDisplayColorAssignmentEncoding::Linked(row) => Some(row.first_index().atom.value()),
        RmDisplayColorAssignmentEncoding::Target(_) => None,
    };
    let mut bindings = Vec::new();
    for (position, assignment) in assignments.iter().enumerate() {
        let Some(object_index) = linked_index(assignment) else {
            continue;
        };
        if assignments[..position]
            .iter()
            .any(|earlier| linked_index(earlier) == Some(object_index))
        {
            continue;
        }
        let mut choice = RmColorChoice::new(assignment);
        for later in &assignments[position + 1..] {
            if linked_index(later) == Some(object_index) {
                choice.observe(later);
            }
        }
        let RmColorChoice::Unique {
            definition,
            source_offset,
        } = choice
        else {
            continue;
        };
        if !definitions
            .iter()
            .any(|candidate| candidate.id == definition)
        {
            continue;
        }
        let mut candidate = None::<String>;
        let mut ambiguous = false;
        for record in records {
            let crate::deltas::record_family::RecordFamily::Face { node_id, .. } = &record.family
            else {
                continue;
            };
            if *node_id != object_index {
                continue;
            }
            let mut partition = None;
            let mut multiple_partitions = false;
            for (raw_partition, deltas) in delta_pairs {
                let Ok(partition_id) = u32::try_from(*raw_partition) else {
                    continue;
                };
                if !deltas
                    .iter()
                    .any(|delta| u32::try_from(*delta).ok() == Some(record.stream_ordinal))
                {
                    continue;
                }
                match partition {
                    None => partition = Some(partition_id),
                    Some(existing) if existing == partition_id => {}
                    Some(_) => {
                        multiple_partitions = true;
                        break;
                    }
                }
            }
            let (false, Some(partition)) = (multiple_partitions, partition) else {
                continue;
            };
            let face_id = format!("nx:s{partition}:face#{}", record.xmt);
            if !face_ids.contains(&face_id) {
                continue;
            }
            match candidate.as_ref() {
                None => candidate = Some(face_id),
                Some(existing) if existing == &face_id => {}
                Some(_) => {
                    ambiguous = true;
                    break;
                }
            }
        }
        let (false, Some(face_id)) = (ambiguous, candidate) else {
            continue;
        };
        bindings.push(RmFaceColorBinding {
            face_id,
            color_definition: definition.to_owned(),
            source_offset,
        });
    }
    bindings.sort_by(|left, right| left.face_id.cmp(&right.face_id));
    bindings
}

#[test]
fn rm_face_color_bindings_match_the_pairwise_join_on_many_assignments() {
    let definition = |index: u32| crate::native::om::PartColorDefinition {
        id: format!("nx:test:color#{index}"),
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
    let color_index = crate::om::color::PaletteIndex::new(201).unwrap();
    let assignment = |ordinal: u32, object_index: u8, color: u32| {
        let offset = 100 + u64::from(ordinal) * 8;
        let encoding = if ordinal % 7 == 3 {
            RmDisplayColorAssignmentEncoding::Target(
                TargetRow::<(), u64>::new(
                    CompactIndexAtom::read(&[object_index]).unwrap().into(),
                    [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                    crate::om::discriminators::IndexRowMode::Form04,
                    offset,
                )
                .unwrap(),
            )
        } else {
            RmDisplayColorAssignmentEncoding::Linked(
                LinkedRow::<(), u64>::new(
                    CompactIndexAtom::read(&[object_index]).unwrap(),
                    crate::om::discriminators::LinkedIndexDiscriminator::Form16,
                    CompactIndexAtom::read(&[7]).unwrap().into(),
                    [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                    crate::om::discriminators::LinkedIndexFlag::Form03,
                    crate::om::discriminators::IndexRowMode::Form04,
                    offset,
                )
                .unwrap(),
            )
        };
        RmDisplayColorAssignment {
            id: format!("nx:test:assignment#{ordinal}"),
            ordinal,
            frame: DisplayColorFrame::new(encoding, color_index).unwrap(),
            target_object_id: None,
            color_definition: format!("nx:test:color#{color}"),
            source_entry: "/Root/FastLoad/RMFastLoad".into(),
        }
    };
    // A third of the object indices mix palettes; colour 4 has no definition.
    let assignments = (0..600_u32)
        .map(|ordinal| {
            let object_index = u8::try_from((ordinal * 37) % 100).unwrap();
            let color = if object_index % 3 == 0 {
                (ordinal / 100) % 5
            } else {
                u32::from(object_index) % 5
            };
            assignment(ordinal, object_index, color)
        })
        .collect::<Vec<_>>();
    let definitions = (0..4).chain([1]).map(definition).collect::<Vec<_>>();
    let records = (0..330_u32)
        .map(|ordinal| crate::native::parasolid::ParasolidDeltasRecord {
            id: format!("nx:test:deltas#{ordinal}"),
            stream_ordinal: [1, 2, 5, 6, 3, 0]
                [usize::try_from((ordinal / 110 + ordinal) % 6).unwrap()],
            family: if ordinal % 11 == 5 {
                crate::deltas::record_family::RecordFamily::Shell {
                    node_id: ordinal % 110,
                    references: [1; 8],
                }
            } else {
                crate::deltas::record_family::RecordFamily::Face {
                    node_id: ordinal % 110,
                    references: [1; 11],
                }
            },
            xmt: if ordinal < 220 {
                (ordinal % 110) % 40
            } else {
                (ordinal * 7) % 40
            },
            byte_len: 1,
            inflated_offset: 0,
        })
        .collect::<Vec<_>>();
    // Stream 3 pairs with two partitions and stream 0 with none; the
    // unconvertible partition and delta are ignored.
    let delta_pairs = BTreeMap::from([
        (0, vec![1, 2, 3, 3]),
        (4, vec![3, 5]),
        (6, vec![6, usize::MAX]),
        (usize::MAX, vec![1]),
    ]);
    let face_ids = [0, 4, 6]
        .into_iter()
        .flat_map(|partition| {
            (0..40)
                .filter(|xmt| xmt % 3 != 0)
                .map(move |xmt| format!("nx:s{partition}:face#{xmt}"))
        })
        .collect::<BTreeSet<_>>();

    let expected = pairwise_rm_face_color_bindings(
        &face_ids,
        &assignments,
        &definitions,
        &records,
        &delta_pairs,
    );
    assert!(expected.len() >= 20);
    assert_eq!(
        resolve_rm_face_color_bindings(
            &face_ids,
            &assignments,
            &definitions,
            &records,
            &delta_pairs,
        ),
        expected
    );
}

#[test]
fn rm_source_color_bindings_require_one_palette_per_source_identity() {
    let assignment = |id: &str, source_id: Option<&str>, color_definition: &str, offset| {
        let color_index = crate::om::color::PaletteIndex::new(201).unwrap();
        RmDisplayColorAssignment {
            id: id.into(),
            ordinal: 0,
            frame: DisplayColorFrame::new(
                RmDisplayColorAssignmentEncoding::Target(
                    crate::om::column_row::TargetRow::<(), u64>::new(
                        CompactIndexAtom::read(&[7]).unwrap().into(),
                        [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                        crate::om::discriminators::IndexRowMode::Form04,
                        offset + u64::from(color_index.display_byte_len()),
                    )
                    .unwrap(),
                ),
                color_index,
            )
            .unwrap(),
            target_object_id: source_id.map(str::to_owned),
            color_definition: color_definition.into(),
            source_entry: "/Root/FastLoad/RMFastLoad".into(),
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
