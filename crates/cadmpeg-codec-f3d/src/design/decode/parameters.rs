// SPDX-License-Identifier: Apache-2.0
//! Parse Design parameter, owner, and companion frames.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::design::decode::body::decode_stream;
use crate::design::decode::dimension_frames::companion_owned_interval;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::ids::{self, native_stream};
use crate::records::{
    ConstructionRecipe, DesignParameter, DesignParameterCompanion, DesignParameterKind,
    DesignParameterOwner, DesignParameterScope, DesignRecordHeader,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{f64_at, u32_at, u64_at as read_u64};
use std::collections::HashMap;

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub fn decode_recipes(scan: &ContainerScan) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM
            && entry.name.contains("Design")
            && scan
                .asset_folder
                .as_ref()
                .is_none_or(|folder| entry.name.starts_with(&format!("{folder}/")))
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(bytes, &entry.name, &mut out);
    }
    Ok(out)
}

/// Decode every indexed parameter record in each Design `BulkStream`.
pub fn decode_parameters(scan: &ContainerScan) -> Result<Vec<DesignParameter>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while let Some(at) = next_indexed_record_offset(bytes, position) {
            let end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            if let Some(mut parameter) = parse_design_parameter(&bytes[at..end]) {
                parameter.id = ids::native_design_parameter_id(&entry.name, at);
                parameter.byte_offset = at as u64;
                parameter.prefix_value_offset += at as u64;
                parameter.expression_offset += at as u64;
                parameter.source_kind_offset += at as u64;
                parameter.unit_offset = parameter.unit_offset.map(|offset| offset + at as u64);
                parameter.name_offset += at as u64;
                parameter.evaluated_value_offset += at as u64;
                out.push(parameter);
                position = end;
            } else {
                position = at + 1;
            }
        }
    }
    out.sort_by_key(|parameter| parameter.id.clone());
    Ok(out)
}

