// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand paths and their locator envelopes.

use super::assembly_path_layout::{ParsedOperandPath, PathLinkSources, PathSourceDraft};
use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_locator_reference_run as path_locator_run;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::layout::assembly_wide_operand_path_locator as wide_locator;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;

/// Type GUID of the operand-path locator of the 945-byte `Assemble` scope.
pub(super) const WIDE_PATH_LOCATOR_TYPE_GUID: &str = "2E3AC990-483D-4A55-8662-C064B976A957";
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Record types of one Design stream by entity id: type GUID and version.
pub(super) type StreamTypes<'a> = HashMap<u64, (&'a str, u32)>;

pub(super) fn exact_assembly_operand_paths<'a>(
    source: View<'a>,
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &StreamTypes<'_>,
) -> Result<Option<[ParsedOperandPath<'a>; 2]>, CodecError> {
    let bytes = source.window();
    (|| {
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let search_start = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(11)?;
        let locator_offsets = crate::design::assembly::AssemblyScopeGeneration::new(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        )
        .operand_path_locator_offsets()?;
        let count_at = scope_at.checked_add(
            locator_offsets[0].checked_sub(path_locator_run::FIRST_LOCATOR_REFERENCE)?,
        )?;
        // A rigid group counts one locator per member; every other form has two.
        let expected = if crate::design::assembly::rigid_as_built_scope(
            scope.frame_length(),
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::AsBuilt,
        ) {
            crate::design::assembly::rigid_group_member_count(scope.frame_length())?
        } else {
            2
        };
        if usize::try_from(View::u32_le_at(bytes, count_at)?).ok()? != expected {
            return None;
        }
        let mut paths = [None, None];
        for (ordinal, relative_offset) in locator_offsets.into_iter().enumerate() {
            let locator_reference_at = scope_at.checked_add(relative_offset)?;
            let (locator_record_index, locator_reference_offset) =
                exact_same_segment_record_reference(bytes, locator_reference_at)?;
            let candidates = records
                .offsets(locator_record_index)
                .iter()
                .copied()
                .filter(|locator_at| *locator_at >= search_start);
            let mut candidate = None;
            for locator_at in candidates {
                let parsed = match exact_assembly_operand_path_envelope(
                    source,
                    scope,
                    stream_types,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                ) {
                    Ok(parsed) => parsed,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(parsed) = parsed {
                    if candidate.replace(parsed).is_some() {
                        return None;
                    }
                }
            }
            paths[ordinal] = Some(candidate?);
        }
        let [Some(first), Some(second)] = paths else {
            return None;
        };
        let first_start = first.source.locator_start()?;
        let first_end =
            next_indexed_record_offset(bytes, first.source.wrapper_start()?.checked_add(1)?)?;
        let second_start = second.source.locator_start()?;
        let second_end =
            next_indexed_record_offset(bytes, second.source.wrapper_start()?.checked_add(1)?)?;
        if first.content.link().locator_record_index == second.content.link().locator_record_index
            || (first_start < second_end && second_start < first_end)
        {
            return None;
        }
        Some(Ok([first, second]))
    })()
    .transpose()
}

/// The operand paths of a rigid group's members after its first two, in
/// locator order. Each member's locator reference follows the previous one;
/// every locator is a distinct record whose envelope overlaps no other
/// member's.
pub(super) fn exact_rigid_group_member_paths<'a>(
    source: View<'a>,
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &StreamTypes<'_>,
    leading: &[ParsedOperandPath<'a>; 2],
) -> Result<Option<Vec<ParsedOperandPath<'a>>>, CodecError> {
    use crate::layout::assembly_rigid_as_built_scope as rigid;
    let bytes = source.window();
    (|| {
        let members = crate::design::assembly::rigid_group_member_count(scope.frame_length())?;
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let search_start = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(11)?;
        let mut paths = Vec::new();
        for member in 2..members {
            let locator_reference_at = scope_at
                .checked_add(rigid::FIRST_LOCATOR_REFERENCE)?
                .checked_add(member.checked_mul(11)?)?;
            let (locator_record_index, locator_reference_offset) =
                exact_same_segment_record_reference(bytes, locator_reference_at)?;
            let mut candidate = None;
            for locator_at in records
                .offsets(locator_record_index)
                .iter()
                .copied()
                .filter(|locator_at| *locator_at >= search_start)
            {
                let parsed = match exact_assembly_operand_path_envelope(
                    source,
                    scope,
                    stream_types,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                ) {
                    Ok(parsed) => parsed,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(parsed) = parsed {
                    if candidate.replace(parsed).is_some() {
                        return None;
                    }
                }
            }
            paths.push(candidate?);
        }
        let mut spans = Vec::new();
        for path in leading.iter().chain(&paths) {
            let start = path.source.locator_start()?;
            let end =
                next_indexed_record_offset(bytes, path.source.wrapper_start()?.checked_add(1)?)?;
            let index = path.content.link().locator_record_index;
            if spans.iter().any(|(other, other_start, other_end)| {
                *other == index || (start < *other_end && *other_start < end)
            }) {
                return None;
            }
            spans.push((index, start, end));
        }
        Some(Ok(paths))
    })()
    .transpose()
}

fn exact_assembly_operand_path_envelope<'a>(
    source: View<'a>,
    scope: &DesignParameterScope,
    stream_types: &StreamTypes<'_>,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Result<Option<ParsedOperandPath<'a>>, CodecError> {
    let bytes = source.window();
    (|| {
        let locator_class_tag = exact_indexed_header_at(bytes, locator_at, locator_record_index)?;
        let variable_reference = crate::design::assembly::variable_reference_assembly_generation(
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
        // The wide scope's locator: its own record type and prologue.
        let wide = (crate::design::assembly::AssemblyScopeGeneration::new(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        )
        .operand_frame_variant()
        .is_some_and(crate::design::assembly::AssemblyOperandFrameVariant::is_wide_family)
            || crate::design::assembly::wide_as_built_scope(
                scope.frame_length(),
                scope.kind() == crate::records::feature::scope::DesignFeatureKind::AsBuilt,
            ))
            && stream_types
                .get(&u64::from(locator_record_index))
                .is_some_and(|(guid, _)| guid.eq_ignore_ascii_case(WIDE_PATH_LOCATOR_TYPE_GUID));
        let (locator_length, scope_backlink, wrapper_reference, constant_two, zero_tail) = if wide {
            if bytes
                .get(
                    locator_at.checked_add(wide_locator::ZERO_RUN_28)?
                        ..locator_at.checked_add(wide_locator::CONSTANT_THREE)?,
                )?
                .iter()
                .any(|byte| *byte != 0)
                || View::u32_le_at(bytes, locator_at.checked_add(wide_locator::CONSTANT_THREE)?)?
                    != 3
                || bytes
                    .get(
                        locator_at.checked_add(wide_locator::ZERO_RUN_13)?
                            ..locator_at.checked_add(wide_locator::NONZERO_RECORD_REFERENCE)?,
                    )?
                    .iter()
                    .any(|byte| *byte != 0)
                || exact_same_segment_record_reference(
                    bytes,
                    locator_at.checked_add(wide_locator::NONZERO_RECORD_REFERENCE)?,
                )?
                .0 == 0
            {
                return None;
            }
            // The transform is present only when the identity flag is 0.
            let after_transform = match bytes
                .get(locator_at.checked_add(wide_locator::IDENTITY_FLAG)?)?
            {
                1 => wide_locator::TRANSFORM,
                0 => {
                    rigid_transform_at(bytes, locator_at.checked_add(wide_locator::TRANSFORM)?)?;
                    wide_locator::TRANSFORM + 16 * 8
                }
                _ => return None,
            };
            if bytes.get(locator_at.checked_add(after_transform)?) != Some(&0) {
                return None;
            }
            let backlink = after_transform + 1;
            (
                backlink + 28,
                backlink,
                backlink + 11,
                backlink + 22,
                backlink + 26,
            )
        } else if variable_reference {
            if locator_class_tag != "390"
                || bytes
                    .get(
                        locator_at.checked_add(variable_path_locator::TRANSFORM + 16 * 8)?
                            ..locator_at.checked_add(variable_path_locator::SCOPE_BACKLINK)?,
                    )?
                    .iter()
                    .any(|byte| *byte != 0)
                || rigid_transform_at(
                    bytes,
                    locator_at.checked_add(variable_path_locator::TRANSFORM)?,
                )
                .is_none()
            {
                return None;
            }
            (
                variable_path_locator::LEN,
                variable_path_locator::SCOPE_BACKLINK,
                variable_path_locator::WRAPPER_REFERENCE,
                variable_path_locator::CONSTANT_TWO,
                variable_path_locator::ZERO_TAIL,
            )
        } else {
            if bytes.get(
                locator_at.checked_add(path_locator::ZERO_RUN_10)?
                    ..locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
            )? != [0; 10]
                || exact_same_segment_record_reference(
                    bytes,
                    locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
                )?
                .0 == 0
                || bytes.get(locator_at.checked_add(path_locator::ZERO_32)?) != Some(&0)
                || rigid_transform_at(bytes, locator_at.checked_add(path_locator::TRANSFORM)?)
                    .is_none()
                || bytes.get(locator_at.checked_add(path_locator::ZERO_161)?) != Some(&0)
            {
                return None;
            }
            (
                path_locator::LEN,
                path_locator::SCOPE_BACKLINK,
                path_locator::WRAPPER_REFERENCE,
                path_locator::CONSTANT_TWO,
                path_locator::ZERO_TAIL_2,
            )
        };
        let (scope_record_index, locator_scope_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_at.checked_add(scope_backlink)?)?;
        let (wrapper_record_index, wrapper_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_at.checked_add(wrapper_reference)?)?;
        let path_record_index = locator_record_index.checked_add(1)?;
        // A variable-reference or wide locator's path can continue over
        // further path records before its wrapper.
        let counted_wrapper = variable_reference || wide;
        if scope_record_index != scope.record_index
            || if counted_wrapper {
                !(locator_record_index.checked_add(2)?..=locator_record_index.checked_add(65)?)
                    .contains(&wrapper_record_index)
            } else {
                wrapper_record_index != locator_record_index.checked_add(2)?
            }
            || View::u32_le_at(bytes, locator_at.checked_add(constant_two)?)? != 2
            || bytes
                .get(locator_at.checked_add(zero_tail)?..locator_at.checked_add(locator_length)?)?
                != [0; 2]
        {
            return None;
        }
        let path_at = locator_at.checked_add(locator_length)?;
        if next_indexed_record_offset(bytes, locator_at.checked_add(1)?)? != path_at {
            return None;
        }
        if wrapper_record_index < path_record_index {
            return None;
        }

        let mut path_spans = Vec::new();
        let mut record_index = path_record_index;
        let mut record_at = path_at;
        let wrapper_at = loop {
            if record_index == wrapper_record_index {
                break record_at;
            }
            let next = next_indexed_record_offset(bytes, record_at.checked_add(1)?)?;
            path_spans.push((record_index, record_at, next));
            record_index = record_index.checked_add(1)?;
            record_at = next;
        };
        let wrapper_class_tag = exact_indexed_header_at(bytes, wrapper_at, wrapper_record_index)?;
        let wrapper_end = next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)?;
        let expected_wrapper_length = if counted_wrapper {
            path_wrapper::LEN.checked_add(path_spans.len().checked_sub(1)?.checked_mul(11)?)?
        } else {
            path_wrapper::LEN
        };
        if variable_reference && wrapper_class_tag != "397"
            || bytes.get(
                wrapper_at.checked_add(path_wrapper::ZERO_RUN_10)?
                    ..wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?,
            )? != [0; 10]
            || bytes.get(wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?) != Some(&1)
            || View::u32_le_at(
                bytes,
                wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_WORD)?,
            )? != if counted_wrapper {
                u32::try_from(path_spans.len()).ok()?
            } else {
                1
            }
            || wrapper_end != wrapper_at.checked_add(expected_wrapper_length)?
        {
            return None;
        }
        let (referenced_path_record_index, path_reference_offset) =
            exact_same_segment_record_reference(
                bytes,
                wrapper_at.checked_add(path_wrapper::PATH_REFERENCE)?,
            )?;
        if referenced_path_record_index != path_record_index {
            return None;
        }
        if counted_wrapper
            && path_spans
                .iter()
                .skip(1)
                .enumerate()
                .any(|(ordinal, (record_index, _, _))| {
                    exact_same_segment_record_reference(
                        bytes,
                        wrapper_at + path_wrapper::LEN + ordinal * 11,
                    )
                    .map(|reference| reference.0)
                        != Some(*record_index)
                })
        {
            return None;
        }
        let link_sources = PathLinkSources::new(
            source,
            locator_reference_offset,
            locator_at,
            locator_scope_reference_offset,
            wrapper_reference_offset,
            wrapper_at,
            path_reference_offset,
        )?;
        let link = DesignAssemblyOperandPathLink {
            locator_record_index,
            locator_class_tag: locator_class_tag.try_into().ok()?,
            wrapper_record_index,
            wrapper_class_tag: wrapper_class_tag.try_into().ok()?,
        };
        let mut paths = path_spans.into_iter().map(|(record_index, start, limit)| {
            let type_guid = stream_types
                .get(&u64::from(record_index))
                .map(|(guid, _)| *guid)
                .filter(|guid| guid.eq_ignore_ascii_case(TYPED_OCCURRENCE_PATH_TYPE_GUID));
            exact_assembly_operand_path(source, start, record_index, limit, link.clone(), type_guid)
        });
        let mut path = match paths.next()? {
            Ok(Some(path)) => path,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if variable_reference && path.content.class_tag().as_str() != "330" {
            return None;
        }
        for continuation in paths {
            let continuation = match continuation {
                Ok(Some(path)) => path,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            let continues = if variable_reference {
                continuation.content.class_tag().as_str() == "330"
            } else {
                wide && path.content.admitted_by_type() && continuation.content.admitted_by_type()
            };
            if !continues {
                return None;
            }
            path.content = path.content.try_append(continuation.content)?;
            path.source.append(continuation.source)?;
        }
        path.source.set_link(link_sources);
        Some(Ok(path))
    })()
    .transpose()
}

/// Read one path record. `type_guid` is the record's type GUID when it is the
/// identity-qualified counted path type, which admits it whatever its class.
fn exact_assembly_operand_path<'a>(
    source: View<'a>,
    start: usize,
    record_index: u32,
    limit: usize,
    link: DesignAssemblyOperandPathLink,
    type_guid: Option<&str>,
) -> Result<Option<ParsedOperandPath<'a>>, CodecError> {
    let bytes = source.window();
    (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 1..=8, u8::is_ascii_digit)?;
        if View::u64_le_at(bytes, after_tag)? != u64::from(record_index) {
            return None;
        }
        let mut draft = PathSourceDraft::new(source, start)?;
        let mut occurrence_guids = Vec::new();
        let mut identity_guids = Vec::new();
        let mut paired_occurrence_keys = Vec::new();
        match class_tag {
            "294" | "299" | "307" => {
                let end = next_indexed_record_offset(bytes, start + 1)?;
                if end != limit
                    || bytes.get(after_tag + 8..after_tag + 14)? != [0; 6]
                    || bytes.get(after_tag + 14) != Some(&1)
                    || bytes.get(after_tag + 15..after_tag + 18)? != [0; 3]
                {
                    return None;
                }
                let mut position = after_tag + 18;
                let (occurrence, after_occurrence) =
                    match fixed_relaxed_guid_text(bytes.get(..end)?, position) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                draft.occurrence(position, after_occurrence)?;
                occurrence_guids.push(occurrence.into());
                position = after_occurrence;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(bytes.get(..end)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    draft.identity(position, after_guid)?;
                    identity_guids.push(guid);
                    position = after_guid;
                }
                if View::u64_le_at(bytes, position)? != 2 {
                    return None;
                }
                position += 8;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(bytes.get(..end)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    draft.identity(position, after_guid)?;
                    identity_guids.push(guid);
                    position = after_guid;
                }
                if View::u32_le_at(bytes, position)? != 2
                    || !bytes.get(position + 4..end)?.iter().all(|byte| *byte == 0)
                {
                    return None;
                }
            }
            tag if type_guid.is_some() || matches!(tag, "329" | "330" | "386" | "390") => {
                if bytes.get(after_tag + 8..after_tag + 14)? != [0; 6] {
                    return None;
                }
                let count = usize::try_from(View::u32_le_at(bytes, after_tag + 14)?).ok()?;
                if !(1..=64).contains(&count) {
                    return None;
                }

                let mut position = after_tag + 18;
                for _ in 0..count {
                    let (guid, after_guid) = match crate::design::decode::text::occurrence_key_text(
                        bytes.get(..limit)?,
                        position,
                    ) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    draft.occurrence(position, after_guid)?;
                    occurrence_guids.push(guid);
                    position = after_guid;
                }
                if position == limit {
                    if !matches!(class_tag, "329" | "330") {
                        return None;
                    }
                } else {
                    for _ in 0..2 {
                        let (guid, after_guid) =
                            match fixed_relaxed_guid_text(bytes.get(..limit)?, position) {
                                Ok(Some(value)) => value,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                        draft.identity(position, after_guid)?;
                        identity_guids.push(guid);
                        position = after_guid;
                    }
                    if View::u64_le_at(bytes, position)? != 2 {
                        return None;
                    }
                    position += 8;
                    for _ in 0..2 {
                        let (guid, after_guid) =
                            match fixed_relaxed_guid_text(bytes.get(..limit)?, position) {
                                Ok(Some(value)) => value,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                        draft.identity(position, after_guid)?;
                        identity_guids.push(guid);
                        position = after_guid;
                    }
                    if View::u32_le_at(bytes, position)? != 2 {
                        return None;
                    }
                    position += 4;
                    // A typed path can end in a zero word, a pair count, and
                    // per pair the u32 2 and two occurrence identities; a zero
                    // tail has no pairs, and only a typed path has pairs.
                    if !bytes.get(position..limit)?.iter().all(|byte| *byte == 0) {
                        if type_guid.is_none() || View::u32_le_at(bytes, position)? != 0 {
                            return None;
                        }
                        let pairs = usize::try_from(View::u32_le_at(bytes, position + 4)?).ok()?;
                        if !(1..=64).contains(&pairs) {
                            return None;
                        }
                        position += 8;
                        for _ in 0..pairs {
                            if View::u32_le_at(bytes, position)? != 2 {
                                return None;
                            }
                            position += 4;
                            for _ in 0..2 {
                                let (key, after_key) =
                                    match crate::design::decode::text::occurrence_key_text(
                                        bytes.get(..limit)?,
                                        position,
                                    ) {
                                        Ok(Some(value)) => value,
                                        Ok(None) => return None,
                                        Err(error) => return Some(Err(error)),
                                    };
                                draft.pair(position, after_key)?;
                                paired_occurrence_keys.push(key);
                                position = after_key;
                            }
                        }
                        if !bytes.get(position..limit)?.iter().all(|byte| *byte == 0) {
                            return None;
                        }
                    }
                }
            }
            _ => return None,
        }
        let type_guid = match type_guid {
            Some(guid) => Some(guid.to_owned().try_into().ok()?),
            None => None,
        };
        draft.validate_positions()?;
        DesignAssemblyOperandPath::try_new_typed(
            link,
            record_index,
            crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            type_guid,
            occurrence_guids,
            identity_guids,
        )
        .and_then(|path| path.with_paired_occurrence_keys(paired_occurrence_keys))
        .ok()
        .map(|content| {
            Ok(ParsedOperandPath {
                content,
                source: draft,
            })
        })
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
