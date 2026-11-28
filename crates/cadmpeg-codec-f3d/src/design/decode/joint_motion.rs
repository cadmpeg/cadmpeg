// SPDX-License-Identifier: Apache-2.0
//! Joint motion: the freedom entries of a joint's motion record and the limit
//! lanes of its joint record.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{bounded_len, u64_from_index, View};
use cadmpeg_core::CodecError;

use crate::container::ContainerScan;
use crate::design::assembly::compact_placement_as_built_scope;
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::layout::assembly_motion_parent_entry as parent_entry;
use crate::layout::assembly_motion_parent_run as parents;
use crate::records::entity_header::SegmentType;
use crate::records::feature::joint_motion::{
    DesignJointFreedomWord, DesignJointLimitLanes, DesignJointMotion, DesignJointRest,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::container::ContainerRole;

use super::scopes::shared_frames::marked_record_reference;
use super::sketch::IndexedRecordOffsets;

/// A freedom entry before its word: byte `1`, f64 `0`, and f64 π. The u32
/// word, `1` or `3`, and the u32 code follow; a word-`1` entry then stores an
/// f64.
const FREEDOM_ENTRY_PREFIX: [u8; 17] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0x18, 0x2d, 0x44, 0x54, 0xfb, 0x21, 0x09, 0x40,
];
/// A word-`1` freedom entry whose second f64 is `0` rather than π.
const ZERO_LANE_FREEDOM_ENTRY_PREFIX: [u8; 17] =
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// The words a freedom entry stores before its code.
const FREEDOM_ENTRY_WORDS: [u32; 2] = [1, 3];
/// The freedom entries of one record: each code with its word.
type FreedomEntries = Vec<(Located<u32>, DesignJointFreedomWord)>;
/// Offset of the joint reference in a current-generation motion record.
const MOTION_JOINT_REFERENCE: usize = 24;
/// Offsets after the joint record's self-reference marker.
const LIMIT_FLAGS_AFTER_SELF_REFERENCE: usize = 11;
const LIMIT_VALUES_AFTER_SELF_REFERENCE: usize = 23;
/// The rest flag follows the two limit flags; the rest value follows the
/// minimum.
const REST_FLAG_AFTER_SELF_REFERENCE: usize = 13;
const REST_VALUE_AFTER_SELF_REFERENCE: usize = 39;

pub(crate) const JOINT_TYPE_GUID: &str = "7F5A0426-1D2A-4DC4-9F06-58631F69D2F5";
const MOTION_TYPE_GUID: &str = "8C188F11-5D1B-4E1C-8E87-AF7214CA3C83";
const OCCURRENCE_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

#[derive(Default)]
struct AsBuiltJoints<'a> {
    types: HashMap<u64, (&'a str, u32)>,
    joints: HashSet<u32>,
}

/// Decode the motion of every joint whose motion record carries freedom
/// entries, in every Design `BulkStream`.
pub(crate) fn decode_joint_motions(
    scan: &ContainerScan<'_>,
    scopes: &[DesignParameterScope],
    types: &[SegmentType],
) -> Result<Vec<DesignJointMotion>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = crate::ids::native_scope(&entry.name);
        let assemble_scopes: HashMap<u32, bool> = scopes
            .iter()
            .filter(|scope| crate::ids::native_stream(&scope.id) == Some(stream.as_str()))
            .map(|scope| {
                (
                    scope.record_index,
                    design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Assemble),
                )
            })
            .collect();
        let joints: HashSet<_> = scopes
            .iter()
            .filter(|scope| {
                crate::ids::native_stream(&scope.id) == Some(stream.as_str())
                    && compact_placement_as_built_scope(scope)
                    && scope
                        .assembly_alignment()
                        .is_some_and(|alignment| alignment.operand_path_refs().is_some())
            })
            .filter_map(|scope| scope.record_index.checked_sub(1))
            .collect();
        let as_built = AsBuiltJoints {
            types: if joints.is_empty() {
                HashMap::new()
            } else {
                super::meta::stream_types_by_entity(types, &entry.name)
            },
            joints,
        };
        let records = scan.design_record_offsets(&entry.name)?;
        out.extend(decode_stream(
            bytes,
            &entry.name,
            &records,
            &assemble_scopes,
            &as_built,
        ));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// The motions of one stream. `assemble_scopes` names the decoded scopes of
