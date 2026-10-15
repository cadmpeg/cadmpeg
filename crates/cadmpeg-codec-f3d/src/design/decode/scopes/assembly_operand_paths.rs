// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand paths and their locator envelopes.

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::byte_fields::zeros_at;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::design::decode::text::retain_class_tag;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_locator_reference_run as path_locator_run;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::mesh::DesignRelaxedGuidText;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

/// Unwrap an `Option`, ending a fallible parse with `Ok(None)` when it is empty.
macro_rules! try_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

pub(super) fn exact_assembly_operand_paths(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    let Some((scope_at, search_start, locator_offsets)) = operand_path_locators(bytes, scope)
    else {
        return Ok(None);
    };
    let mut paths = [None, None];
    for (slot, relative_offset) in paths.iter_mut().zip(locator_offsets) {
        let Some((locator_record_index, locator_reference_offset)) = scope_at
            .checked_add(relative_offset)
            .map(|at| exact_same_segment_record_reference(ctx, bytes, at))
            .transpose()?
            .flatten()
        else {
            return Ok(None);
        };
        let offsets = records.offsets(locator_record_index);
        let first = ctx.partition_point(
            offsets,
            |locator_at| Ok(*locator_at < search_start),
            "find F3D assembly operand locator offsets",
        )?;
        let mut candidate = None;
        let ambiguous = ctx.position_by(
            offsets.get(first..).unwrap_or(&[]),
            |&locator_at| {
                let Some(parsed) = exact_assembly_operand_path_envelope(
                    ctx,
                    bytes,
                    scope,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                )?
                else {
                    return Ok(false);
                };
                Ok(candidate.replace(parsed).is_some())
            },
            "scan F3D assembly operand locator offsets",
        )?;
        if ambiguous.is_some() {
            return Ok(None);
        }
        *slot = candidate;
    }
    let [Some(first), Some(second)] = paths else {
        return Ok(None);
    };
    let mut spans = [(0, 0); 2];
    for (span, path) in spans.iter_mut().zip([&first, &second]) {
        let Some(start) = usize::try_from(path.link().locator_byte_offset).ok() else {
            return Ok(None);
        };
        let Some(wrapper_at) = usize::try_from(path.link().wrapper_byte_offset).ok() else {
            return Ok(None);
        };
        let Some(end) = next_indexed_record_offset(ctx, bytes, wrapper_at + 1)? else {
            return Ok(None);
        };
        *span = (start, end);
    }
    let [(first_start, first_end), (second_start, second_end)] = spans;
    if first.link().locator_record_index == second.link().locator_record_index
        || (first_start < second_end && second_start < first_end)
    {
        return Ok(None);
    }
    Ok(Some([first, second]))
}

/// The scope offset, the first offset a locator may occupy and the two
/// locator-reference offsets of an assembly scope that names two operand paths.
fn operand_path_locators(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<(usize, usize, [usize; 2])> {
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
    let count_at = scope_at
        .checked_add(locator_offsets[0].checked_sub(path_locator_run::FIRST_LOCATOR_REFERENCE)?)?;
    (View::u32_le_at(bytes, count_at)? == 2).then_some((scope_at, search_start, locator_offsets))
}

/// The fixed fields of an operand-path locator record.
struct LocatorFrame<'bytes> {
    class_tag: &'bytes [u8; 3],
    variable_reference: bool,
    scope_reference_offset: u64,
    wrapper_record_index: u32,
    wrapper_reference_offset: u64,
    /// Offset of the first path record, directly after the locator.
    path_at: usize,
}

