// SPDX-License-Identifier: Apache-2.0
//! Parse Design parameter, owner, and companion frames.

use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

use cadmpeg_core::container::{ContainerEntry, ContainerRole};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

use crate::bytes::lp_utf16_bounded_charged;
use crate::container::ContainerScan;
use crate::design::decode::body::decode_stream;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::dimension_frames::companion_owned_interval;
use crate::design::decode::record_streams::{in_stream, record_stream};
use crate::design::decode::sketch::{
    indexed_record_header_at, native_scope_charged, next_indexed_record_offset,
    IndexedRecordHeader, IndexedRecordOffsets,
};
use crate::design::decode::text::{design_record_id_charged, retain_class_tag};
use crate::layout::design_parameter_legacy_287_prefix as legacy_287;
use crate::layout::design_parameter_legacy_287_tail as legacy_287_tail;
use crate::layout::design_parameter_owner_legacy_68 as legacy_owner_68;
use crate::layout::design_parameter_owner_legacy_88 as legacy_owner_88;
use crate::layout::design_parameter_owner_prefix as owner_prefix;
use crate::layout::indexed_companion_record_prefix as companion_prefix;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    identity::Located,
    parameters::{
        DesignParameter, DesignParameterCompanion, DesignParameterDiscriminator,
        DesignParameterOwner,
    },
    recipes::ConstructionRecipe,
};

/// A counted UTF-16 text field of `bounds` code units at `at`, copied into
/// retained storage, and the offset after it.
fn parameter_text(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(String, usize)>, CodecError> {
    lp_utf16_bounded_charged(ctx, payload, at, bounds, "f3d Design UTF-16 text")
}

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub(crate) fn decode_recipes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D parameter recipe streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(ctx, bytes, &entry.name, &mut out)?;
    }
    Ok(out)
}

/// Decode every indexed parameter record in each Design `BulkStream`.
pub(crate) fn decode_parameters(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignParameter>, CodecError> {
    let mut out = Vec::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D parameter streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream_parameters(ctx, bytes, &entry.name, &mut out)?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| &value.id,
        Ord::cmp,
        "sort f3d design parameters 1",
    )?;
    Ok(out)
}

/// Decode the parameter frames of one Design `BulkStream` into `out`. A frame
/// runs from an indexed-record header to the first header at least one header
/// length later. After a header that opens no parameter, the next candidate is
/// the first later header, so each stream byte is searched once.
fn decode_stream_parameters(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    out: &mut Vec<DesignParameter>,
) -> Result<(), CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "f3d parameter record index")?;
    let mut emitted_record_indices = HashSet::new();
    // The start of the last frame-end search and the header it found.
    let mut last_end_search: Option<(usize, Option<usize>)> = None;
    let mut next = next_indexed_record_offset(ctx, bytes, 0)?;
    while let Some(at) = next {
        let from = at + indexed_header::LEN;
        let end = match last_end_search {
            Some((searched, found))
                if searched <= from && found.is_none_or(|found| found >= from) =>
            {
                found
            }
            _ => next_indexed_record_offset(ctx, bytes, from)?,
        };
        last_end_search = Some((from, end));
        let Some(frame) = bytes.get(at..end.unwrap_or(bytes.len())) else {
            break;
        };
        let Some(parsed) = parse_design_parameter(ctx, frame)? else {
            next = overlapping_header(bytes, at).or(end);
            continue;
        };
        next = end;
        // The Design primary index exposes one live header for each logical
        // record index. Keep the first serialized parameter frame so stale
        // copies cannot create duplicate owner bindings or duplicate neutral
        // parameter identities.
        if emitted_record_indices.contains(&parsed.record_index) {
            continue;
        }
        reservation.with_storage(|| {
            ctx.reserve_set(&mut emitted_record_indices, 1, "f3d parameter record index")
        })?;
        emitted_record_indices.insert(parsed.record_index);
        ctx.reserve_vec(out, 1, "f3d decoded parameter records")?;
        out.push(locate_design_parameter(ctx, parsed, stream, at)?);
    }
    Ok(())
}

/// The first indexed-record header that starts inside the header at `at`.
fn overlapping_header(bytes: &[u8], at: usize) -> Option<usize> {
    (1..indexed_header::LEN)
        .map(|delta| at + delta)
        .find(|start| indexed_record_header_at(bytes, *start).is_some())
}

/// Design parameter parsed in frame-relative coordinates.
pub(in crate::design) struct ParsedDesignParameter {
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
    source_ordinal: u32,
    source_kind: String,
    owner_record_index: Option<u32>,
    family_discriminator: Option<DesignParameterDiscriminator>,
    expression: String,
    expression_offset: FrameRelative,
    source_kind_offset: FrameRelative,
    unit: Option<ParsedParameterUnit>,
    name: String,
    name_offset: FrameRelative,
    evaluated_value: f64,
    evaluated_value_offset: FrameRelative,
}

/// Unit token parsed in frame-relative coordinates.
struct ParsedParameterUnit {
    value: String,
    offset: FrameRelative,
}

impl ParsedDesignParameter {
    /// Locate this parameter in its containing stream.
    pub(in crate::design) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        frame_start: u64,
    ) -> Result<Option<DesignParameter>, CodecError> {
        let source_kind =
            ctx.validate_nonblank_text(self.source_kind, "validate parameter source kind")?;
        let expression =
            ctx.validate_nonblank_text(self.expression, "validate parameter expression")?;
        let name = ctx.validate_nonblank_text(self.name, "validate parameter name")?;
        let unit = match self.unit {
            Some(unit) => {
                let value = ctx.validate_nonblank_text(unit.value, "validate parameter unit")?;
                let Some(offset) = unit.offset.absolute(frame_start) else {
                    return Ok(None);
                };
                Some(crate::records::identity::RecordedValue { value, offset })
            }
            None => None,
        };
        let family_discriminator = match self.family_discriminator {
            Some(value) => {
                let Some(offset) =
                    FrameRelative(i128::from(DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET))
                        .absolute(frame_start)
                else {
                    return Ok(None);
                };
                Some(Located { value, offset })
            }
            None => None,
        };
        let Ok(source) = crate::records::parameters::DesignParameterSource::new(
            source_kind,
            self.owner_record_index,
            family_discriminator,
        ) else {
            return Ok(None);
        };
        let (
            Some(expression_offset),
            Some(source_kind_offset),
            Some(name_offset),
            Some(evaluated_value_offset),
        ) = (
            self.expression_offset.absolute(frame_start),
            self.source_kind_offset.absolute(frame_start),
            self.name_offset.absolute(frame_start),
            self.evaluated_value_offset.absolute(frame_start),
        )
        else {
            return Ok(None);
        };
        let id = design_record_id_charged(
            ctx,
            stream,
            ":design-parameter#",
            frame_start,
            "f3d parameter identifier",
        )?;
        Ok(
            DesignParameter::try_from(crate::records::parameters::DesignParameterDraft::<
                cadmpeg_core::text::NonBlankText<String>,
            > {
                id,
                byte_offset: frame_start,
                class_tag: self.class_tag,
                record_index: self.record_index,
                source_ordinal: self.source_ordinal,
                source,
                expression,
                expression_offset,
                source_kind_offset,
                unit,
                name,
                name_offset,
                evaluated_value: self.evaluated_value,
                evaluated_value_offset,
            })
            .ok(),
        )
    }
}