/// the stream and whether each is an `Assemble` scope.
fn decode_stream(
    bytes: &[u8],
    entry_name: &str,
    records: &IndexedRecordOffsets,
    assemble_scopes: &HashMap<u32, bool>,
    as_built: &AsBuiltJoints<'_>,
) -> Vec<DesignJointMotion> {
    // A record ends at the next indexed header of any record.
    let record_end = |start: usize| records.next_offset(start + 1).unwrap_or(bytes.len());
    // Freedom entries grouped by the record that holds them.
    let mut by_record: Vec<(usize, u32, FreedomEntries)> = Vec::new();
    let mut starts = memchr::memmem::find_iter(bytes, &FREEDOM_ENTRY_PREFIX)
        .map(|at| (at, false))
        .chain(
            memchr::memmem::find_iter(bytes, &ZERO_LANE_FREEDOM_ENTRY_PREFIX).map(|at| (at, true)),
        )
        .collect::<Vec<_>>();
    starts.sort_unstable();
    for (at, zero_second_value) in starts {
        let word_at = at + FREEDOM_ENTRY_PREFIX.len();
        let code_at = word_at + 4;
        let (Some(word), Some(code)) = (
            View::u32_le_at(bytes, word_at),
            View::u32_le_at(bytes, code_at),
        ) else {
            continue;
        };
        if !FREEDOM_ENTRY_WORDS.contains(&word) || (zero_second_value && word != 1) {
            continue;
        }
        let value = match word {
            1 => match crate::bytes::f64s_at::<1>(bytes, code_at + 4) {
                Some([value]) if value.is_finite() => Some(value),
                _ => continue,
            },
            _ => None,
        };
        let headers = records.headers();
        let position = headers.partition_point(|header| header.offset <= at);
        let Some(header) = position.checked_sub(1).and_then(|i| headers.get(i)) else {
            continue;
        };
        let (record_start, record_index) = (header.offset, header.record_index);
        let located = Located {
            value: code,
            offset: u64_from_index(code_at),
        };
        let entry = (
            located,
            DesignJointFreedomWord {
                word,
                zero_second_value,
                value,
            },
        );
        match by_record.last_mut() {
            Some((start, _, entries)) if *start == record_start => entries.push(entry),
            _ => by_record.push((record_start, record_index, vec![entry])),
        }
    }
    let mut out = Vec::new();
    for (motion_start, motion_index, mut entries) in by_record {
        // A record with π entries stores no zero-lane entry.
        if entries.iter().any(|(_, entry)| !entry.zero_second_value) {
            entries.retain(|(_, entry)| !entry.zero_second_value);
        }
        let motion = MotionRecord {
            start: motion_start,
            end: record_end(motion_start),
            index: motion_index,
        };
        let Some(joint_index) =
            as_built_motion_joint(bytes, &motion, &entries, as_built).or_else(|| {
                motion_joint(bytes, records, &record_end, assemble_scopes, &motion)
                    .filter(|joint| !as_built.joints.contains(joint))
            })
        else {
            continue;
        };
        let Some(&joint_start) = records.offsets(joint_index).first() else {
            continue;
        };
        let (freedoms, words): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        let usual = words.iter().all(|entry| entry.word == 3);
        out.push(DesignJointMotion {
            id: crate::ids::native_scoped_id(entry_name, "design-joint-motion", motion_start),
            joint_record_index: joint_index,
            joint_byte_offset: u64_from_index(joint_start),
            motion_record_index: motion_index,
            motion_byte_offset: u64_from_index(motion_start),
            freedoms,
            entry_words: if usual { Vec::new() } else { words },
            limits: joint_limits(bytes, joint_start, record_end(joint_start), joint_index),
        });
    }
    out
}

/// A record holding freedom entries: its header offset, end, and index.
struct MotionRecord {
    start: usize,
    end: usize,
    index: u32,
}

fn as_built_motion_joint(
    bytes: &[u8],
    motion: &MotionRecord,
    freedoms: &FreedomEntries,
    as_built: &AsBuiltJoints<'_>,
) -> Option<u32> {
    let is_type = |index, guid: &str, version| {
        as_built
            .types
            .get(&u64::from(index))
            .is_some_and(|(actual, v)| *v == version && actual.eq_ignore_ascii_case(guid))
    };
    let &(guid, version) = as_built.types.get(&u64::from(motion.index))?;
    if !guid.eq_ignore_ascii_case(MOTION_TYPE_GUID) || !matches!(version, 2 | 3) {
        return None;
    }
    let record = bytes.get(motion.start..motion.end)?;
    let count = bounded_len(
        u64::from(View::u32_le_at(record, parents::COUNT)?),
        parent_entry::LEN,
        record.len().checked_sub(parents::FIRST_REFERENCE)?,
    )?;
    if !(1..=3).contains(&count) {
        return None;
    }
    let run_end = parents::FIRST_REFERENCE + count * parent_entry::LEN;
    let minimum_code = u64_from_index(motion.start + run_end + FREEDOM_ENTRY_PREFIX.len() + 4);
    if freedoms.iter().any(|(code, entry)| {
        code.offset < minimum_code
            || code.offset + if entry.value.is_some() { 12 } else { 4 } > u64_from_index(motion.end)
    }) {
        return None;
    }
    let mut entries = record
        .get(parents::FIRST_REFERENCE..run_end)?
        .chunks_exact(parent_entry::LEN);
    let joint = marked_record_reference(entries.next_back()?, parent_entry::REFERENCE)?;
    (as_built.joints.contains(&joint)
        && is_type(joint, JOINT_TYPE_GUID, version + 2)
        && entries.all(|entry| {
            marked_record_reference(entry, parent_entry::REFERENCE)
                .is_some_and(|parent| is_type(parent, OCCURRENCE_TYPE_GUID, 2))
        }))
    .then_some(joint)
}