pub(crate) fn parse_design_parameter(payload: &[u8]) -> Option<DesignParameter> {
    let (class_tag, after_tag) = lp_ascii_filtered(payload, 0, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != 7
        || payload.get(11..22) != Some(&[0; 11])
        || payload.get(30) != Some(&0)
    {
        return None;
    }
    let record_index = u32_at(payload, 7)?;
    let prefix_value = read_u64(payload, 22)?;
    let source_ordinal = u32_at(payload, 31)?;
    let (owner_record_index, expression_at, expression_trailer) = match payload.get(35)? {
        0 => (None, 36, [0, 0, 0, 0, 0, 0, 0, 0, 1]),
        1 if payload.get(40..46) == Some(&[0; 6]) => (Some(u32_at(payload, 36)?), 46, [0; 9]),
        _ => return None,
    };
    let (expression, expression_end) = lp_utf16_bounded(payload, expression_at, 1..=256)?;
    if payload.get(expression_end..expression_end + 9) != Some(&expression_trailer) {
        return None;
    }
    let source_kind_at = if owner_record_index.is_some()
        && payload.get(expression_end..expression_end + 10) == Some(&[0; 10])
        && lp_utf16_bounded(payload, expression_end + 10, 1..=256).is_some()
    {
        expression_end + 10
    } else {
        expression_end + 9
    };
    let (source_kind, source_kind_end) = lp_utf16_bounded(payload, source_kind_at, 1..=256)?;
    if u32_at(payload, source_kind_end) != Some(0)
        || !valid_design_parameter_prefix(prefix_value, &source_kind)
    {
        return None;
    }
    let first_at = source_kind_end + 4;
    let (unit, unit_offset, name, name_at, name_end) = if u32_at(payload, first_at) == Some(0) {
        let name_at = first_at + 4;
        let (name, name_end) = lp_utf16_bounded(payload, name_at, 1..=256)?;
        (None, None, name, name_at, name_end)
    } else {
        let (first, first_end) = lp_utf16_bounded(payload, first_at, 1..=256)?;
        if let Some((second, second_end)) = lp_utf16_bounded(payload, first_end, 1..=256) {
            (
                Some(first),
                Some(first_at + 4),
                second,
                first_end,
                second_end,
            )
        } else {
            (None, None, first, first_at, first_end)
        }
    };
    let evaluated_value = f64_at(payload, name_end)?;
    let tail = payload.get(name_end + 8..)?;
    if tail.len() != 12
        || tail[0..2] != [0, 1]
        || tail[3..] != [0; 9]
        || !valid_design_parameter_family(prefix_value, &source_kind, tail[2])
        || expression.is_empty()
        || source_kind.is_empty()
        || name.is_empty()
        || !evaluated_value.is_finite()
    {
        return None;
    }
    let kind = if source_kind == "User Parameter" {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    };
    Some(DesignParameter {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        prefix_value,
        prefix_value_offset: 22,
        source_ordinal,
        owner_record_index,
        expression,
        expression_offset: (expression_at + 4) as u64,
        source_kind,
        source_kind_offset: (source_kind_at + 4) as u64,
        kind,
        unit,
        unit_offset: unit_offset.map(|offset| offset as u64),
        name,
        name_offset: (name_at + 4) as u64,
        evaluated_value,
        evaluated_value_offset: name_end as u64,
    })
}

pub(crate) fn design_parameter_prefix(source_kind: &str) -> u64 {
    if source_kind == "TangencyWeight" {
        6
    } else {
        0
    }
}

pub(crate) fn valid_design_parameter_prefix(prefix: u64, source_kind: &str) -> bool {
    prefix == 6 || (prefix == 0 && source_kind != "TangencyWeight")
}

fn valid_design_parameter_family(prefix: u64, source_kind: &str, tail: u8) -> bool {
    match tail {
        16 => prefix == 6,
        19 => prefix == design_parameter_prefix(source_kind),
        _ => false,
    }
}

/// Decode the fixed-width owner frame for every owned Design parameter.
pub fn decode_parameter_owners(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for parameter in parameters {
        let Some(owner_index) = parameter.owner_record_index else {
            continue;
        };
        let Some(scope) = native_stream(&parameter.id) else {
            continue;
        };
        let Some(header) = headers.get(&(scope, owner_index)) else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && parameter
                    .id
                    .starts_with(&ids::native_scope_prefix(&entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let owner = at.and_then(|at| {
            [104, 101].into_iter().find_map(|length| {
                at.checked_add(length)
                    .and_then(|end| bytes.get(at..end))
                    .and_then(parse_parameter_owner)
            })
        });
        let Some(mut owner) = owner else {
            continue;
        };
        owner.id = ids::native_design_parameter_owner_id(&entry.name, header.byte_offset);
        owner.byte_offset = header.byte_offset;
        owner.evaluated_value_offset += header.byte_offset;
        out.push(owner);
    }
    out.sort_by_key(|owner| owner.id.clone());
    Ok(out)
}

pub(crate) fn parse_parameter_owner(frame: &[u8]) -> Option<DesignParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != 7
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || frame.get(11..19) != Some(&[0; 8])
        || frame.get(19..24) != Some(&[1, 1, 0, 0, 0])
        || frame.get(24) != Some(&1)
        || frame.get(29..35) != Some(&[0; 6])
        || frame.get(39) != Some(&0)
    {
        return None;
    }
    let (
        evaluated_value,
        parameter_marker,
        parameter_index,
        parameter_tail,
        owned_ordinal,
        repeated_scope_marker,
        repeated_scope_index,
        repeated_scope_tail,
        variant_marker,
        variant,
        variant_tail,
        companion_marker,
        companion_index,
        companion_tail,
        final_scope_marker,
        final_scope_index,
        final_scope_tail,
    ) = match frame.len() {
        104 => (
            f64_at(frame, 40)?,
            48,
            49,
            53..59,
            59,
            67,
            68,
            72..78,
            78,
            79,
            80,
            81,
            82,
            86..93,
            93,
            94,
            98..104,
        ),
        101 if frame.get(40) == Some(&1) => (
            f64::from(u32_at(frame, 41)?),
            45,
            46,
            50..56,
            56,
            64,
            65,
            69..75,
            75,
            76,
            77,
            78,
            79,
            83..90,
            90,
            91,
            95..101,
        ),
        _ => return None,
    };
    if frame.get(parameter_marker) != Some(&1)
        || frame.get(parameter_tail) != Some(&[0; 6])
        || frame.get(owned_ordinal + 4..repeated_scope_marker) != Some(&[0; 4])
        || frame.get(repeated_scope_marker) != Some(&1)
        || frame.get(repeated_scope_tail) != Some(&[0; 6])
        || frame.get(variant_marker) != Some(&1)
        || frame.get(variant_tail) != Some(&0)
        || frame.get(companion_marker) != Some(&1)
        || frame.get(companion_tail) != Some(&[0; 7])
        || frame.get(final_scope_marker) != Some(&1)
        || frame.get(final_scope_tail) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = u32_at(frame, 7)?;
    let parameter_record_index = u32_at(frame, parameter_index)?;
    let companion_record_index = u32_at(frame, companion_index)?;
    let owner_first = parameter_record_index == record_index.checked_add(1)?
        && companion_record_index == record_index.checked_add(2)?;
    let parameter_first = record_index == parameter_record_index.checked_add(1)?
        && companion_record_index == record_index.checked_add(1)?;
    let scope_record_index = u32_at(frame, 25)?;
    if u32_at(frame, repeated_scope_index)? != scope_record_index
        || u32_at(frame, final_scope_index)? != scope_record_index
        || !(owner_first || parameter_first)
        || !evaluated_value.is_finite()
    {
        return None;
    }
    Some(DesignParameterOwner {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: u32_at(frame, 35)?,
        evaluated_value,
        evaluated_value_offset: if frame.len() == 104 { 40 } else { 41 },
        parameter_record_index,
        owned_ordinal: u32_at(frame, owned_ordinal)?,
        variant: *frame.get(variant)?,
        companion_record_index,
    })
}

/// Decode the fixed prefix of every indexed record paired with a parameter
/// owner. Record-specific payload after the prefix is decoded independently.
pub fn decode_parameter_companions(
    scan: &ContainerScan,
    owners: &[DesignParameterOwner],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let headers = headers
        .iter()
        .filter_map(|header| Some(((native_stream(&header.id)?, header.record_index), header)))
        .collect::<HashMap<_, _>>();
    let mut out = Vec::new();
    for owner in owners {
        let Some(scope) = native_stream(&owner.id) else {
            continue;
        };
        let Some(header) = headers.get(&(scope, owner.companion_record_index)) else {
            continue;
        };
        let entry = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && owner.id.starts_with(&ids::native_scope_prefix(&entry.name))
        });
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let prefix = at.and_then(|at| at.checked_add(58).and_then(|end| bytes.get(at..end)));
        let Some(mut companion) = prefix.and_then(parse_parameter_companion) else {
            continue;
        };
        if companion.record_index != owner.companion_record_index
            || companion.owner_record_index != owner.record_index
        {
            continue;
        }
        companion.id = ids::native_design_parameter_companion_id(&entry.name, header.byte_offset);
        companion.byte_offset = header.byte_offset;
        companion.timestamp_micros_offset += header.byte_offset;
        companion.payload_byte_offset += header.byte_offset;
        out.push(companion);
    }
    out.sort_by_key(|companion| companion.id.clone());
    Ok(out)
}