/// Frame-relative offset of the parameter family discriminator.
const DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET: u64 = 22;

/// Retained-copy operation of a parameter class tag.
const PARAMETER_CLASS_TAG_OPERATION: &str = "copy F3D Design parameter class tag";

/// Parse one indexed parameter frame in a Design `BulkStream` test fixture.
#[cfg(test)]
pub(in crate::design) fn parse_design_parameter_record(payload: &[u8]) -> Option<DesignParameter> {
    parse_design_parameter(&cadmpeg_test_support::service_decode_context(), payload)
        .unwrap()?
        .into_record(
            &cadmpeg_test_support::service_decode_context(),
            TEST_PARAMETER_STREAM,
            0,
        )
        .unwrap()
}

/// Design `BulkStream` name used when a test parses one isolated frame.
#[cfg(test)]
const TEST_PARAMETER_STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

/// Locate one parsed parameter frame in its containing stream.
fn locate_design_parameter(
    ctx: &DecodeContext<'_>,
    parsed: ParsedDesignParameter,
    stream: &str,
    at: usize,
) -> Result<DesignParameter, CodecError> {
    let frame_start = u64::try_from(at).map_err(|_| {
        CodecError::malformed("Fusion Design parameter frame offset exceeds the addressable stream")
    })?;
    parsed
        .into_record(ctx, stream, frame_start)?
        .ok_or_else(|| {
            CodecError::malformed("Fusion Design parameter frame has invalid fields or offsets")
        })
}

/// Source fields of a modern parameter prefix.
struct ParameterPrefix {
    family_discriminator: Option<DesignParameterDiscriminator>,
    source_ordinal: u32,
    owner_record_index: Option<u32>,
    expression_at: usize,
}

/// The discriminated prefix: a family discriminator at 22 and an optional
/// owner whose marker is at 35.
fn discriminated_parameter_prefix(payload: &[u8]) -> Option<ParameterPrefix> {
    let family_discriminator =
        DesignParameterDiscriminator::try_from(View::u64_le_at(payload, 22)?).ok()?;
    let (owner_record_index, expression_at) = match payload.get(35)? {
        0 => (None, 36),
        1 if zeros_at::<6>(payload, 40) => (Some(View::u32_le_at(payload, 36)?), 46),
        _ => return None,
    };
    Some(ParameterPrefix {
        family_discriminator: Some(family_discriminator),
        source_ordinal: View::u32_le_at(payload, 31)?,
        owner_record_index,
        expression_at,
    })
}

/// The compact owned prefix: a source ordinal at 26 and an owner at 31.
fn compact_owned_parameter_prefix(payload: &[u8]) -> Option<ParameterPrefix> {
    Some(ParameterPrefix {
        family_discriminator: None,
        source_ordinal: View::u32_le_at(payload, 26)?,
        owner_record_index: Some(View::u32_le_at(payload, 31)?),
        expression_at: 41,
    })
}

pub(in crate::design) fn parse_design_parameter(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let Some(header) = indexed_record_header_at(payload, 0) else {
        return Ok(None);
    };
    if !zeros_at::<11>(payload, 11) {
        return Ok(None);
    }
    if header.class_code == 287 {
        return parse_legacy_287_design_parameter(ctx, payload, header);
    }
    let compact_owned =
        zeros_at::<15>(payload, 11) && payload.get(30) == Some(&1) && zeros_at::<6>(payload, 35);
    let discriminated = !compact_owned && payload.get(30) == Some(&0);
    let prefix = if discriminated {
        discriminated_parameter_prefix(payload)
    } else if compact_owned {
        compact_owned_parameter_prefix(payload)
    } else {
        return parse_legacy_design_parameter(ctx, payload, header);
    };
    let Some(ParameterPrefix {
        family_discriminator,
        source_ordinal,
        owner_record_index,
        expression_at,
    }) = prefix
    else {
        return Ok(None);
    };
    let Some((expression, expression_end)) = parameter_text(ctx, payload, expression_at, 1..=256)?
    else {
        return Ok(None);
    };
    let (trailer_len, valid_expression_trailer) = if !discriminated {
        (5, zeros_at::<5>(payload, expression_end))
    } else if owner_record_index.is_none() {
        (
            9,
            bytes_at::<9>(payload, expression_end) == Some(&[0, 0, 0, 0, 0, 0, 0, 0, 1]),
        )
    } else {
        (9, zeros_at::<9>(payload, expression_end))
    };
    if !valid_expression_trailer {
        return Ok(None);
    }
    let prefixed_kind_at = expression_end + 10;
    let prefixed_kind =
        if discriminated && owner_record_index.is_some() && zeros_at::<10>(payload, expression_end)
        {
            parameter_text(ctx, payload, prefixed_kind_at, 1..=256)?
        } else {
            None
        };
    let (source_kind_at, (source_kind, source_kind_end)) = match prefixed_kind {
        Some(value) => (prefixed_kind_at, value),
        None => {
            let source_kind_at = expression_end + trailer_len;
            let Some(value) = parameter_text(ctx, payload, source_kind_at, 1..=256)? else {
                return Ok(None);
            };
            (source_kind_at, value)
        }
    };
    let first_at = source_kind_end + usize::from(discriminated) * 4;
    if discriminated && View::u32_le_at(payload, source_kind_end) != Some(0) {
        return Ok(None);
    }
    let (unit, name, name_at, name_end) = if View::u32_le_at(payload, first_at) == Some(0) {
        let name_at = first_at + 4;
        let Some((name, name_end)) = parameter_text(ctx, payload, name_at, 1..=256)? else {
            return Ok(None);
        };
        (None, name, name_at, name_end)
    } else {
        let Some((first, first_end)) = parameter_text(ctx, payload, first_at, 1..=256)? else {
            return Ok(None);
        };
        match parameter_text(ctx, payload, first_end, 1..=256)? {
            Some((second, second_end)) => (
                Some(ParsedParameterUnit {
                    value: first,
                    offset: FrameRelative::of(first_at + 4),
                }),
                second,
                first_end,
                second_end,
            ),
            None => (None, first, first_at, first_end),
        }
    };
    let Some(evaluated_value) = View::f64_le_at(payload, name_end) else {
        return Ok(None);
    };
    let Some(tail) = payload.get(name_end + 8..) else {
        return Ok(None);
    };
    if tail.len() != 12
        || bytes_at::<2>(tail, 0) != Some(&[0, 1])
        || !zeros_at::<9>(tail, 3)
        || !valid_design_parameter_family(family_discriminator, &source_kind, tail[2])
        || source_kind.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(ParsedDesignParameter {
        class_tag: header.retain_class_tag(ctx, PARAMETER_CLASS_TAG_OPERATION)?,
        record_index: header.record_index,
        source_ordinal,
        source_kind,
        owner_record_index,
        family_discriminator,
        expression,
        expression_offset: FrameRelative::of(expression_at + 4),
        source_kind_offset: FrameRelative::of(source_kind_at + 4),
        unit,
        name,
        name_offset: FrameRelative::of(name_at + 4),
        evaluated_value,
        evaluated_value_offset: FrameRelative::of(name_end),
    }))
}

