// SPDX-License-Identifier: Apache-2.0
//! Face-owner occurrences closed over source-scoped edge and carrier identities.

use std::collections::HashMap;
use std::ops::Range;

use crate::families::b2::records::{b2_counted_owners_from_records, B2CountedOwner};
use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::compact_unsigned::CompactUnsigned;
use crate::wire::records::{ConsolidatedFamily, ConsolidatedRecord};

use super::allocation::AllocationJournal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OwnerOccurrence {
    pub(crate) owner_index: usize,
    pub(crate) face_index: usize,
    pub(crate) representation_index: usize,
    pub(crate) carrier_index: usize,
    pub(crate) face_tag: u64,
}

#[derive(Default)]
pub(crate) struct OwnerClosureIndex {
    by_edge: HashMap<usize, Vec<OwnerOccurrence>>,
}

struct CountedFaceReferences {
    carrier: AllocationReference,
    owner_lane: Range<usize>,
    owner_count: u8,
}

impl OwnerClosureIndex {
    pub(crate) fn new(
        data: &[u8],
        records: &[ConsolidatedRecord],
        journal: &AllocationJournal<'_>,
    ) -> Self {
        let mut by_edge = HashMap::<usize, Vec<OwnerOccurrence>>::new();
        for face_record in records {
            if face_record.family() != ConsolidatedFamily::B || face_record.class() != 0x5f {
                continue;
            }
            let Some(payload) = face_record.payload().and_then(|range| data.get(range)) else {
                continue;
            };
            let Some(references) = counted_face_references(payload) else {
                continue;
            };
            let Some(face_index) =
                journal.index(face_record.source_index(), face_record.source_range().start)
            else {
                continue;
            };
            let Some(face_tag) = journal.absolute_address(face_index) else {
                continue;
            };
            let Some(carrier) = owner_reference(
                journal,
                face_index,
                references.carrier.value,
                references.carrier.encoding,
            ) else {
                continue;
            };
            if !surface_carrier(carrier) {
                continue;
            }
            let Some(carrier_index) =
                journal.index(carrier.source_index(), carrier.source_range().start)
            else {
                continue;
            };
            let mut owners: [Option<(usize, B2CountedOwner)>; 127] = std::array::from_fn(|_| None);
            let mut at = references.owner_lane.start;
            let mut complete = true;
            for slot in 0..usize::from(references.owner_count) {
                let Some(reference) = allocation_reference(payload, &mut at) else {
                    complete = false;
                    break;
                };
                let Some(owner_record) =
                    owner_reference(journal, face_index, reference.value, reference.encoding)
                else {
                    complete = false;
                    break;
                };
                let Some(owner_index) = journal.index(
                    owner_record.source_index(),
                    owner_record.source_range().start,
                ) else {
                    complete = false;
                    break;
                };
                if owner_record.family() != ConsolidatedFamily::B
                    || owner_record.class() != 0x62
                    || owners[..slot]
                        .iter()
                        .flatten()
                        .any(|(index, _)| *index == owner_index)
                {
                    complete = false;
                    break;
                }
                let Some(owner) =
                    b2_counted_owners_from_records(data, std::slice::from_ref(owner_record))
                        .into_iter()
                        .next()
                else {
                    complete = false;
                    break;
                };
                if owner.references.len() < 5 || owner.references.len().is_multiple_of(2) {
                    complete = false;
                    break;
                }
                let support_slot = owner.references.len() - 1;
                if owner_reference(
                    journal,
                    owner_index,
                    owner.references[support_slot],
                    owner.reference_encodings[support_slot],
                ) != Some(carrier)
                {
                    complete = false;
                    break;
                }
                owners[slot] = Some((owner_index, owner));
            }
            if !complete || at != references.owner_lane.end {
                continue;
            }
            for (owner_index, owner) in owners.into_iter().flatten() {
                let support_slot = owner.references.len() - 1;
                for (references, encodings) in owner.references[..support_slot]
                    .chunks_exact(2)
                    .zip(owner.reference_encodings[..support_slot].chunks_exact(2))
                {
                    let Some(representation) =
                        owner_reference(journal, owner_index, references[0], encodings[0])
                    else {
                        continue;
                    };
                    if !matches!(
                        (representation.family(), representation.class()),
                        (ConsolidatedFamily::A, 0x20) | (ConsolidatedFamily::B, 0x18 | 0x19 | 0x20)
                    ) {
                        continue;
                    }
                    let Some(edge) =
                        owner_reference(journal, owner_index, references[1], encodings[1])
                    else {
                        continue;
                    };
                    if edge.family() != ConsolidatedFamily::B || edge.class() != 0x5e {
                        continue;
                    }
                    let Some(representation_index) = journal.index(
                        representation.source_index(),
                        representation.source_range().start,
                    ) else {
                        continue;
                    };
                    let Some(edge_index) =
                        journal.index(edge.source_index(), edge.source_range().start)
                    else {
                        continue;
                    };
                    let Some(payload) = representation.payload().and_then(|range| data.get(range))
                    else {
                        continue;
                    };
                    let mut at = 0;
                    let Some(support) = CompactUnsigned::read(payload, &mut at) else {
                        continue;
                    };
                    if journal.resolve_weighted_distance(representation_index, support.value())
                        != Some(carrier)
                    {
                        continue;
                    }
                    by_edge
                        .entry(edge_index)
                        .or_default()
                        .push(OwnerOccurrence {
                            owner_index,
                            face_index,
                            representation_index,
                            carrier_index,
                            face_tag,
                        });
                }
            }
        }
        Self { by_edge }
    }