fn locator_frame<'bytes>(
    ctx: &DecodeContext<'_>,
    bytes: &'bytes [u8],
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_at: usize,
) -> Result<Option<LocatorFrame<'bytes>>, CodecError> {
    let class_tag = try_some!(exact_indexed_header_at(
        bytes,
        locator_at,
        locator_record_index
    ));
    let variable_reference = crate::design::assembly::variable_reference_assembly_generation(
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    );
    let (locator_length, scope_backlink, wrapper_reference, constant_two, zero_tail) =
        if variable_reference {
            const PADDING: usize = variable_path_locator::TRANSFORM + 16 * 8;
            if class_tag != b"390"
                || !zeros_at::<{ variable_path_locator::SCOPE_BACKLINK - PADDING }>(
                    bytes,
                    try_some!(locator_at.checked_add(PADDING)),
                )
                || rigid_transform_at(
                    bytes,
                    try_some!(locator_at.checked_add(variable_path_locator::TRANSFORM)),
                )
                .is_none()
            {
                return Ok(None);
            }
            (
                variable_path_locator::LEN,
                variable_path_locator::SCOPE_BACKLINK,
                variable_path_locator::WRAPPER_REFERENCE,
                variable_path_locator::CONSTANT_TWO,
                variable_path_locator::ZERO_TAIL,
            )
        } else {
            if !zeros_at::<{ path_locator::NONZERO_RECORD_REFERENCE - path_locator::ZERO_RUN_10 }>(
                bytes,
                try_some!(locator_at.checked_add(path_locator::ZERO_RUN_10)),
            ) || try_some!(exact_same_segment_record_reference(
                ctx,
                bytes,
                try_some!(locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)),
            )?)
            .0 == 0
                || bytes.get(try_some!(locator_at.checked_add(path_locator::ZERO_32))) != Some(&0)
                || rigid_transform_at(
                    bytes,
                    try_some!(locator_at.checked_add(path_locator::TRANSFORM)),
                )
                .is_none()
                || bytes.get(try_some!(locator_at.checked_add(path_locator::ZERO_161))) != Some(&0)
            {
                return Ok(None);
            }
            (
                path_locator::LEN,
                path_locator::SCOPE_BACKLINK,
                path_locator::WRAPPER_REFERENCE,
                path_locator::CONSTANT_TWO,
                path_locator::ZERO_TAIL_2,
            )
        };
    let (scope_record_index, scope_reference_offset) =
        try_some!(exact_same_segment_record_reference(
            ctx,
            bytes,
            try_some!(locator_at.checked_add(scope_backlink))
        )?);
    let (wrapper_record_index, wrapper_reference_offset) =
        try_some!(exact_same_segment_record_reference(
            ctx,
            bytes,
            try_some!(locator_at.checked_add(wrapper_reference))
        )?);
    // A variable-reference wrapper lies at most 65 records past its locator,
    // so a path has at most 64 span records.
    let wrapper_in_range = if variable_reference {
        (try_some!(locator_record_index.checked_add(2))
            ..=try_some!(locator_record_index.checked_add(65)))
            .contains(&wrapper_record_index)
    } else {
        wrapper_record_index == try_some!(locator_record_index.checked_add(2))
    };
    // Both locator layouts end with a two-byte zero tail.
    if scope_record_index != scope.record_index
        || !wrapper_in_range
        || try_some!(View::u32_le_at(
            bytes,
            try_some!(locator_at.checked_add(constant_two))
        )) != 2
        || !zeros_at::<2>(bytes, try_some!(locator_at.checked_add(zero_tail)))
    {
        return Ok(None);
    }
    Ok(Some(LocatorFrame {
        class_tag,
        variable_reference,
        scope_reference_offset,
        wrapper_record_index,
        wrapper_reference_offset,
        path_at: try_some!(locator_at.checked_add(locator_length)),
    }))
}

