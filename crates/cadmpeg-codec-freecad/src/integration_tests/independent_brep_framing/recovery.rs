// SPDX-License-Identifier: Apache-2.0
//! Damaged-source locator contradictions, #224 comment6375055929 / #215.
use super::Boundary;
use crate::brep::records::polygons::polygon_rows;
use crate::brep::ShapePayloadRecord;
use cadmpeg_ir::{codec::DecodeResult, source_fidelity::parts::PartState};

pub(super) fn assert_recovery_boundaries(
    target: &Boundary,
    fill: u8,
    end: u64,
    damaged: &[Boundary],
    after: &DecodeResult,
) {
    let current_member = damaged
        .iter()
        .find(|part| part.member == target.member && is_member(part))
        .expect("actual damaged BRep member frame");
    assert_eq!(current_member.member_offset, target.member_offset);
    let stream = current_member.stream.as_str();
    for part in after
        .source_fidelity()
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
    {
        let ordinary = part.stream() == stream
            && part.offset() >= target.offset
            && part.offset() + part.byte_len() <= end;
        assert!(ordinary,
            "damage {target:?} fill={fill:#x} escaped independently proved damaged-source recovery: {part:?}");
    }
}

pub(super) fn assert_failed_headers_drop_owned_records(
    before: &DecodeResult,
    after: &DecodeResult,
    previous: &[ShapePayloadRecord],
    current: &[ShapePayloadRecord],
    streams: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeSet<String> {
    let mut failed_payloads = std::collections::BTreeSet::new();
    let old = before.ir().native.namespace("fcstd").unwrap();
    let new = after.ir().native.namespace("fcstd").unwrap();
    for payload in previous {
        let layout = &before.source_fidelity().record_layout()[&payload.id];
        let stream = streams
            .get(layout.stream())
            .expect("actual paired member source");
        let header = after.source_fidelity().parts().values().find(|part| {
            matches!(part.state(), PartState::GivenUp {})
                && part.stream() == stream
                && part.offset() >= layout.offset()
                && part.offset() + part.byte_len() <= layout.offset() + layout.byte_len()
                && ((is_member_level(part.level().as_str()) && part.offset() == layout.offset())
                    || (part.level().as_str().ends_with("_header")
                        && part
                            .parent()
                            .and_then(|id| after.source_fidelity().parts().get(id))
                            .is_some_and(|parent| {
                                matches!(
                                    parent.level().as_str(),
                                    "freecad.brep_text" | "freecad.brep_binary"
                                ) && parent.stream() == stream
                                    && parent.offset() == layout.offset()
                            })))
        });
        let Some(header) = header else { continue };
        failed_payloads.insert(payload.id.clone());
        assert!(!current.iter().any(|row| row.id == payload.id));
        let mut owned = vec![payload.id.clone()];
        for row in polygon_rows(old, previous)
            .unwrap()
            .into_iter()
            .filter(|row| row.owner == payload.id)
        {
            assert!(!polygon_rows(new, current)
                .unwrap()
                .iter()
                .any(|current| current.id == row.id));
            owned.push(row.id);
        }
        for id in owned {
            assert!(
                !after.source_fidelity().record_layout().contains_key(&id),
                "failed header retained {id}"
            );
        }
        for layout in after.source_fidelity().record_layout().values() {
            assert!(
                !layout
                    .reads()
                    .get(header.stream())
                    .into_iter()
                    .flatten()
                    .any(|read| read.start < header.offset() + header.byte_len()
                        && header.offset() < read.end),
                "failed header retained semantic reads: {layout:?}"
            );
        }
    }
    failed_payloads
}

/// One binary level contains eight different table schemas. Select first,
/// middle and last independently for each actual selector, never just across
/// the concatenated rows (which would leave five schemas unexercised).
pub(super) fn choices(candidates: &[&Boundary]) -> std::collections::BTreeSet<usize> {
    let mut tables = std::collections::BTreeMap::new();
    for (index, part) in candidates.iter().enumerate() {
        tables
            .entry((part.table.as_deref(), part.marker))
            .or_insert_with(Vec::new)
            .push(index);
    }
    tables
        .values()
        .flat_map(|rows| [rows[0], rows[rows.len() / 2], rows[rows.len() - 1]])
        .collect()
}

fn is_member_level(level: &str) -> bool {
    matches!(level, "freecad.brep_text" | "freecad.brep_binary")
}
fn is_member(part: &Boundary) -> bool {
    is_member_level(&part.level)
}

pub(super) fn stream_pairs(
    before: &[Boundary],
    after: &[Boundary],
) -> std::collections::BTreeMap<String, String> {
    before
        .iter()
        .filter(|part| is_member(part))
        .map(|old| {
            let current = after
                .iter()
                .find(|part| is_member(part) && part.member == old.member)
                .expect("the same independently opened ZIP member exists");
            assert_eq!(old.member_offset, current.member_offset);
            (old.stream.clone(), current.stream.clone())
        })
        .collect()
}

/// The same authenticated sequential-parent suffix used by given_up_zip.
/// Its current failure may end at a physically shorter EOF; the original
/// counterpart still proves precisely which later records cannot be probed.
pub(super) fn sequential_suffixes(
    before: &DecodeResult,
    after: &DecodeResult,
    streams: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, std::ops::Range<u64>)> {
    use cadmpeg_ir::source_fidelity::parts::PartFraming;
    let mut suffixes = Vec::new();
    for failed in after.source_fidelity().parts().values().filter(|part| {
        matches!(part.state(), PartState::GivenUp {})
            && part.level().framing() == PartFraming::Sequential
    }) {
        let Some((old_stream, _)) = streams
            .iter()
            .find(|(_, stream)| stream.as_str() == failed.stream())
        else {
            continue;
        };
        let Some(original) = before.source_fidelity().parts().values().find(|part| {
            part.stream() == old_stream
                && part.level() == failed.level()
                && part.offset() == failed.offset()
        }) else {
            continue;
        };
        let old_parent = original
            .parent()
            .and_then(|id| before.source_fidelity().parts().get(id))
            .unwrap();
        let new_parent = failed
            .parent()
            .and_then(|id| after.source_fidelity().parts().get(id))
            .unwrap();
        assert_eq!(old_parent.stream(), old_stream);
        assert_eq!(new_parent.stream(), failed.stream());
        assert_eq!(old_parent.level(), new_parent.level());
        assert_eq!(old_parent.offset(), new_parent.offset());
        let original_end = old_parent
            .offset()
            .checked_add(old_parent.byte_len())
            .unwrap();
        let current_end = new_parent
            .offset()
            .checked_add(new_parent.byte_len())
            .unwrap();
        assert!(current_end <= original_end);
        assert!(old_parent.offset() <= original.offset());
        assert!(original.offset() + original.byte_len() <= original_end);
        assert!(new_parent.offset() <= failed.offset());
        assert!(failed.offset() + failed.byte_len() <= current_end);
        suffixes.push((old_stream.clone(), original.offset()..original_end));
    }
    suffixes
}

/// Called only after given_up_zip has authenticated the actual member prefix.
/// An exact cut-containing failed home and its original counterpart prove the
/// physically removed source suffix, independently of semantic dependencies.
pub(super) fn physical_suffixes(
    before: &DecodeResult,
    after: &DecodeResult,
    streams: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, std::ops::Range<u64>)> {
    let mut suffixes = Vec::new();
    for (old_stream, new_stream) in streams {
        let original_member = before
            .source_fidelity()
            .parts()
            .values()
            .find(|part| part.stream() == old_stream && is_member_level(part.level().as_str()))
            .unwrap();
        let current_member = after
            .source_fidelity()
            .parts()
            .values()
            .find(|part| part.stream() == new_stream && is_member_level(part.level().as_str()))
            .unwrap();
        assert_eq!(original_member.offset(), current_member.offset());
        let original_end = original_member
            .offset()
            .checked_add(original_member.byte_len())
            .unwrap();
        let current_end = current_member
            .offset()
            .checked_add(current_member.byte_len())
            .unwrap();
        if current_end >= original_end {
            continue;
        }
        let proven = after
            .source_fidelity()
            .parts()
            .values()
            .filter(|part| {
                part.stream() == new_stream
                    && matches!(part.state(), PartState::GivenUp {})
                    && part.offset().checked_add(part.byte_len()) == Some(current_end)
            })
            .any(|failed| {
                before
                    .source_fidelity()
                    .parts()
                    .values()
                    .filter(|part| {
                        part.stream() == old_stream
                            && part.level() == failed.level()
                            && part.offset() == failed.offset()
                            && part
                                .offset()
                                .checked_add(part.byte_len())
                                .is_some_and(|end| current_end < end && end <= original_end)
                    })
                    .any(|original| {
                        let Some(old_parent) = original
                            .parent()
                            .and_then(|id| before.source_fidelity().parts().get(id))
                        else {
                            return false;
                        };
                        let Some(new_parent) = failed
                            .parent()
                            .and_then(|id| after.source_fidelity().parts().get(id))
                        else {
                            return false;
                        };
                        old_parent.stream() == old_stream
                            && new_parent.stream() == new_stream
                            && old_parent.level() == new_parent.level()
                            && old_parent.offset() == new_parent.offset()
                            && old_parent
                                .offset()
                                .checked_add(old_parent.byte_len())
                                .is_some_and(|old_end| {
                                    new_parent
                                        .offset()
                                        .checked_add(new_parent.byte_len())
                                        .is_some_and(|new_end| {
                                            old_end >= new_end
                                                && new_end == current_end
                                                && old_parent.offset() <= original.offset()
                                                && original
                                                    .offset()
                                                    .checked_add(original.byte_len())
                                                    .is_some_and(|end| end <= old_end)
                                                && new_parent.offset() <= failed.offset()
                                        })
                                })
                    })
            });
        if proven {
            suffixes.push((old_stream.clone(), current_end..original_end));
        }
    }
    suffixes
}