    pub(crate) fn bind(
        &self,
        edge_index: usize,
        representations: [usize; 2],
        carriers: [usize; 2],
        face_tags: [u64; 2],
    ) -> Option<[OwnerOccurrence; 2]> {
        let [first, second] = self.edge_occurrences(edge_index, face_tags)?;
        if representations[0] == representations[1] {
            return None;
        }
        let sides = if first.representation_index == representations[0]
            && first.carrier_index == carriers[0]
            && second.representation_index == representations[1]
            && second.carrier_index == carriers[1]
        {
            [first, second]
        } else if second.representation_index == representations[0]
            && second.carrier_index == carriers[0]
            && first.representation_index == representations[1]
            && first.carrier_index == carriers[1]
        {
            [second, first]
        } else {
            return None;
        };
        Some(sides)
    }

    pub(crate) fn edge_occurrences(
        &self,
        edge_index: usize,
        mut face_tags: [u64; 2],
    ) -> Option<[OwnerOccurrence; 2]> {
        if face_tags[0] == face_tags[1] {
            return None;
        }
        let [first, second] = self.by_edge.get(&edge_index)?.as_slice() else {
            return None;
        };
        if first.owner_index == second.owner_index
            || first.face_index == second.face_index
            || first.representation_index == second.representation_index
        {
            return None;
        }
        let mut owned_tags = [first.face_tag, second.face_tag];
        owned_tags.sort_unstable();
        face_tags.sort_unstable();
        (owned_tags == face_tags).then_some([*first, *second])
    }
}

fn counted_face_references(payload: &[u8]) -> Option<CountedFaceReferences> {
    let count = payload.first()?.checked_sub(0x80)?;
    if count < 2 {
        return None;
    }
    let mut at = 1;
    let carrier = allocation_reference(payload, &mut at)?;
    let owners_start = at;
    for _ in 1..count {
        allocation_reference(payload, &mut at)?;
    }
    matches!(payload.get(at..), Some([0x03 | 0x05])).then_some(CountedFaceReferences {
        carrier,
        owner_lane: owners_start..at,
        owner_count: count.checked_sub(1)?,
    })
}

fn owner_reference<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    value: u32,
    encoding: AllocationReferenceEncoding,
) -> Option<&'a ConsolidatedRecord> {
    match encoding {
        AllocationReferenceEncoding::BackwardDistance | AllocationReferenceEncoding::WidthCoded => {
            journal.resolve_weighted_distance(origin, value)
        }
        AllocationReferenceEncoding::OwnedChild => journal.resolve_address(
            origin,
            journal
                .address(origin)?
                .checked_add(u64::from(value))?
                .checked_add(1)?,
        ),
        AllocationReferenceEncoding::Selector2
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16 => None,
    }
}

fn surface_carrier(record: &ConsolidatedRecord) -> bool {
    matches!(
        (record.family(), record.class()),
        (ConsolidatedFamily::A, 0x32 | 0x34)
            | (
                ConsolidatedFamily::B,
                0x27 | 0x28 | 0x29 | 0x2a | 0x2b | 0x2d
            )
    )
}

#[cfg(test)]
mod tests {
    use super::OwnerClosureIndex;
    use crate::families::consolidated::allocation::AllocationJournal;
    use crate::wire::records::{consolidated_records, consolidated_records_in_range_sources};