/// Parse the class-287 owned parameter family.
///
/// This family uses the compact-owned prefix and a class-specific `0xAF` tail.
/// Its expression is followed by one of the two fixed five-byte trailers.
fn parse_legacy_287_design_parameter(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    header: IndexedRecordHeader<'_>,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    if !zeros_at::<15>(payload, legacy_287::ZERO_RUN_15)
        || payload.get(legacy_287::OWNER_MARKER) != Some(&legacy_287::OWNER_MARKER_VALUE)
        || !zeros_at::<6>(payload, legacy_287::ZERO_RUN_6)
    {
        return Ok(None);
    }
    let (Some(source_ordinal), Some(owner_record_index)) = (
        View::u32_le_at(payload, legacy_287::SOURCE_ORDINAL),
        View::u32_le_at(payload, legacy_287::OWNER_RECORD_INDEX),
    ) else {
        return Ok(None);
    };
    let Some((expression, expression_end)) =
        parameter_text(ctx, payload, legacy_287::EXPRESSION_LENGTH, 1..=256)?
    else {
        return Ok(None);
    };
    if !matches!(
        bytes_at::<CLASS_287_EXPRESSION_TRAILER_LEN>(payload, expression_end),
        Some([0, 0, 0, 0 | 1, 0])
    ) {
        return Ok(None);
    }
    let source_kind_at = expression_end + CLASS_287_EXPRESSION_TRAILER_LEN;
    let Some((source_kind, source_kind_end)) =
        parameter_text(ctx, payload, source_kind_at, 1..=256)?
    else {
        return Ok(None);
    };
    let (unit, name, name_at, name_end) = if View::u32_le_at(payload, source_kind_end) == Some(0) {
        let name_at = source_kind_end + 4;
        let Some((name, name_end)) = parameter_text(ctx, payload, name_at, 1..=256)? else {
            return Ok(None);
        };
        (None, name, name_at, name_end)
    } else {
        let Some((unit, unit_end)) = parameter_text(ctx, payload, source_kind_end, 1..=64)? else {
            return Ok(None);
        };
        let Some((name, name_end)) = parameter_text(ctx, payload, unit_end, 1..=256)? else {
            return Ok(None);
        };
        (
            Some(ParsedParameterUnit {
                value: unit,
                offset: FrameRelative::of(source_kind_end + 4),
            }),
            name,
            unit_end,
            name_end,
        )
    };
    let Some(evaluated_value) = View::f64_le_at(payload, name_end) else {
        return Ok(None);
    };
    let Some(tail) = payload.get(name_end + 8..) else {
        return Ok(None);
    };
    if tail.len() != legacy_287_tail::LEN
        || bytes_at::<2>(tail, 0) != Some(&legacy_287_tail::TAIL_PREFIX_VALUE)
        || tail[legacy_287_tail::FAMILY_MARKER] != legacy_287_tail::FAMILY_MARKER_VALUE
        || !zeros_at::<9>(tail, legacy_287_tail::ZERO_RUN_9)
        || source_kind.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(ParsedDesignParameter {
        class_tag: header.retain_class_tag(ctx, PARAMETER_CLASS_TAG_OPERATION)?,
        record_index: header.record_index,
        source_ordinal,
        source_kind,
        owner_record_index: Some(owner_record_index),
        family_discriminator: None,
        expression,
        expression_offset: FrameRelative::of(legacy_287::EXPRESSION_LENGTH + 4),
        source_kind_offset: FrameRelative::of(source_kind_at + 4),
        unit,
        name,
        name_offset: FrameRelative::of(name_at + 4),
        evaluated_value,
        evaluated_value_offset: FrameRelative::of(name_end),
    }))
}

const CLASS_287_EXPRESSION_TRAILER_LEN: usize = 5;