fn exact_assembly_operand_path_envelope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    let Some(locator) = locator_frame(ctx, bytes, scope, locator_record_index, locator_at)? else {
        return Ok(None);
    };
    let variable_reference = locator.variable_reference;
    let path_record_index = locator_record_index + 1;
    if next_indexed_record_offset(ctx, bytes, locator_at + 1)? != Some(locator.path_at) {
        return Ok(None);
    }
    let mut span_storage = ctx.reserve_scoped(0, "f3d assembly path spans")?;
    let mut path_spans = Vec::new();
    let mut record_at = locator.path_at;
    // `locator_frame` bounds the path at 64 span records.
    for record_index in path_record_index..locator.wrapper_record_index {
        let Some(next) = next_indexed_record_offset(ctx, bytes, record_at + 1)? else {
            return Ok(None);
        };
        ctx.push_scoped_vec(
            &mut span_storage,
            &mut path_spans,
            (record_index, record_at, next),
            "f3d assembly path spans",
        )?;
        record_at = next;
    }
    let wrapper_at = record_at;
    let Some(wrapper_class_tag) =
        exact_indexed_header_at(bytes, wrapper_at, locator.wrapper_record_index)
    else {
        return Ok(None);
    };
    let Some(wrapper_end) = next_indexed_record_offset(ctx, bytes, wrapper_at + 1)? else {
        return Ok(None);
    };
    let Some(path_reference_offset) = wrapper_frame(
        ctx,
        bytes,
        (wrapper_at, wrapper_end),
        *wrapper_class_tag,
        variable_reference,
        path_spans.len(),
        path_record_index,
    )?
    else {
        return Ok(None);
    };
    let continuations = path_spans.get(1..).unwrap_or(&[]);
    if variable_reference {
        let mut reference_at = wrapper_at + path_wrapper::LEN;
        let references_match = ctx.all_by(
            continuations,
            |(record_index, _, _)| {
                let matches = exact_same_segment_record_reference(ctx, bytes, reference_at)?
                    .map(|reference| reference.0)
                    == Some(*record_index);
                reference_at += 11;
                Ok(matches)
            },
            "scan F3D variable assembly path spans",
        )?;
        if !references_match {
            return Ok(None);
        }
    }
    let link = DesignAssemblyOperandPathLink {
        locator_reference_offset,
        locator_record_index,
        locator_class_tag: retain_class_tag(
            ctx,
            *locator.class_tag,
            "copy F3D assembly operand path class tag",
        )?,
        locator_byte_offset: u64_from_index(locator_at),
        locator_scope_reference_offset: locator.scope_reference_offset,
        wrapper_record_index: locator.wrapper_record_index,
        wrapper_reference_offset: locator.wrapper_reference_offset,
        wrapper_class_tag: retain_class_tag(
            ctx,
            *wrapper_class_tag,
            "copy F3D assembly operand path class tag",
        )?,
        wrapper_byte_offset: u64_from_index(wrapper_at),
        path_reference_offset,
    };
    let Some(&(record_index, start, limit)) = path_spans.first() else {
        return Ok(None);
    };
    let Some(first) = exact_assembly_operand_path(ctx, bytes, start, record_index, limit, link)?
    else {
        return Ok(None);
    };
    if variable_reference && first.class_tag().as_str() != "330" {
        return Ok(None);
    }
    // Each continuation is validated as a path of its own before it joins the
    // first; the records API takes the continuation with its own link.
    let mut path = Some(first);
    let failed = ctx.position_by(
        continuations,
        |&(record_index, start, limit)| {
            let Some(current) = path.take() else {
                return Ok(true);
            };
            let Some(continuation) = exact_assembly_operand_path(
                ctx,
                bytes,
                start,
                record_index,
                limit,
                current.link().clone(),
            )?
            else {
                return Ok(true);
            };
            if !variable_reference || continuation.class_tag().as_str() != "330" {
                return Ok(true);
            }
            path = current.try_append(continuation, ctx)?;
            Ok(path.is_none())
        },
        "scan F3D assembly operand path continuations",
    )?;
    Ok(if failed.is_some() { None } else { path })
}

/// The path-reference offset of the wrapper record at `wrapper_at`, which
/// ends at `wrapper_end` and names the first of `span_count` path records.
fn wrapper_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    (wrapper_at, wrapper_end): (usize, usize),
    wrapper_class_tag: [u8; 3],
    variable_reference: bool,
    span_count: usize,
    path_record_index: u32,
) -> Result<Option<u64>, CodecError> {
    let (expected_wrapper_length, expected_span_count) = if variable_reference {
        (
            try_some!(path_wrapper::LEN
                .checked_add(try_some!(
                    try_some!(span_count.checked_sub(1)).checked_mul(11)
                ))),
            try_some!(u32::try_from(span_count).ok()),
        )
    } else {
        (path_wrapper::LEN, 1)
    };
    if variable_reference && wrapper_class_tag != *b"397"
        || !zeros_at::<{ path_wrapper::CONSTANT_ONE_BYTE - path_wrapper::ZERO_RUN_10 }>(
            bytes,
            try_some!(wrapper_at.checked_add(path_wrapper::ZERO_RUN_10)),
        )
        || bytes.get(try_some!(
            wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)
        )) != Some(&1)
        || try_some!(View::u32_le_at(
            bytes,
            try_some!(wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_WORD)),
        )) != expected_span_count
        || wrapper_end != try_some!(wrapper_at.checked_add(expected_wrapper_length))
    {
        return Ok(None);
    }
    let (referenced_path_record_index, path_reference_offset) =
        try_some!(exact_same_segment_record_reference(
            ctx,
            bytes,
            try_some!(wrapper_at.checked_add(path_wrapper::PATH_REFERENCE)),
        )?);
    Ok((referenced_path_record_index == path_record_index).then_some(path_reference_offset))
}