    fn frame(class: u8, increment: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![
            0xb2,
            0x03,
            class,
            u8::try_from(payload.len()).expect("small payload"),
            increment * 4 + 1,
        ];
        bytes.extend_from_slice(payload);
        bytes
    }

    fn compact(value: u8) -> u8 {
        value * 4 + 1
    }

    fn face(distance: u8) -> Vec<u8> {
        frame(0x5f, 1, &[0x82, compact(distance), 0x03, 0x05])
    }

    fn owner(representation: u8, edge: u8, other: u8, carrier: u8) -> Vec<u8> {
        frame(
            0x62,
            1,
            &[
                0x85,
                compact(representation),
                edge,
                compact(representation),
                compact(other),
                compact(carrier),
                0x01,
            ],
        )
    }

    fn stream() -> Vec<Vec<u8>> {
        vec![
            vec![0xa5, 0x03, 0x34, 0, 0, 0, 0, 0x05],
            frame(0x18, 1, &[compact(1)]),
            frame(0x27, 1, &[]),
            frame(0x20, 1, &[compact(1)]),
            frame(0x5e, 1, &[0x03, 0x03, 0x03, 0x03, 0x03, 0x21]),
            face(5),
            owner(5, compact(2), 6, 6),
            face(5),
            owner(5, compact(4), 8, 6),
        ]
    }

    #[test]
    fn owner_closure_binds_both_faces_to_the_selected_representation_order() {
        let data = stream().concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        let closure = OwnerClosureIndex::new(&data, &records, &journal);
        let sides = closure
            .bind(4, [1, 3], [0, 2], [8, 6])
            .expect("closed sides");
        assert_eq!(sides.map(|side| side.owner_index), [6, 8]);
        assert_eq!(sides.map(|side| side.face_index), [5, 7]);
        assert_eq!(sides.map(|side| side.face_tag), [6, 8]);
        let reversed = closure
            .bind(4, [3, 1], [2, 0], [6, 8])
            .expect("reordered sides");
        assert_eq!(reversed, [sides[1], sides[0]]);
        assert!(closure.bind(4, [1, 3], [2, 0], [6, 8]).is_none());
        assert!(closure.bind(4, [1, 3], [0, 2], [6, 7]).is_none());
    }

    #[test]
    fn owner_closure_keeps_equal_tags_in_distinct_sources_separate() {
        let first = stream().concat();
        let boundary = first.len();
        let data = [first.clone(), first].concat();
        let records = consolidated_records_in_range_sources(
            &data,
            [
                std::iter::once(0..boundary),
                std::iter::once(boundary..data.len()),
            ],
        );
        let journal = AllocationJournal::new(&records);
        let closure = OwnerClosureIndex::new(&data, &records, &journal);
        assert!(closure.bind(4, [1, 3], [0, 2], [6, 8]).is_some());
        assert!(closure.bind(13, [10, 12], [9, 11], [6, 8]).is_some());
        assert!(closure.bind(4, [10, 12], [0, 2], [6, 8]).is_none());
    }