/// Fixed tail of the legacy parameter frame.
const LEGACY_PARAMETER_TAIL: [u8; 12] = [0, 1, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn parse_legacy_design_parameter(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    header: IndexedRecordHeader<'_>,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    if !zeros_at::<14>(payload, 11) || payload.get(29) != Some(&1) || !zeros_at::<6>(payload, 34) {
        return Ok(None);
    }
    let (Some(source_ordinal), Some(owner_record_index)) =
        (View::u32_le_at(payload, 25), View::u32_le_at(payload, 30))
    else {
        return Ok(None);
    };
    let expression_at = 40;
    let Some((expression, expression_end)) = parameter_text(ctx, payload, expression_at, 1..=256)?
    else {
        return Ok(None);
    };
    if !zeros_at::<5>(payload, expression_end) {
        return Ok(None);
    }
    let source_kind_at = expression_end + 5;
    let Some((source_kind, source_kind_end)) =
        parameter_text(ctx, payload, source_kind_at, 1..=256)?
    else {
        return Ok(None);
    };
    let unit_at = source_kind_end;
    let Some((unit, unit_end)) = parameter_text(ctx, payload, unit_at, 1..=64)? else {
        return Ok(None);
    };
    let name_at = unit_end;
    let Some((name, name_end)) = parameter_text(ctx, payload, name_at, 1..=256)? else {
        return Ok(None);
    };
    let Some(evaluated_value) = View::f64_le_at(payload, name_end) else {
        return Ok(None);
    };
    let Some(tail) = payload.get(name_end + 8..) else {
        return Ok(None);
    };
    if tail.len() != LEGACY_PARAMETER_TAIL.len()
        || bytes_at::<12>(tail, 0) != Some(&LEGACY_PARAMETER_TAIL)
        || source_kind.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(ParsedDesignParameter {
        class_tag: header.retain_class_tag(ctx, PARAMETER_CLASS_TAG_OPERATION)?,
        record_index: header.record_index,
        source_ordinal,
        source_kind,
        owner_record_index: Some(owner_record_index),
        family_discriminator: None,
        expression,
        expression_offset: FrameRelative::of(expression_at + 4),
        source_kind_offset: FrameRelative::of(source_kind_at + 4),
        unit: Some(ParsedParameterUnit {
            value: unit,
            offset: FrameRelative::of(unit_at + 4),
        }),
        name,
        name_offset: FrameRelative::of(name_at + 4),
        evaluated_value,
        evaluated_value_offset: FrameRelative::of(name_end),
    }))
}

pub(crate) fn design_parameter_discriminator(source_kind: &str) -> u64 {
    match source_kind {
        "ScaleFactor" => 5,
        "TangencyWeight" => 6,
        _ => 0,
    }
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

fn valid_design_parameter_family(
    discriminator: Option<DesignParameterDiscriminator>,
    source_kind: &str,
    tail: u8,
) -> bool {
    use crate::records::parameters::DesignParameterDiscriminator::{
        Code0, Code3, Code4, Code5, Code6,
    };
    match tail {
        16 => {
            (discriminator == Some(Code5) && source_kind == "ScaleFactor")
                || discriminator == Some(Code6)
        }
        19 => discriminator.is_none_or(|value| {
            matches!(value, Code0 | Code3 | Code4)
                || (value == Code6 && source_kind == "TangencyWeight")
        }),
        _ => false,
    }
}

/// Decode the exact same-index-delimited owner frame for every owned Design
/// parameter.
pub(crate) fn decode_parameter_owners(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    if !ctx.any_by(
        parameters,
        |parameter| Ok(parameter.owner_record_index().is_some()),
        "scan F3D parameter owner references",
    )? {
        return Ok(Vec::new());
    }
    let mut reservation = ctx.reserve_scoped(0, "f3d owner header index")?;
    let mut headers_by_record = HashMap::<(&str, u32), &DesignRecordHeader>::new();
    for header in ctx.admit_iter(headers, "scan F3D parameter owner headers")? {
        let Some(stream) = record_stream(ctx, &header.id)? else {
            continue;
        };
        let previous = reservation.with_storage(|| {
            ctx.insert_hash_map(
                &mut headers_by_record,
                (stream, header.record_index),
                header,
                "f3d owner header index",
            )
        })?;
        if previous.is_some() {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion Design stream has duplicate primary headers for record {}",
                    header.record_index
                ),
            ));
        }
    }
    // Each Design `BulkStream` by native scope, with its header index once a
    // parameter owner needs it.
    let mut streams = HashMap::<String, (&ContainerEntry, Option<IndexedRecordOffsets>)>::new();
    for entry in ctx
        .admit_iter(&scan.entries, "scan F3D parameter owner streams")?
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = reservation.with_storage(|| native_scope_charged(ctx, &entry.name))?;
        if ctx.contains_key_hash_map(&streams, stream.as_str(), "f3d owner stream index")? {
            return Err(CodecError::Malformed(
                "F3D contains duplicate Design BulkStream identities".into(),
            ));
        }
        reservation.with_storage(|| {
            ctx.insert_hash_map(
                &mut streams,
                stream,
                (entry, None),
                "f3d owner stream index",
            )
        })?;
    }
    let mut out = Vec::new();
    for parameter in ctx.admit_iter(parameters, "scan F3D parameter owner bindings")? {
        let Some(owner_index) = parameter.owner_record_index() else {
            continue;
        };
        let malformed = |invariant: &str| {
            crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion Design parameter {} owner {} {invariant}",
                    parameter.record_index, owner_index
                ),
            )
        };
        let scope = record_stream(ctx, &parameter.id)?
            .ok_or_else(|| malformed("has no Design stream identity"))?;
        let Some(header) = ctx
            .get_hash_map(
                &headers_by_record,
                &(scope, owner_index),
                "find F3D parameter owner header",
            )?
            .copied()
        else {
            // A parameter can retain a source owner reference after Fusion has
            // omitted that owner's primary frame. Keep the parameter native;
            // projection reports the unresolved binding as a loss.
            continue;
        };
        let (entry, records) = ctx
            .get_mut_hash_map(&mut streams, scope, "find F3D parameter owner stream")?
            .ok_or_else(|| malformed("has no containing Design BulkStream"))?;
        let bytes = scan.entry_bytes(&entry.name)?;
        let records = match records {
            Some(records) => records,
            slot @ None => {
                slot.insert(reservation.with_storage(|| IndexedRecordOffsets::build(ctx, bytes))?)
            }
        };
        let at = usize::try_from(header.byte_offset)
            .map_err(|_| malformed("primary header offset exceeds the platform address space"))?;
        let end = records
            .frame_at(ctx, owner_index, at)?
            .ok_or_else(|| malformed("has no following same-index paired header"))?;
        let frame = bytes
            .get(at..end)
            .ok_or_else(|| malformed("frame lies outside its Design BulkStream"))?;
        let evaluated = Located {
            value: parameter.evaluated_value().get(),
            offset: parameter.evaluated_value_offset(),
        };
        let layout = parameter_owner_layout(frame)
            .or_else(|| legacy_parameter_owner_68_layout(frame, evaluated, header.byte_offset))
            .or_else(|| legacy_parameter_owner_88_layout(frame, evaluated, header.byte_offset))
            .ok_or_else(|| malformed("does not match the parameter-owner grammar"))?;
        let owner = layout
            .retain(ctx)?
            .locate(ctx, &entry.name, header.byte_offset)?
            .ok_or_else(|| malformed("has invalid owner fields or evaluated-value offset"))?;
        if owner.record_index() != owner_index
            || owner.parameter_record_index() != parameter.record_index
        {
            return Err(malformed("does not link back to its referencing parameter"));
        }
        ctx.push_vec(&mut out, owner, "f3d parameter owner")?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| value.id(),
        Ord::cmp,
        "sort f3d design parameters 2",
    )?;
    Ok(out)
}