pub(crate) fn parse_parameter_companion(prefix: &[u8]) -> Option<DesignParameterCompanion> {
    let (class_tag, after_tag) = lp_ascii_filtered(prefix, 0, 0..=2000, u8::is_ascii_graphic)?;
    if prefix.len() != 58
        || after_tag != 7
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || prefix.get(11..31) != Some(&[0; 20])
        || prefix.get(31) != Some(&1)
        || prefix.get(36..42) != Some(&[0; 6])
        || prefix.get(50..58) != Some(&[0; 8])
    {
        return None;
    }
    let timestamp_micros = read_u64(prefix, 42)?;
    if timestamp_micros == 0 {
        return None;
    }
    Some(DesignParameterCompanion {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index: u32_at(prefix, 7)?,
        owner_record_index: u32_at(prefix, 32)?,
        timestamp_micros,
        timestamp_micros_offset: 42,
        payload_byte_offset: 58,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    })
}

/// Bind each companion to its exact owned byte interval and the construction
/// recipes nested in that interval.
pub fn bind_parameter_companion_payloads<S: std::hash::BuildHasher>(
    companions: &mut [DesignParameterCompanion],
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    recipes: &[ConstructionRecipe],
    stream_lengths: &HashMap<String, usize, S>,
) {
    for companion in companions {
        let Some(stream) = native_stream(&companion.id) else {
            continue;
        };
        let Some(stream_length) = stream_lengths.get(stream).copied() else {
            continue;
        };
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.iter(),
            owners,
            scopes,
            headers,
            stream_length,
        ) else {
            continue;
        };
        companion.payload_byte_offset = u64::try_from(start).unwrap_or(u64::MAX);
        companion.payload_byte_length = u64::try_from(end - start).unwrap_or(u64::MAX);
        let mut owned = recipes
            .iter()
            .filter(|recipe| {
                native_stream(&recipe.id) == Some(stream)
                    && usize::try_from(recipe.byte_offset)
                        .is_ok_and(|offset| offset >= start && offset < end)
            })
            .collect::<Vec<_>>();
        owned.sort_by_key(|recipe| recipe.byte_offset);
        companion.owned_recipe_ids = owned.into_iter().map(|recipe| recipe.id.clone()).collect();
    }
}