    #[test]
    fn owner_closure_requires_adjacent_faces_and_complete_carrier_agreement() {
        let mut frames = stream();
        frames[5] = face(3);
        frames[6] = owner(5, compact(2), 6, 4);
        let data = frames.concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert!(OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 3], [0, 2], [6, 8])
            .is_none());

        let frames = stream();
        let mut data = frames[..6].concat();
        data.push(0);
        data.extend_from_slice(&frames[6..].concat());
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert!(OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 3], [0, 2], [6, 8])
            .is_none());
    }

    #[test]
    fn owner_closure_withholds_extra_occurrences_and_unresolved_reference_forms() {
        let mut frames = stream();
        frames.push(face(9));
        frames.push(owner(9, compact(6), 10, 10));
        let data = frames.concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert!(OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 3], [0, 2], [6, 8])
            .is_none());

        let mut frames = stream();
        frames[6] = frame(
            0x62,
            1,
            &[
                0x85,
                0x06,
                5,
                compact(2),
                compact(5),
                compact(6),
                compact(6),
                1,
            ],
        );
        let data = frames.concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert!(OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 3], [0, 2], [6, 8])
            .is_none());
    }

    #[test]
    fn owner_closure_child_references_use_weighted_allocation_units() {
        let initial = stream();
        let data = [
            initial[..4].concat(),
            face(4),
            owner(4, 0x0b, 5, 5),
            frame(0x5e, 3, &[0x03, 0x03, 0x03, 0x03, 0x03, 0x21]),
            face(7),
            owner(7, compact(2), 10, 8),
        ]
        .concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert_eq!(journal.absolute_address(6), Some(9));
        let closure = OwnerClosureIndex::new(&data, &records, &journal);
        let sides = closure
            .bind(6, [1, 3], [0, 2], [5, 10])
            .expect("weighted children");
        assert_eq!(sides.map(|side| side.owner_index), [5, 8]);
    }

    #[test]
    fn owner_closure_withholds_an_ambiguous_side_assignment() {
        let mut frames = stream();
        frames[7] = face(7);
        frames[8] = owner(7, compact(4), 8, 8);
        let data = frames.concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert!(OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 1], [0, 0], [6, 8])
            .is_none());
    }

    fn seven_loop_face() -> Vec<Vec<u8>> {
        let mut frames = stream()[..4].to_vec();
        for _ in 0..7 {
            frames.push(frame(0x5e, 1, &[0x03, 0x03, 0x03, 0x03, 0x03, 0x21]));
        }
        let mut references = vec![0x88, compact(9)];
        for ordinal in 0..7 {
            references.push(ordinal * 4 + 3);
        }
        references.push(0x03);
        frames.push(frame(0x5f, 1, &references));
        for ordinal in 0..7 {
            let address = 13 + ordinal;
            frames.push(owner(
                address - 4,
                compact(address - (5 + ordinal)),
                address - 11,
                address - 3,
            ));
        }
        frames.push(face(19));
        frames.push(owner(19, compact(16), 10, 20));
        frames
    }

    #[test]
    fn owner_closure_counted_face_binds_seven_loop_owners_to_one_carrier() {
        let data = seven_loop_face().concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        let closure = OwnerClosureIndex::new(&data, &records, &journal);
        let sides = closure
            .bind(4, [1, 3], [0, 2], [20, 12])
            .expect("seven source-bound loops");
        assert_eq!(sides.map(|side| side.owner_index), [20, 12]);
        assert_eq!(sides.map(|side| side.face_index), [19, 11]);
    }

    #[test]
    fn owner_closure_counted_face_selects_nonadjacent_loop_allocations() {
        let initial = stream();
        let data = [
            initial[..5].concat(),
            frame(0x5f, 1, &[0x83, compact(3), 0x03, 0x0b, 0x03]),
            owner(3, compact(2), 1, 4),
            frame(0x5e, 1, &[0x03, 0x03, 0x03, 0x03, 0x03, 0x21]),
            owner(5, compact(1), 1, 6),
            face(9),
            owner(9, compact(6), 3, 10),
        ]
        .concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        let sides = OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 3], [0, 2], [10, 6])
            .expect("explicit loop roster");
        assert_eq!(sides.map(|side| side.owner_index), [10, 6]);
    }

    #[test]
    fn owner_closure_counted_face_requires_every_roster_member_to_close() {
        let mut frames = seven_loop_face();
        frames[18] = owner(15, compact(8), 8, 18);
        let data = frames.concat();
        let records = consolidated_records(&data);
        let journal = AllocationJournal::new(&records);
        assert!(OwnerClosureIndex::new(&data, &records, &journal)
            .bind(4, [1, 3], [0, 2], [20, 12])
            .is_none());

        for payload in [
            vec![0x83, compact(9), 0x03, 0x03, 0x03],
            vec![0x88, compact(9), 0x03, 0x07, 0x0b, 0x0f, 0x13, 0x17, 0x03],
            vec![
                0x88,
                compact(9),
                0x03,
                0x07,
                0x0b,
                0x0f,
                0x13,
                0x17,
                0x1b,
                0x03,
                0,
            ],
            vec![
                0x88,
                compact(9),
                0x03,
                0x07,
                0x0b,
                0x0f,
                0x13,
                0x17,
                0x1b,
                0x07,
            ],
        ] {
            let mut frames = seven_loop_face();
            frames[11] = frame(0x5f, 1, &payload);
            let data = frames.concat();
            let records = consolidated_records(&data);
            let journal = AllocationJournal::new(&records);
            assert!(OwnerClosureIndex::new(&data, &records, &journal)
                .bind(4, [1, 3], [0, 2], [20, 12])
                .is_none());
        }
    }
}