/// Byte offset measured from an indexed frame start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameRelative(pub(crate) i128);

impl FrameRelative {
    /// The offset `at` bytes after the frame start.
    fn of(at: usize) -> Self {
        Self(i128::from(u64_from_index(at)))
    }

    fn between(offset: u64, frame_start: u64) -> Self {
        Self(i128::from(offset) - i128::from(frame_start))
    }

    /// Convert this offset to a stream-absolute position.
    pub(super) fn absolute(self, frame_start: u64) -> Option<u64> {
        u64::try_from(i128::from(frame_start).checked_add(self.0)?).ok()
    }
}

/// Parameter owner parsed in frame-relative coordinates.
pub(in crate::design) struct ParsedParameterOwner {
    pub(super) frame_length: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) record_index: u32,
    pub(super) scope_record_index: u32,
    pub(super) local_ordinal: u32,
    pub(super) evaluated_value: FiniteReal,
    pub(super) evaluated_value_offset: FrameRelative,
    pub(super) parameter_record_index: u32,
    pub(super) owned_ordinal: u32,
    variant: Option<u8>,
    pub(super) companion_record_index: u32,
}

impl ParsedParameterOwner {
    /// Locate this owner in its containing stream.
    fn locate(
        self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        frame_start: u64,
    ) -> Result<Option<DesignParameterOwner>, CodecError> {
        let Some(evaluated_value_offset) = self.evaluated_value_offset.absolute(frame_start) else {
            return Ok(None);
        };
        let id = design_record_id_charged(
            ctx,
            stream,
            ":design-parameter-owner#",
            frame_start,
            "f3d parameter owner identifier",
        )?;
        Ok(self.with_id(id, frame_start, evaluated_value_offset))
    }

    /// Locate this owner in a test stream.
    #[cfg(test)]
    pub(in crate::design) fn into_record(
        self,
        stream: &str,
        frame_start: u64,
    ) -> Option<DesignParameterOwner> {
        let evaluated_value_offset = self.evaluated_value_offset.absolute(frame_start)?;
        self.with_id(
            crate::ids::native_design_parameter_owner_id(stream, frame_start),
            frame_start,
            evaluated_value_offset,
        )
    }

    fn with_id(
        self,
        id: String,
        frame_start: u64,
        evaluated_value_offset: u64,
    ) -> Option<DesignParameterOwner> {
        DesignParameterOwner::from_parts(crate::records::parameters::DesignParameterOwnerWire {
            id,
            byte_offset: frame_start,
            frame_length: self.frame_length,
            class_tag: self.class_tag,
            record_index: self.record_index,
            scope_record_index: self.scope_record_index,
            local_ordinal: self.local_ordinal,
            evaluated_value: self.evaluated_value,
            evaluated_value_offset,
            parameter_record_index: self.parameter_record_index,
            owned_ordinal: self.owned_ordinal,
            variant: self.variant,
            companion_record_index: self.companion_record_index,
        })
        .ok()
    }
}

/// Parameter owner fields read from fixed offsets, with the class tag still
/// borrowed from the frame.
struct ParameterOwnerLayout<'a> {
    class_tag: &'a [u8; 3],
    frame_length: u64,
    record_index: u32,
    scope_record_index: u32,
    local_ordinal: u32,
    evaluated_value: FiniteReal,
    evaluated_value_offset: FrameRelative,
    parameter_record_index: u32,
    owned_ordinal: u32,
    variant: Option<u8>,
    companion_record_index: u32,
}

impl ParameterOwnerLayout<'_> {
    /// The parsed owner, with its class tag copied into retained storage.
    fn retain(self, ctx: &DecodeContext<'_>) -> Result<ParsedParameterOwner, CodecError> {
        Ok(ParsedParameterOwner {
            frame_length: self.frame_length,
            class_tag: retain_class_tag(ctx, self.class_tag, "copy F3D class tag")?,
            record_index: self.record_index,
            scope_record_index: self.scope_record_index,
            local_ordinal: self.local_ordinal,
            evaluated_value: self.evaluated_value,
            evaluated_value_offset: self.evaluated_value_offset,
            parameter_record_index: self.parameter_record_index,
            owned_ordinal: self.owned_ordinal,
            variant: self.variant,
            companion_record_index: self.companion_record_index,
        })
    }
}

/// Whether three record indexes ascend by one.
fn consecutive(first: u32, second: u32, third: u32) -> bool {
    first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
}

pub(in crate::design) fn parse_parameter_owner(
    ctx: &DecodeContext<'_>,
    frame: &[u8],
) -> Result<Option<ParsedParameterOwner>, CodecError> {
    parameter_owner_layout(frame)
        .map(|layout| layout.retain(ctx))
        .transpose()
}