/// The joint record a motion record belongs to.
///
/// Current generation: the marked reference at offset 24 names joint record
/// `J`, and record `J + 1` references `J`; when that record is a decoded scope
/// it is an `Assemble` scope. Older generation: the motion record is `J + 1`
/// and references `J`.
fn motion_joint(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_end: &dyn Fn(usize) -> usize,
    assemble_scopes: &HashMap<u32, bool>,
    motion: &MotionRecord,
) -> Option<u32> {
    let references = |index: u32, target: u32| {
        records
            .offsets(index)
            .first()
            .is_some_and(|&start| contains_reference(bytes, start, record_end(start), target))
    };
    if let Some(joint) = marked_record_reference(bytes, motion.start + MOTION_JOINT_REFERENCE) {
        let scope = joint.checked_add(1)?;
        if joint != motion.index
            && assemble_scopes.get(&scope).copied().unwrap_or(true)
            && !records.offsets(joint).is_empty()
            && references(scope, joint)
        {
            return Some(joint);
        }
    }
    let joint = motion.index.checked_sub(1)?;
    (!records.offsets(joint).is_empty()
        && !assemble_scopes.contains_key(&motion.index)
        && contains_reference(bytes, motion.start, motion.end, joint))
    .then_some(joint)
}

/// Whether the bytes in `start..end` (after the eleven-byte header) hold a
/// marked same-segment reference to `target`.
fn contains_reference(bytes: &[u8], start: usize, end: usize, target: u32) -> bool {
    let mut needle = [0u8; 5];
    needle[0] = 1;
    needle[1..].copy_from_slice(&target.to_le_bytes());
    let Some(window) = bytes.get(start + 11..end) else {
        return false;
    };
    memchr::memmem::find_iter(window, &needle)
        .any(|at| marked_record_reference(window, at) == Some(target))
}

/// The limit lanes after the joint record's first self-reference.
fn joint_limits(
    bytes: &[u8],
    joint_start: usize,
    joint_end: usize,
    joint_index: u32,
) -> Option<DesignJointLimitLanes> {
    let bytes = bytes.get(..joint_end)?;
    let window = bytes.get(joint_start + 11..)?;
    let mut needle = [0u8; 5];
    needle[0] = 1;
    needle[1..].copy_from_slice(&joint_index.to_le_bytes());
    let marker = memchr::memmem::find_iter(window, &needle)
        .find(|&at| marked_record_reference(window, at) == Some(joint_index))?;
    let marker = joint_start + 11 + marker;
    let flags_at = marker + LIMIT_FLAGS_AFTER_SELF_REFERENCE;
    let mut fields = View::over_retained(bytes);
    fields.seek(flags_at)?;
    let (first_flag, second_flag) = (fields.u8()?, fields.u8()?);
    if first_flag > 1 || second_flag > 1 {
        return None;
    }
    let values_at = marker + LIMIT_VALUES_AFTER_SELF_REFERENCE;
    let [maximum, minimum] = crate::bytes::f64s_at::<2>(bytes, values_at)?;
    if !maximum.is_finite() || !minimum.is_finite() || values_at + 16 > joint_end {
        return None;
    }
    let rest_value_at = marker + REST_VALUE_AFTER_SELF_REFERENCE;
    let stored_rest = fields
        .seek(marker + REST_FLAG_AFTER_SELF_REFERENCE)
        .and_then(|()| fields.u8())
        .zip(View::f64_le_at(bytes, rest_value_at))
        .filter(|(flag, value)| *flag <= 1 && value.is_finite());
    let rest_malformed = stored_rest.is_none();
    let rest = stored_rest.and_then(|(flag, value)| {
        (flag != 0 || value.to_bits() != 0).then_some(DesignJointRest { flag, value })
    });
    Some(DesignJointLimitLanes {
        flags_offset: u64_from_index(flags_at),
        first_flag,
        second_flag,
        maximum,
        minimum,
        values_offset: u64_from_index(values_at),
        rest,
        rest_malformed,
    })
}

#[cfg(test)]
mod tests;
