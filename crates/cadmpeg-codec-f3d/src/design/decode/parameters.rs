// SPDX-License-Identifier: Apache-2.0
//! Parse Design parameter, owner, and companion frames.

use crate::bytes::{lp_ascii_filtered, lp_utf16_bounded};
use crate::container::{role, ContainerScan};
use crate::design::decode::body::decode_stream;
use crate::design::decode::dimension_frames::companion_owned_interval;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::ids::{self, native_stream};
use crate::layout::design_parameter_legacy_287_prefix as legacy_287;
use crate::layout::design_parameter_legacy_287_tail as legacy_287_tail;
use crate::layout::design_parameter_owner_legacy_68 as legacy_owner_68;
use crate::layout::design_parameter_owner_legacy_88 as legacy_owner_88;
use crate::layout::design_parameter_owner_prefix as owner_prefix;
use crate::layout::indexed_companion_record_prefix as companion_prefix;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::records::{
    ConstructionRecipe, DesignEntityHeader, DesignParameter, DesignParameterCompanion,
    DesignParameterKind, DesignParameterOwner, DesignParameterScope, DesignRecordHeader,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub fn decode_recipes(scan: &ContainerScan) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
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
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        let mut emitted_record_indices = HashSet::new();
        while let Some(at) = next_indexed_record_offset(bytes, position) {
            let end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            if let Some(mut parameter) = parse_design_parameter(&bytes[at..end]) {
                // The Design primary index exposes one live header for each
                // logical record index. Keep the first serialized parameter
                // frame so stale copies cannot create duplicate owner
                // bindings or duplicate neutral parameter identities.
                if !emitted_record_indices.insert(parameter.record_index) {
                    position = end;
                    continue;
                }
                parameter.id = ids::native_design_parameter_id(&entry.name, at);
                parameter.byte_offset = at as u64;
                parameter.family_discriminator_offset = parameter
                    .family_discriminator_offset
                    .map(|offset| offset + at as u64);
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
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(crate) fn parse_design_parameter(payload: &[u8]) -> Option<DesignParameter> {
    let (class_tag, after_tag) = lp_ascii_filtered(payload, 0, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != 7
        || payload.get(11..22) != Some(&[0; 11])
    {
        return None;
    }
    let record_index = View::u32_le_at(payload, 7)?;
    if class_tag == "287" {
        return parse_legacy_287_design_parameter(payload, class_tag, record_index);
    }
    let compact_owned = payload.get(11..26) == Some(&[0; 15])
        && payload.get(30) == Some(&1)
        && payload.get(35..41) == Some(&[0; 6]);
    let discriminated = !compact_owned && payload.get(30) == Some(&0);
    if !compact_owned && !discriminated {
        return parse_legacy_design_parameter(payload, class_tag, record_index);
    }
    let (family_discriminator, source_ordinal, owner_record_index, expression_at, trailer_len) =
        if discriminated {
            let discriminator = View::u64_le_at(payload, 22)?;
            let owner = match payload.get(35)? {
                0 => (None, 36, 9),
                1 if payload.get(40..46) == Some(&[0; 6]) => {
                    (Some(View::u32_le_at(payload, 36)?), 46, 9)
                }
                _ => return None,
            };
            (
                Some(discriminator),
                View::u32_le_at(payload, 31)?,
                owner.0,
                owner.1,
                owner.2,
            )
        } else if compact_owned {
            (
                None,
                View::u32_le_at(payload, 26)?,
                Some(View::u32_le_at(payload, 31)?),
                41,
                5,
            )
        } else {
            return None;
        };
    let (expression, expression_end) = lp_utf16_bounded(payload, expression_at, 1..=256)?;
    let expression_trailer = payload.get(expression_end..expression_end + trailer_len)?;
    let valid_expression_trailer = if discriminated && owner_record_index.is_none() {
        expression_trailer == [0, 0, 0, 0, 0, 0, 0, 0, 1]
    } else {
        expression_trailer.iter().all(|byte| *byte == 0)
    };
    if !valid_expression_trailer {
        return None;
    }
    let source_kind_at = if discriminated
        && owner_record_index.is_some()
        && payload.get(expression_end..expression_end + 10) == Some(&[0; 10])
        && lp_utf16_bounded(payload, expression_end + 10, 1..=256).is_some()
    {
        expression_end + 10
    } else {
        expression_end + trailer_len
    };
    let (source_kind, source_kind_end) = lp_utf16_bounded(payload, source_kind_at, 1..=256)?;
    if family_discriminator.is_some_and(|value| !valid_design_parameter_discriminator(value)) {
        return None;
    }
    let first_at = source_kind_end + usize::from(discriminated) * 4;
    if discriminated && View::u32_le_at(payload, source_kind_end) != Some(0) {
        return None;
    }
    let (unit, unit_offset, name, name_at, name_end) =
        if View::u32_le_at(payload, first_at) == Some(0) {
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
    let evaluated_value = View::f64_le_at(payload, name_end)?;
    let tail = payload.get(name_end + 8..)?;
    if tail.len() != 12
        || tail[0..2] != [0, 1]
        || tail[3..].iter().any(|byte| *byte != 0)
        || !valid_design_parameter_family(family_discriminator, &source_kind, tail[2])
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
        family_discriminator,
        family_discriminator_offset: family_discriminator.map(|_| 22),
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

/// Parse the class-287 owned parameter family.
///
/// This family uses the compact-owned prefix and a class-specific `0xAF` tail.
/// Its expression is followed by one of the two fixed five-byte trailers.
fn parse_legacy_287_design_parameter(
    payload: &[u8],
    class_tag: String,
    record_index: u32,
) -> Option<DesignParameter> {
    if payload.get(legacy_287::ZERO_RUN_15..legacy_287::SOURCE_ORDINAL) != Some(&[0; 15])
        || payload.get(legacy_287::OWNER_MARKER) != Some(&legacy_287::OWNER_MARKER_VALUE)
        || payload.get(legacy_287::ZERO_RUN_6..legacy_287::EXPRESSION_LENGTH) != Some(&[0; 6])
    {
        return None;
    }
    let source_ordinal = View::u32_le_at(payload, legacy_287::SOURCE_ORDINAL)?;
    let owner_record_index = View::u32_le_at(payload, legacy_287::OWNER_RECORD_INDEX)?;
    let (expression, expression_end) =
        lp_utf16_bounded(payload, legacy_287::EXPRESSION_LENGTH, 1..=256)?;
    let expression_trailer_end = expression_end.checked_add(CLASS_287_EXPRESSION_TRAILER_LEN)?;
    let expression_trailer = payload.get(expression_end..expression_trailer_end)?;
    if !matches!(expression_trailer, [0, 0, 0, 0 | 1, 0]) {
        return None;
    }
    let source_kind_at = expression_trailer_end;
    let (source_kind, source_kind_end) = lp_utf16_bounded(payload, source_kind_at, 1..=256)?;
    let (unit, unit_offset, name, name_at, name_end) =
        if View::u32_le_at(payload, source_kind_end) == Some(0) {
            let name_at = source_kind_end.checked_add(4)?;
            let (name, name_end) = lp_utf16_bounded(payload, name_at, 1..=256)?;
            (None, None, name, name_at, name_end)
        } else {
            let (unit, unit_end) = lp_utf16_bounded(payload, source_kind_end, 1..=64)?;
            let (name, name_end) = lp_utf16_bounded(payload, unit_end, 1..=256)?;
            let unit_offset = source_kind_end.checked_add(4)?;
            (
                Some(unit),
                Some(u64::try_from(unit_offset).ok()?),
                name,
                unit_end,
                name_end,
            )
        };
    let evaluated_value = View::f64_le_at(payload, name_end)?;
    let tail_start = name_end.checked_add(8)?;
    let tail = payload.get(tail_start..)?;
    if tail.len() != legacy_287_tail::LEN
        || tail[..2] != legacy_287_tail::TAIL_PREFIX_VALUE
        || tail[legacy_287_tail::FAMILY_MARKER] != legacy_287_tail::FAMILY_MARKER_VALUE
        || tail[legacy_287_tail::ZERO_RUN_9..]
            .iter()
            .any(|byte| *byte != 0)
        || expression.is_empty()
        || source_kind.is_empty()
        || name.is_empty()
        || !evaluated_value.is_finite()
    {
        return None;
    }
    let kind = design_parameter_kind(&source_kind);
    Some(DesignParameter {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        family_discriminator: None,
        family_discriminator_offset: None,
        source_ordinal,
        owner_record_index: Some(owner_record_index),
        expression,
        expression_offset: u64::try_from(legacy_287::EXPRESSION_LENGTH + 4).ok()?,
        source_kind,
        source_kind_offset: u64::try_from(source_kind_at.checked_add(4)?).ok()?,
        kind,
        unit,
        unit_offset,
        name,
        name_offset: u64::try_from(name_at.checked_add(4)?).ok()?,
        evaluated_value,
        evaluated_value_offset: u64::try_from(name_end).ok()?,
    })
}

const CLASS_287_EXPRESSION_TRAILER_LEN: usize = 5;

fn parse_legacy_design_parameter(
    payload: &[u8],
    class_tag: String,
    record_index: u32,
) -> Option<DesignParameter> {
    if payload.get(11..25)? != [0; 14]
        || payload.get(29) != Some(&1)
        || payload.get(34..40)? != [0; 6]
    {
        return None;
    }
    let source_ordinal = View::u32_le_at(payload, 25)?;
    let owner_record_index = View::u32_le_at(payload, 30)?;
    let expression_at = 40;
    let (expression, expression_end) = lp_utf16_bounded(payload, expression_at, 1..=256)?;
    if payload.get(expression_end..expression_end + 5)? != [0; 5] {
        return None;
    }
    let source_kind_at = expression_end + 5;
    let (source_kind, source_kind_end) = lp_utf16_bounded(payload, source_kind_at, 1..=256)?;
    let unit_at = source_kind_end;
    let (unit, unit_end) = lp_utf16_bounded(payload, unit_at, 1..=64)?;
    let name_at = unit_end;
    let (name, name_end) = lp_utf16_bounded(payload, name_at, 1..=256)?;
    let evaluated_value = View::f64_le_at(payload, name_end)?;
    let tail = payload.get(name_end + 8..)?;
    if tail != [0, 1, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        || expression.is_empty()
        || source_kind.is_empty()
        || unit.is_empty()
        || name.is_empty()
        || !evaluated_value.is_finite()
    {
        return None;
    }
    let kind = design_parameter_kind(&source_kind);
    Some(DesignParameter {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index,
        family_discriminator: None,
        family_discriminator_offset: None,
        source_ordinal,
        owner_record_index: Some(owner_record_index),
        expression,
        expression_offset: (expression_at + 4) as u64,
        source_kind,
        source_kind_offset: (source_kind_at + 4) as u64,
        kind,
        unit: Some(unit),
        unit_offset: Some((unit_at + 4) as u64),
        name,
        name_offset: (name_at + 4) as u64,
        evaluated_value,
        evaluated_value_offset: name_end as u64,
    })
}

fn design_parameter_kind(source_kind: &str) -> DesignParameterKind {
    if source_kind == "User Parameter" {
        DesignParameterKind::User
    } else if source_kind.contains("Dimension") {
        DesignParameterKind::Dimension
    } else {
        DesignParameterKind::Feature
    }
}

pub(crate) fn design_parameter_discriminator(source_kind: &str) -> u64 {
    match source_kind {
        "ScaleFactor" => 5,
        "TangencyWeight" => 6,
        _ => 0,
    }
}

pub(crate) fn valid_design_parameter_discriminator(value: u64) -> bool {
    matches!(value, 0 | 3 | 4 | 5 | 6)
}

/// Whether a class tag admits the legacy owner grammar without scope or scalar
/// lanes.
pub(crate) fn is_legacy_parameter_owner_68_class(class_tag: &str) -> bool {
    matches!(
        class_tag,
        "268" | "282" | "284" | "289" | "297" | "299" | "325" | "336"
    )
}

/// Whether a class tag admits the legacy owner grammar with repeated scope
/// references but without scalar or local-ordinal lanes.
pub(crate) fn is_legacy_parameter_owner_88_class(class_tag: &str) -> bool {
    matches!(class_tag, "284" | "282" | "336" | "325" | "297")
}

fn valid_design_parameter_family(discriminator: Option<u64>, source_kind: &str, tail: u8) -> bool {
    match tail {
        16 => {
            (discriminator == Some(5) && source_kind == "ScaleFactor") || discriminator == Some(6)
        }
        19 => discriminator.is_none_or(|value| {
            matches!(value, 0 | 3 | 4) || (value == 6 && source_kind == "TangencyWeight")
        }),
        _ => false,
    }
}

/// Decode the exact same-index-delimited owner frame for every owned Design
/// parameter.
pub fn decode_parameter_owners(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    if parameters
        .iter()
        .all(|parameter| parameter.owner_record_index.is_none())
    {
        return Ok(Vec::new());
    }
    let mut headers_by_stream = HashMap::<&str, HashMap<u32, &DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        if headers_by_stream
            .entry(stream)
            .or_default()
            .insert(header.record_index, header)
            .is_some()
        {
            return Err(CodecError::malformed(format_args!(
                "Fusion Design stream has duplicate primary headers for record {}",
                header.record_index
            )));
        }
    }
    let mut streams = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = ids::native_scope(&entry.name);
        if streams
            .insert(stream, (entry, IndexedRecordOffsets::build(bytes)))
            .is_some()
        {
            return Err(CodecError::Malformed(
                "F3D contains duplicate Design BulkStream identities".into(),
            ));
        }
    }
    let mut out = Vec::new();
    for parameter in parameters {
        let Some(owner_index) = parameter.owner_record_index else {
            continue;
        };
        let malformed = |invariant: &str| {
            CodecError::malformed(format_args!(
                "Fusion Design parameter {} owner {} {invariant}",
                parameter.record_index, owner_index
            ))
        };
        let scope = native_stream(&parameter.id)
            .ok_or_else(|| malformed("has no Design stream identity"))?;
        let Some(header) = headers_by_stream
            .get(scope)
            .and_then(|headers| headers.get(&owner_index))
            .copied()
        else {
            // A parameter can retain a source owner reference after Fusion has
            // omitted that owner's primary frame. Keep the parameter native;
            // projection reports the unresolved binding as a loss.
            continue;
        };
        let (entry, records) = streams
            .get(scope)
            .ok_or_else(|| malformed("has no containing Design BulkStream"))?;
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset)
            .map_err(|_| malformed("primary header offset exceeds the platform address space"))?;
        let end = records
            .frames(owner_index)
            .find_map(|(start, end)| (start == at).then_some(end))
            .ok_or_else(|| malformed("has no following same-index paired header"))?;
        let frame = bytes
            .get(at..end)
            .ok_or_else(|| malformed("frame lies outside its Design BulkStream"))?;
        let (mut owner, evaluated_value_is_absolute) =
            if let Some(owner) = parse_parameter_owner(frame) {
                (owner, false)
            } else if let Some(mut owner) =
                parse_legacy_parameter_owner_68(frame, parameter.evaluated_value)
            {
                owner.evaluated_value_offset = parameter.evaluated_value_offset;
                (owner, true)
            } else if let Some(mut owner) =
                parse_legacy_parameter_owner_88(frame, parameter.evaluated_value)
            {
                owner.evaluated_value_offset = parameter.evaluated_value_offset;
                (owner, true)
            } else {
                return Err(malformed("does not match the parameter-owner grammar"));
            };
        if owner.record_index != owner_index
            || owner.parameter_record_index != parameter.record_index
        {
            return Err(malformed("does not link back to its referencing parameter"));
        }
        owner.id = ids::native_design_parameter_owner_id(&entry.name, header.byte_offset);
        owner.byte_offset = header.byte_offset;
        if !evaluated_value_is_absolute {
            owner.evaluated_value_offset = owner
                .evaluated_value_offset
                .checked_add(header.byte_offset)
                .ok_or_else(|| malformed("evaluated-value offset overflows u64"))?;
        }
        out.push(owner);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(crate) fn parse_parameter_owner(frame: &[u8]) -> Option<DesignParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != indexed_header::RECORD_INDEX
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || frame.get(owner_prefix::ZERO_RUN_8..owner_prefix::ONE_MARKER) != Some(&[0; 8])
        || frame.get(owner_prefix::ONE_MARKER..owner_prefix::SCOPE_MARKER) != Some(&[1, 1, 0, 0, 0])
        || frame.get(owner_prefix::SCOPE_MARKER) != Some(&1)
        || frame.get(owner_prefix::ZERO_RUN_6..owner_prefix::LOCAL_ORDINAL) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let scope_record_index = View::u32_le_at(frame, owner_prefix::SCOPE_RECORD_INDEX)?;

    // Parse the fixed suffix backward from the exact paired-header boundary.
    // This prevents a valid shorter prefix from being accepted as the record.
    let final_scope_marker = frame.len().checked_sub(11)?;
    let companion_marker = final_scope_marker.checked_sub(12)?;
    if frame.get(final_scope_marker) != Some(&1)
        || View::u32_le_at(frame, final_scope_marker + 1) != Some(scope_record_index)
        || frame.get(final_scope_marker + 5..frame.len()) != Some(&[0; 6])
        || frame.get(companion_marker) != Some(&1)
        || frame.get(companion_marker + 5..final_scope_marker) != Some(&[0; 7])
    {
        return None;
    }

    let without_variant = companion_marker.checked_sub(13).and_then(|at| {
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && frame.get(at + 5..companion_marker) == Some(&[0; 8]))
        .then_some((at, None))
    });
    let with_variant = companion_marker.checked_sub(14).and_then(|at| {
        let variant = *frame.get(at + 12)?;
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && frame.get(at + 5..at + 11) == Some(&[0; 6])
            && frame.get(at + 11) == Some(&1)
            && variant <= 1
            && frame.get(at + 13) == Some(&0))
        .then_some((at, Some(variant)))
    });
    let with_compact_variant = companion_marker.checked_sub(13).and_then(|at| {
        let variant = *frame.get(at + 12)?;
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && frame.get(at + 5..at + 11) == Some(&[0; 6])
            && frame.get(at + 11) == Some(&1)
            && variant <= 1)
            .then_some((at, Some(variant)))
    });
    if [without_variant, with_variant, with_compact_variant]
        .into_iter()
        .flatten()
        .count()
        != 1
    {
        return None;
    }
    let (repeated_scope_marker, variant) =
        without_variant.or(with_variant).or(with_compact_variant)?;

    let owned_ordinal_offset = repeated_scope_marker.checked_sub(8)?;
    let parameter_marker = owned_ordinal_offset.checked_sub(11)?;
    if frame.get(owned_ordinal_offset + 4..repeated_scope_marker) != Some(&[0; 4])
        || frame.get(parameter_marker) != Some(&1)
        || frame.get(parameter_marker + 5..owned_ordinal_offset) != Some(&[0; 6])
    {
        return None;
    }

    let scalar = frame.get(owner_prefix::LEN..parameter_marker)?;
    let (evaluated_value, evaluated_value_offset) = match scalar.len() {
        9 if scalar.first() == Some(&0) => (View::f64_le_at(frame, 40)?, 40),
        6 if matches!(scalar.get(..2), Some([0, 0 | 1])) => {
            (f64::from(View::u32_le_at(frame, 41)?), 41)
        }
        5 if scalar.first() == Some(&0) && variant.is_none() => {
            (f64::from(View::u32_le_at(frame, 40)?), 40)
        }
        13 if scalar.first() == Some(&1) && scalar.get(1..5) == Some(&[0; 4]) => {
            (View::f64_le_at(frame, 44)?, 44)
        }
        _ => return None,
    };
    let parameter_record_index = View::u32_le_at(frame, parameter_marker + 1)?;
    let companion_record_index = View::u32_le_at(frame, companion_marker + 1)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    if !evaluated_value.is_finite()
        || !(consecutive(record_index, parameter_record_index, companion_record_index)
            || consecutive(parameter_record_index, record_index, companion_record_index)
            || consecutive(record_index, companion_record_index, parameter_record_index))
    {
        return None;
    }

    Some(DesignParameterOwner {
        id: String::new(),
        byte_offset: 0,
        frame_length: u64::try_from(frame.len()).ok()?,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: View::u32_le_at(frame, owner_prefix::LOCAL_ORDINAL)?,
        evaluated_value,
        evaluated_value_offset,
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, owned_ordinal_offset)?,
        variant,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope and scalar lanes are absent.
///
/// The class admission is intentional. A short frame is not enough to select
/// this grammar because older class tags also occur on modern owner records.
pub(crate) fn parse_legacy_parameter_owner_68(
    frame: &[u8],
    evaluated_value: f64,
) -> Option<DesignParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if !is_legacy_parameter_owner_68_class(&class_tag)
        || frame.len() != legacy_owner_68::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || frame.get(legacy_owner_68::ZERO_RUN_8..legacy_owner_68::FIRST_MARKER) != Some(&[0; 8])
        || frame.get(legacy_owner_68::FIRST_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_13..legacy_owner_68::PARAMETER_MARKER)
            != Some(&[0; 13])
        || frame.get(legacy_owner_68::PARAMETER_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_6..legacy_owner_68::OWNED_ORDINAL) != Some(&[0; 6])
        || frame.get(legacy_owner_68::ZERO_RUN_7..legacy_owner_68::COMPANION_MARKER)
            != Some(&[0; 7])
        || frame.get(legacy_owner_68::COMPANION_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_8_TAIL..legacy_owner_68::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_68::PARAMETER_RECORD_INDEX)?;
    let companion_record_index = View::u32_le_at(frame, legacy_owner_68::COMPANION_RECORD_INDEX)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    if !evaluated_value.is_finite()
        || !consecutive(record_index, parameter_record_index, companion_record_index)
    {
        return None;
    }
    Some(DesignParameterOwner {
        id: String::new(),
        byte_offset: 0,
        frame_length: u64::try_from(legacy_owner_68::LEN).ok()?,
        class_tag,
        record_index,
        scope_record_index: 0,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: 0,
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_68::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope is repeated in the suffix but
/// whose scalar and local-ordinal lanes are absent.
pub(crate) fn parse_legacy_parameter_owner_88(
    frame: &[u8],
    evaluated_value: f64,
) -> Option<DesignParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if !is_legacy_parameter_owner_88_class(&class_tag)
        || frame.len() != legacy_owner_88::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || frame.get(legacy_owner_88::ZERO_RUN_8..legacy_owner_88::FIRST_MARKER) != Some(&[0; 8])
        || frame.get(legacy_owner_88::FIRST_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_13..legacy_owner_88::PARAMETER_MARKER)
            != Some(&[0; 13])
        || frame.get(legacy_owner_88::PARAMETER_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_6..legacy_owner_88::OWNED_ORDINAL) != Some(&[0; 6])
        || frame.get(legacy_owner_88::ZERO_RUN_4..legacy_owner_88::SCOPE_MARKER) != Some(&[0; 4])
        || frame.get(legacy_owner_88::SCOPE_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_8_BETWEEN_SCOPES..legacy_owner_88::COMPANION_MARKER)
            != Some(&[0; 8])
        || frame.get(legacy_owner_88::COMPANION_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_7..legacy_owner_88::REPEATED_SCOPE_MARKER)
            != Some(&[0; 7])
        || frame.get(legacy_owner_88::REPEATED_SCOPE_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_6_TAIL..legacy_owner_88::LEN) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_88::PARAMETER_RECORD_INDEX)?;
    let scope_record_index = View::u32_le_at(frame, legacy_owner_88::SCOPE_RECORD_INDEX)?;
    if scope_record_index == 0
        || View::u32_le_at(frame, legacy_owner_88::REPEATED_SCOPE_RECORD_INDEX)?
            != scope_record_index
    {
        return None;
    }
    let companion_record_index = View::u32_le_at(frame, legacy_owner_88::COMPANION_RECORD_INDEX)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    if !evaluated_value.is_finite()
        || !consecutive(record_index, parameter_record_index, companion_record_index)
    {
        return None;
    }
    Some(DesignParameterOwner {
        id: String::new(),
        byte_offset: 0,
        frame_length: u64::try_from(legacy_owner_88::LEN).ok()?,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: 0,
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_88::OWNED_ORDINAL)?,
        variant: None,
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
            scan.is_design_stream(entry, role::BULKSTREAM)
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
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(crate) fn parse_parameter_companion(prefix: &[u8]) -> Option<DesignParameterCompanion> {
    let (class_tag, after_tag) = lp_ascii_filtered(prefix, 0, 0..=2000, u8::is_ascii_graphic)?;
    if prefix.len() != companion_prefix::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || prefix.get(companion_prefix::ZERO_RUN_20..companion_prefix::OWNER_MARKER)
            != Some(&[0; 20])
        || prefix.get(companion_prefix::OWNER_MARKER) != Some(&1)
        || prefix.get(companion_prefix::ZERO_RUN_6..companion_prefix::TIMESTAMP_MICROS)
            != Some(&[0; 6])
        || prefix.get(companion_prefix::ZERO_RUN_8..companion_prefix::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let timestamp_micros = View::u64_le_at(prefix, companion_prefix::TIMESTAMP_MICROS)?;
    if timestamp_micros == 0 {
        return None;
    }
    Some(DesignParameterCompanion {
        id: String::new(),
        byte_offset: 0,
        class_tag,
        record_index: View::u32_le_at(prefix, indexed_header::RECORD_INDEX)?,
        owner_record_index: View::u32_le_at(prefix, companion_prefix::OWNER_RECORD_INDEX)?,
        timestamp_micros,
        timestamp_micros_offset: companion_prefix::TIMESTAMP_MICROS as u64,
        payload_byte_offset: companion_prefix::LEN as u64,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    })
}

/// Bind each companion to its exact owned byte interval and the construction
/// recipes nested in that interval.
#[allow(clippy::too_many_arguments)]
pub fn bind_parameter_companion_payloads<S: std::hash::BuildHasher>(
    companions: &mut [DesignParameterCompanion],
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    entities: &[DesignEntityHeader],
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
        let Some((start, mut end)) = companion_owned_interval(
            companion,
            parameters.iter(),
            owners,
            scopes,
            headers,
            stream_length,
        ) else {
            continue;
        };
        // Entity headers precede their owning scope record. A parameter
        // companion immediately before a new scope does not own that scope's
        // preamble even though no indexed sibling separates the two records.
        // Bind the preamble through the scope's entity identity, not by an
        // assumed class-tag or byte length.
        end = scopes
            .iter()
            .filter(|scope| {
                native_stream(&scope.id) == Some(stream)
                    && scope.byte_offset >= u64::try_from(end).unwrap_or(u64::MAX)
                    && scope.sketch_entity().is_some()
            })
            .filter_map(|scope| {
                entities
                    .iter()
                    .filter(|entity| {
                        native_stream(&entity.id) == Some(stream)
                            && scope.sketch_entity().is_some_and(|binding| {
                                binding.entity_suffix == entity.entity_suffix
                            })
                            && usize::try_from(entity.byte_offset)
                                .is_ok_and(|offset| offset >= start && offset < end)
                    })
                    .filter_map(|entity| usize::try_from(entity.byte_offset).ok())
                    .min()
            })
            .min()
            .unwrap_or(end);
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

#[cfg(test)]
mod tests;