/// The modern owner frame: a fixed prefix, a scalar lane and a fixed suffix
/// read backward from the exact paired-header boundary.
fn parameter_owner_layout(frame: &[u8]) -> Option<ParameterOwnerLayout<'_>> {
    let header = indexed_record_header_at(frame, 0)?;
    if !zeros_at::<8>(frame, owner_prefix::ZERO_RUN_8)
        || bytes_at::<5>(frame, owner_prefix::ONE_MARKER) != Some(&[1, 1, 0, 0, 0])
        || frame.get(owner_prefix::SCOPE_MARKER) != Some(&1)
        || !zeros_at::<6>(frame, owner_prefix::ZERO_RUN_6)
    {
        return None;
    }
    let record_index = header.record_index;
    let scope_record_index = View::u32_le_at(frame, owner_prefix::SCOPE_RECORD_INDEX)?;

    // Parse the fixed suffix backward from the exact paired-header boundary.
    // This prevents a valid shorter prefix from being accepted as the record.
    let final_scope_marker = frame.len().checked_sub(11)?;
    let companion_marker = final_scope_marker.checked_sub(12)?;
    if frame.get(final_scope_marker) != Some(&1)
        || View::u32_le_at(frame, final_scope_marker + 1) != Some(scope_record_index)
        || !zeros_at::<6>(frame, final_scope_marker + 5)
        || frame.get(companion_marker) != Some(&1)
        || !zeros_at::<7>(frame, companion_marker + 5)
    {
        return None;
    }

    let without_variant = companion_marker.checked_sub(13).and_then(|at| {
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && zeros_at::<8>(frame, at + 5))
        .then_some((at, None))
    });
    let with_variant = companion_marker.checked_sub(14).and_then(|at| {
        let variant = *frame.get(at + 12)?;
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && zeros_at::<6>(frame, at + 5)
            && frame.get(at + 11) == Some(&1)
            && variant <= 1
            && frame.get(at + 13) == Some(&0))
        .then_some((at, Some(variant)))
    });
    let with_compact_variant = companion_marker.checked_sub(13).and_then(|at| {
        let variant = *frame.get(at + 12)?;
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && zeros_at::<6>(frame, at + 5)
            && frame.get(at + 11) == Some(&1)
            && variant <= 1)
            .then_some((at, Some(variant)))
    });
    let candidate_count = usize::from(without_variant.is_some())
        + usize::from(with_variant.is_some())
        + usize::from(with_compact_variant.is_some());
    if candidate_count != 1 {
        return None;
    }
    let (repeated_scope_marker, variant) =
        without_variant.or(with_variant).or(with_compact_variant)?;

    let owned_ordinal_offset = repeated_scope_marker.checked_sub(8)?;
    let parameter_marker = owned_ordinal_offset.checked_sub(11)?;
    if !zeros_at::<4>(frame, owned_ordinal_offset + 4)
        || frame.get(parameter_marker) != Some(&1)
        || !zeros_at::<6>(frame, parameter_marker + 5)
    {
        return None;
    }

    let scalar = frame.get(owner_prefix::LEN..parameter_marker)?;
    let (evaluated_value, evaluated_value_offset) = match scalar.len() {
        9 if scalar.first() == Some(&0) => (View::f64_le_at(frame, 40)?, 40),
        6 if matches!(bytes_at::<2>(scalar, 0), Some([0, 0 | 1])) => {
            (f64::from(View::u32_le_at(frame, 41)?), 41)
        }
        5 if scalar.first() == Some(&0) && variant.is_none() => {
            (f64::from(View::u32_le_at(frame, 40)?), 40)
        }
        13 if scalar.first() == Some(&1) && zeros_at::<4>(scalar, 1) => {
            (View::f64_le_at(frame, 44)?, 44)
        }
        _ => return None,
    };
    let evaluated_value = FiniteReal::new(evaluated_value)?;
    let parameter_record_index = View::u32_le_at(frame, parameter_marker + 1)?;
    let companion_record_index = View::u32_le_at(frame, companion_marker + 1)?;
    if !(consecutive(record_index, parameter_record_index, companion_record_index)
        || consecutive(parameter_record_index, record_index, companion_record_index)
        || consecutive(record_index, companion_record_index, parameter_record_index))
    {
        return None;
    }

    Some(ParameterOwnerLayout {
        class_tag: header.class_tag,
        frame_length: u64::try_from(frame.len()).ok()?,
        record_index,
        scope_record_index,
        local_ordinal: View::u32_le_at(frame, owner_prefix::LOCAL_ORDINAL)?,
        evaluated_value,
        evaluated_value_offset: FrameRelative(evaluated_value_offset),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, owned_ordinal_offset)?,
        variant,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope and scalar lanes are absent.
#[cfg(test)]
fn parse_legacy_parameter_owner_68(
    ctx: &DecodeContext<'_>,
    frame: &[u8],
    evaluated: Located<f64>,
    frame_start: u64,
) -> Result<Option<ParsedParameterOwner>, CodecError> {
    legacy_parameter_owner_68_layout(frame, evaluated, frame_start)
        .map(|layout| layout.retain(ctx))
        .transpose()
}

/// The legacy owner envelope whose scope and scalar lanes are absent. The
/// class admission is intentional: a short frame is not enough to select this
/// grammar because older class tags also occur on modern owner records.
fn legacy_parameter_owner_68_layout(
    frame: &[u8],
    evaluated: Located<f64>,
    frame_start: u64,
) -> Option<ParameterOwnerLayout<'_>> {
    let header = indexed_record_header_at(frame, 0)?;
    if !is_legacy_parameter_owner_68_class(std::str::from_utf8(header.class_tag).ok()?)
        || frame.len() != legacy_owner_68::LEN
        || !zeros_at::<8>(frame, legacy_owner_68::ZERO_RUN_8)
        || frame.get(legacy_owner_68::FIRST_MARKER) != Some(&1)
        || !zeros_at::<13>(frame, legacy_owner_68::ZERO_RUN_13)
        || frame.get(legacy_owner_68::PARAMETER_MARKER) != Some(&1)
        || !zeros_at::<6>(frame, legacy_owner_68::ZERO_RUN_6)
        || !zeros_at::<7>(frame, legacy_owner_68::ZERO_RUN_7)
        || frame.get(legacy_owner_68::COMPANION_MARKER) != Some(&1)
        || !zeros_at::<8>(frame, legacy_owner_68::ZERO_RUN_8_TAIL)
    {
        return None;
    }
    let record_index = header.record_index;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_68::PARAMETER_RECORD_INDEX)?;
    let companion_record_index = View::u32_le_at(frame, legacy_owner_68::COMPANION_RECORD_INDEX)?;
    let evaluated_value = FiniteReal::new(evaluated.value)?;
    if !consecutive(record_index, parameter_record_index, companion_record_index) {
        return None;
    }
    Some(ParameterOwnerLayout {
        class_tag: header.class_tag,
        frame_length: u64::try_from(legacy_owner_68::LEN).ok()?,
        record_index,
        scope_record_index: 0,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: FrameRelative::between(evaluated.offset, frame_start),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_68::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope is repeated in the suffix but
/// whose scalar and local-ordinal lanes are absent.
#[cfg(test)]
fn parse_legacy_parameter_owner_88(
    ctx: &DecodeContext<'_>,
    frame: &[u8],
    evaluated: Located<f64>,
    frame_start: u64,
) -> Result<Option<ParsedParameterOwner>, CodecError> {
    legacy_parameter_owner_88_layout(frame, evaluated, frame_start)
        .map(|layout| layout.retain(ctx))
        .transpose()
}

/// The legacy owner envelope whose scope is repeated in the suffix but whose
/// scalar and local-ordinal lanes are absent.
fn legacy_parameter_owner_88_layout(
    frame: &[u8],
    evaluated: Located<f64>,
    frame_start: u64,
) -> Option<ParameterOwnerLayout<'_>> {
    let header = indexed_record_header_at(frame, 0)?;
    if !is_legacy_parameter_owner_88_class(std::str::from_utf8(header.class_tag).ok()?)
        || frame.len() != legacy_owner_88::LEN
        || !zeros_at::<8>(frame, legacy_owner_88::ZERO_RUN_8)
        || frame.get(legacy_owner_88::FIRST_MARKER) != Some(&1)
        || !zeros_at::<13>(frame, legacy_owner_88::ZERO_RUN_13)
        || frame.get(legacy_owner_88::PARAMETER_MARKER) != Some(&1)
        || !zeros_at::<6>(frame, legacy_owner_88::ZERO_RUN_6)
        || !zeros_at::<4>(frame, legacy_owner_88::ZERO_RUN_4)
        || frame.get(legacy_owner_88::SCOPE_MARKER) != Some(&1)
        || !zeros_at::<8>(frame, legacy_owner_88::ZERO_RUN_8_BETWEEN_SCOPES)
        || frame.get(legacy_owner_88::COMPANION_MARKER) != Some(&1)
        || !zeros_at::<7>(frame, legacy_owner_88::ZERO_RUN_7)
        || frame.get(legacy_owner_88::REPEATED_SCOPE_MARKER) != Some(&1)
        || !zeros_at::<6>(frame, legacy_owner_88::ZERO_RUN_6_TAIL)
    {
        return None;
    }
    let record_index = header.record_index;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_88::PARAMETER_RECORD_INDEX)?;
    let scope_record_index = View::u32_le_at(frame, legacy_owner_88::SCOPE_RECORD_INDEX)?;
    if scope_record_index == 0
        || View::u32_le_at(frame, legacy_owner_88::REPEATED_SCOPE_RECORD_INDEX)?
            != scope_record_index
    {
        return None;
    }
    let companion_record_index = View::u32_le_at(frame, legacy_owner_88::COMPANION_RECORD_INDEX)?;
    let evaluated_value = FiniteReal::new(evaluated.value)?;
    if !consecutive(record_index, parameter_record_index, companion_record_index) {
        return None;
    }
    Some(ParameterOwnerLayout {
        class_tag: header.class_tag,
        frame_length: u64::try_from(legacy_owner_88::LEN).ok()?,
        record_index,
        scope_record_index,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: FrameRelative::between(evaluated.offset, frame_start),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_88::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Decode the fixed prefix of every indexed record paired with a parameter
/// owner. Record-specific payload after the prefix is decoded independently.
pub(crate) fn decode_parameter_companions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    owners: &[DesignParameterOwner],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "f3d parameter companion headers")?;
    let mut headers_by_record = HashMap::new();
    for header in ctx.admit_iter(headers, "scan F3D parameter companion headers")? {
        let Some(stream) = record_stream(ctx, &header.id)? else {
            continue;
        };
        reservation.with_storage(|| {
            ctx.insert_hash_map(
                &mut headers_by_record,
                (stream, header.record_index),
                header,
                "f3d parameter companion headers",
            )
        })?;
    }
    let mut out = Vec::new();
    for owner in ctx.admit_iter(owners, "scan F3D parameter companion owners")? {
        let Some(scope) = record_stream(ctx, owner.id())? else {
            continue;
        };
        let Some(header) = ctx
            .get_hash_map(
                &headers_by_record,
                &(scope, owner.companion_record_index()),
                "find F3D parameter companion header",
            )?
            .copied()
        else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(parsed) = usize::try_from(header.byte_offset)
            .ok()
            .and_then(|at| bytes.get(at..at.checked_add(companion_prefix::LEN)?))
            .and_then(parse_parameter_companion)
        else {
            continue;
        };
        if parsed.record_index != owner.companion_record_index()
            || parsed.owner_record_index != owner.record_index()
        {
            continue;
        }
        let Some(companion) = parsed.into_record(ctx, &entry.name, header.byte_offset)? else {
            continue;
        };
        ctx.push_vec(&mut out, companion, "f3d parameter companions")?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |value| value.id(),
        Ord::cmp,
        "sort f3d design parameters 3",
    )?;
    Ok(out)
}

/// Parameter companion prefix parsed in frame-relative coordinates, with the
/// class tag still borrowed from the frame.
struct ParsedParameterCompanion<'a> {
    class_tag: &'a [u8; 3],
    record_index: u32,
    owner_record_index: u32,
    timestamp_micros: std::num::NonZeroU64,
    timestamp_micros_offset: FrameRelative,
}