/// Read the counted relaxed GUID at `*position` into `guids` and advance past
/// it. `bytes` ends at the record limit.
fn take_located_guid(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: &mut usize,
    guids: &mut Vec<Located<DesignRelaxedGuidText>>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    let Some((value, end)) = fixed_relaxed_guid_text(ctx, bytes, *position)? else {
        return Ok(false);
    };
    ctx.push_vec(
        guids,
        Located {
            value,
            offset: u64_from_index(*position + 4),
        },
        operation,
    )?;
    *position = end;
    Ok(true)
}

/// Read the two identity GUID pairs, separated by the count 2, that close a
/// path record, and the count 2 after them. Returns the offset after that count.
fn take_identity_guids(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
    identity_guids: &mut Vec<Located<DesignRelaxedGuidText>>,
) -> Result<Option<usize>, CodecError> {
    const OPERATION: &str = "collect F3D assembly path identity GUIDs";
    for pair in 0..2 {
        if pair == 1 {
            if View::u64_le_at(bytes, position) != Some(2) {
                return Ok(None);
            }
            position += 8;
        }
        for _ in 0..2 {
            if !take_located_guid(ctx, bytes, &mut position, identity_guids, OPERATION)? {
                return Ok(None);
            }
        }
    }
    if View::u32_le_at(bytes, position) != Some(2) {
        return Ok(None);
    }
    Ok(Some(position + 4))
}

fn exact_assembly_operand_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    record_index: u32,
    limit: usize,
    link: DesignAssemblyOperandPathLink,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    let Some((class_tag, after_tag)) =
        lp_ascii_filtered_view(ctx, bytes, start, 1..=8, u8::is_ascii_digit)?
    else {
        return Ok(None);
    };
    if View::u64_le_at(bytes, after_tag) != Some(u64::from(record_index)) {
        return Ok(None);
    }
    let Some(record) = bytes.get(..limit) else {
        return Ok(None);
    };
    let mut occurrence_guids = Vec::new();
    let mut identity_guids = Vec::new();
    let padding_at = match class_tag {
        "294" | "299" | "307" => {
            if next_indexed_record_offset(ctx, bytes, start + 1)? != Some(limit)
                || !zeros_at::<6>(bytes, after_tag + 8)
                || bytes.get(after_tag + 14) != Some(&1)
                || !zeros_at::<3>(bytes, after_tag + 15)
            {
                return Ok(None);
            }
            let mut position = after_tag + 18;
            if !take_located_guid(
                ctx,
                record,
                &mut position,
                &mut occurrence_guids,
                "f3d assembly path occurrences",
            )? {
                return Ok(None);
            }
            let Some(padding_at) = take_identity_guids(ctx, record, position, &mut identity_guids)?
            else {
                return Ok(None);
            };
            Some(padding_at)
        }
        "329" | "330" | "386" | "390" => {
            if !zeros_at::<6>(bytes, after_tag + 8) {
                return Ok(None);
            }
            let Some(count) = View::u32_le_at(bytes, after_tag + 14)
                .and_then(|count| usize::try_from(count).ok())
                .filter(|count| (1..=64).contains(count))
            else {
                return Ok(None);
            };
            ctx.reserve_capacity(
                &mut occurrence_guids,
                count,
                "f3d assembly path occurrences",
            )?;
            let mut position = after_tag + 18;
            for _ in 0..count {
                if !take_located_guid(
                    ctx,
                    record,
                    &mut position,
                    &mut occurrence_guids,
                    "f3d assembly path occurrences",
                )? {
                    return Ok(None);
                }
            }
            if position == limit {
                if !matches!(class_tag, "329" | "330") {
                    return Ok(None);
                }
                None
            } else {
                let Some(padding_at) =
                    take_identity_guids(ctx, record, position, &mut identity_guids)?
                else {
                    return Ok(None);
                };
                Some(padding_at)
            }
        }
        _ => return Ok(None),
    };
    if let Some(padding_at) = padding_at {
        let Some(padding) = record.get(padding_at..) else {
            return Ok(None);
        };
        if !ctx.all_by(
            padding,
            |byte| Ok(*byte == 0),
            "validate F3D assembly path padding",
        )? {
            return Ok(None);
        }
    }
    let Some(class_tag) = crate::design::decode::text::class_tag_from_view(ctx, class_tag)? else {
        return Ok(None);
    };
    Ok(DesignAssemblyOperandPath::try_new(
        link,
        record_index,
        class_tag,
        u64_from_index(start),
        occurrence_guids,
        identity_guids,
    )
    .ok())
}

#[cfg(test)]
mod tests;