impl ParsedParameterCompanion<'_> {
    /// Locate this companion prefix in its containing stream. The owned payload
    /// extent and recipes are bound afterward by
    /// `bind_parameter_companion_payloads`.
    fn into_record(
        self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        frame_start: u64,
    ) -> Result<Option<DesignParameterCompanion>, CodecError> {
        let Some(timestamp_offset) = self.timestamp_micros_offset.absolute(frame_start) else {
            return Ok(None);
        };
        let class_tag = retain_class_tag(ctx, self.class_tag, "copy F3D class tag")?;
        let id = design_record_id_charged(
            ctx,
            stream,
            ":design-parameter-companion#",
            frame_start,
            "f3d parameter companion identifier",
        )?;
        Ok(Some(DesignParameterCompanion::unbound(
            id,
            frame_start,
            class_tag,
            self.record_index,
            self.owner_record_index,
            self.timestamp_micros,
            timestamp_offset,
        )))
    }
}

fn parse_parameter_companion(prefix: &[u8]) -> Option<ParsedParameterCompanion<'_>> {
    let header = indexed_record_header_at(prefix, 0)?;
    if prefix.len() != companion_prefix::LEN
        || !zeros_at::<20>(prefix, companion_prefix::ZERO_RUN_20)
        || prefix.get(companion_prefix::OWNER_MARKER) != Some(&1)
        || !zeros_at::<6>(prefix, companion_prefix::ZERO_RUN_6)
        || !zeros_at::<8>(prefix, companion_prefix::ZERO_RUN_8)
    {
        return None;
    }
    Some(ParsedParameterCompanion {
        class_tag: header.class_tag,
        record_index: header.record_index,
        owner_record_index: View::u32_le_at(prefix, companion_prefix::OWNER_RECORD_INDEX)?,
        timestamp_micros: std::num::NonZeroU64::new(View::u64_le_at(
            prefix,
            companion_prefix::TIMESTAMP_MICROS,
        )?)?,
        timestamp_micros_offset: FrameRelative::of(companion_prefix::TIMESTAMP_MICROS),
    })
}

/// Records a companion payload is resolved against.
pub(crate) struct ParameterCompanionInputs<'a, S: std::hash::BuildHasher> {
    /// Indexed parameter records.
    pub(crate) parameters: &'a [DesignParameter],
    /// Parameter owner frames.
    pub(crate) owners: &'a [DesignParameterOwner],
    /// Parameter scope records.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Design entity headers.
    pub(crate) entities: &'a [DesignEntityHeader],
    /// Indexed Design record headers.
    pub(crate) headers: &'a [DesignRecordHeader],
    /// Construction recipes.
    pub(crate) recipes: &'a [ConstructionRecipe],
    /// Byte length of each Design `BulkStream`, by scope.
    pub(crate) stream_lengths: &'a HashMap<String, usize, S>,
}

/// Bind each companion to its exact owned byte interval and the construction
/// recipes nested in that interval. A companion whose payload cannot be
/// resolved is returned unbound.
pub(crate) fn bind_parameter_companion_payloads<S: std::hash::BuildHasher>(
    ctx: &DecodeContext<'_>,
    companions: Vec<DesignParameterCompanion>,
    inputs: &ParameterCompanionInputs<'_, S>,
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    ctx.try_collect_vec(
        companions.into_iter().map(|companion| {
            Ok::<_, CodecError>(match companion_payload(ctx, &companion, inputs)? {
                Some(payload) => companion.bound(payload),
                None => companion,
            })
        }),
        "f3d bound parameter companions",
    )
}

/// Resolve the byte interval and nested recipes one companion owns.
fn companion_payload<S: std::hash::BuildHasher>(
    ctx: &DecodeContext<'_>,
    companion: &DesignParameterCompanion,
    inputs: &ParameterCompanionInputs<'_, S>,
) -> Result<Option<crate::records::parameters::DesignCompanionPayload>, CodecError> {
    let Some(stream) = record_stream(ctx, companion.id())? else {
        return Ok(None);
    };
    let Some(stream_length) = ctx
        .get_hash_map(
            inputs.stream_lengths,
            stream,
            "find F3D companion stream length",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let Some((start, end)) = companion_owned_interval(
        ctx,
        companion,
        ctx.admit_iter(inputs.parameters, "scan F3D companion parameters")?,
        inputs.owners,
        inputs.scopes,
        inputs.headers,
        stream_length,
    )?
    else {
        return Ok(None);
    };
    let end = scope_preamble_start(ctx, stream, start, end, inputs.scopes, inputs.entities)?;
    let byte_offset = u64_from_index(start);
    let byte_length = u64_from_index(end - start);
    let mut reservation = ctx.reserve_scoped(0, "f3d companion owned recipes")?;
    let mut owned = Vec::new();
    for recipe in ctx.admit_iter(inputs.recipes, "scan F3D companion owned recipes")? {
        if recipe.byte_offset < byte_offset
            || recipe.byte_offset >= u64_from_index(end)
            || !in_stream(ctx, &recipe.id, stream)?
        {
            continue;
        }
        ctx.push_scoped_vec(
            &mut reservation,
            &mut owned,
            recipe,
            "f3d companion owned recipes",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut owned[..],
        |recipe| recipe.byte_offset,
        Ord::cmp,
        "sort f3d design parameters 4",
    )?;
    let mut owned_ids = Vec::new();
    for recipe in ctx.admit_iter(&owned, "scan F3D companion owned recipe IDs")? {
        let id = ctx.copy_retained_text(&recipe.id, "f3d companion owned recipe identifier")?;
        ctx.push_vec(&mut owned_ids, id, "f3d companion owned recipe identifiers")?;
    }
    Ok(Some(
        crate::records::parameters::DesignCompanionPayload::new(
            byte_offset,
            byte_length,
            owned_ids,
        ),
    ))
}

/// The end of a companion's owned interval `start..end` in `stream` after
/// scope preambles are excluded.
///
/// Entity headers precede their owning scope record. A parameter companion
/// immediately before a new scope does not own that scope's preamble even
/// though no indexed sibling separates the two records. The preamble is bound
/// through the scope's sketch-entity identity, not by an assumed class tag or
/// byte length: the interval ends at the first entity header in it whose
/// entity is the sketch entity of a scope record of `stream` at or after `end`.
fn scope_preamble_start(
    ctx: &DecodeContext<'_>,
    stream: &str,
    start: usize,
    end: usize,
    scopes: &[DesignParameterScope],
    entities: &[DesignEntityHeader],
) -> Result<usize, CodecError> {
    let mut reservation = ctx.reserve_scoped(0, "f3d companion preamble entities")?;
    let mut sketch_entities = Vec::new();
    for scope in ctx.admit_iter(scopes, "scan F3D companion preamble scopes")? {
        let Some(binding) = scope.sketch_entity() else {
            continue;
        };
        if scope.byte_offset() < u64_from_index(end) || !in_stream(ctx, &scope.id, stream)? {
            continue;
        }
        ctx.push_scoped_vec(
            &mut reservation,
            &mut sketch_entities,
            binding.entity_id.suffix(),
            "f3d companion preamble entities",
        )?;
    }
    if sketch_entities.is_empty() {
        return Ok(end);
    }
    ctx.sort_unstable_by_key(
        &mut sketch_entities,
        |entity_id| *entity_id,
        Ord::cmp,
        "sort F3D companion preamble entities",
    )?;
    let mut preamble = end;
    for entity in ctx.admit_iter(entities, "scan F3D companion preamble entities")? {
        let Ok(offset) = usize::try_from(entity.byte_offset) else {
            continue;
        };
        if offset < start || offset >= preamble || !in_stream(ctx, &entity.id, stream)? {
            continue;
        }
        if ctx
            .binary_search(
                &sketch_entities,
                &entity.entity_id.suffix(),
                "find F3D companion preamble entity",
            )?
            .is_ok()
        {
            preamble = offset;
        }
    }
    Ok(preamble)
}

#[cfg(test)]
mod tests;
