// SPDX-License-Identifier: Apache-2.0
//! Exact thread construction scopes and thread payloads.

use crate::bytes::lp_utf16_bounded_charged;
use crate::design::decode::byte_fields::{bytes_at, zeros_at};
use crate::design::decode::reference_runs::admit_reference_values;
use crate::layout::thread_compact_construction_tail as thread_compact_tail;
use crate::layout::thread_compact_legacy_construction_tail as thread_compact_legacy_tail;
use crate::layout::thread_owner_marked_scope_prefix as thread_owner;
use crate::layout::thread_standard_construction_tail as thread_tail;
use crate::layout::thread_standard_legacy_construction_tail as thread_standard_legacy_tail;
use crate::layout::thread_standard_scope_prefix as thread_standard;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::feature::thread;
use crate::records::feature::thread::DesignThreadConstruction;
use crate::records::feature::thread::DesignThreadForm;
use crate::records::identity::ReferenceRun;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_thread_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignThreadConstruction>, CodecError> {
    let references = scope.reference_members();
    if !matches!(scope.payload(), DesignScopePayload::Thread(_))
        || references.len() < 2
        || !references.len().is_multiple_of(2)
    {
        return Ok(None);
    }
    let Some((prefix_form, designation_at)) =
        usize::try_from(scope.byte_offset()).ok().and_then(|start| {
            let (form, designation_delta) = exact_thread_prefix(bytes.get(start..)?)?;
            Some((form, start.checked_add(designation_delta)?))
        })
    else {
        return Ok(None);
    };
    // The face groups are built once the payload and its class pair are
    // accepted, so a rejected scope holds no storage for them.
    let Some(mut construction) =
        parse_thread_payload(ctx, bytes, designation_at, prefix_form, Vec::new())?
    else {
        return Ok(None);
    };
    let class_pair_is_valid = match construction.form {
        DesignThreadForm::StandardLegacy => {
            scope.class_tag.as_str() == "334" && scope.paired_class_tag.as_str() == "262"
        }
        DesignThreadForm::CompactLegacy => {
            scope.class_tag.as_str() == "414" && scope.paired_class_tag.as_str() == "263"
        }
        DesignThreadForm::Standard | DesignThreadForm::Compact(_) => true,
    };
    if !class_pair_is_valid {
        return Ok(None);
    }
    construction.face_group_record_indices = thread_face_groups(ctx, references, prefix_form)?;
    Ok(Some(construction))
}

/// The face-group references of a Thread scope: the first reference of the
/// standard form, or every other reference of the compact form.
fn thread_face_groups(
    ctx: &DecodeContext<'_>,
    references: &ReferenceRun<u32>,
    form: ThreadPrefix,
) -> Result<Vec<u32>, CodecError> {
    const OPERATION: &str = "f3d Thread face groups";
    match form {
        ThreadPrefix::Standard => {
            let mut groups = ctx.vector_storage(1, OPERATION)?;
            if let Some(first) = references.values().next() {
                ctx.push_vec(&mut groups, *first, OPERATION)?;
            }
            Ok(groups)
        }
        ThreadPrefix::Compact => {
            let mut groups = ctx.vector_storage(references.len() / 2, OPERATION)?;
            for group in
                admit_reference_values(ctx, references, "scan F3D Thread face-group references")?
                    .step_by(2)
            {
                ctx.push_vec(&mut groups, *group, OPERATION)?;
            }
            Ok(groups)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThreadPrefix {
    Standard,
    Compact,
}

fn exact_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    let direct = if zeros_at::<10>(bytes, thread_standard::ZERO_RUN_10)
        && View::f64_le_at(bytes, thread_standard::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(
            bytes,
            thread_standard::STANDARD_MARKER,
            thread_standard::STANDARD_PREFIX_TAIL,
        )
        .map(|form| (form, thread_standard::LEN))
    } else {
        None
    };
    let owner_marked = if zeros_at::<9>(bytes, thread_owner::ZERO_RUN_9)
        && View::u32_le_at(bytes, thread_owner::OWNER_MARKER)? == 1
        && bytes.get(thread_owner::SEPARATOR) == Some(&0)
        && View::f64_le_at(bytes, thread_owner::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(bytes, thread_owner::FORM_MARKER, thread_owner::FORM_TOKEN)
            .map(|form| (form, thread_owner::LEN))
    } else {
        None
    };
    match (direct, owner_marked) {
        (Some(prefix), None) | (None, Some(prefix)) => Some(prefix),
        _ => None,
    }
}

fn thread_form(bytes: &[u8], marker_at: usize, token_at: usize) -> Option<ThreadPrefix> {
    match (
        bytes_at::<5>(bytes, marker_at)?,
        bytes_at::<4>(bytes, token_at)?,
    ) {
        ([1, 2, 0, 0, 0], [0x36, 0, 0x67, 0]) => Some(ThreadPrefix::Standard),
        ([0, 2, 0, 0, 0], [0x36, 0, 0x48, 0]) => Some(ThreadPrefix::Compact),
        _ => None,
    }
}

/// The Thread construction whose designation text starts at `designation_at`,
/// carrying `face_group_record_indices`.
pub(super) fn parse_thread_payload(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    designation_at: usize,
    expected_form: ThreadPrefix,
    face_group_record_indices: Vec<u32>,
) -> Result<Option<DesignThreadConstruction>, CodecError> {
    let Some((designation, after_designation)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        designation_at,
        1..=128,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Some((nominal_size_text, after_nominal)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        after_designation,
        1..=64,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Some((profile, after_profile)) =
        lp_utf16_bounded_charged(ctx, bytes, after_nominal, 1..=256, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    let designation = ctx.validate_nonblank_text(designation, "validate thread designation")?;
    let profile = ctx.validate_nonblank_text(profile, "validate thread profile")?;
    let Some(tail) = thread_tail_fields(bytes, after_profile, expected_form) else {
        return Ok(None);
    };
    let (Ok(nominal_size), Ok(designation), Ok(profile)) = (
        thread::DesignThreadNominalSize::try_from(nominal_size_text),
        cadmpeg_core::text::NonBlankString::try_from(designation),
        cadmpeg_core::text::NonBlankString::try_from(profile),
    ) else {
        return Ok(None);
    };
    Ok(Some(DesignThreadConstruction {
        form: tail.form,
        designation_offset: u64_from_index(designation_at),
        designation,
        nominal_size,
        profile,
        pitch: tail.pitch,
        face_group_record_indices,
        diameters: tail.diameters,
    }))
}

/// The fixed fields after the profile text of a Thread payload.
struct ThreadTail {
    form: DesignThreadForm,
    pitch: cadmpeg_ir::scalar::PositiveReal,
    diameters: thread::DesignThreadDiameters,
}

fn thread_tail_fields(
    bytes: &[u8],
    after_profile: usize,
    expected_form: ThreadPrefix,
) -> Option<ThreadTail> {
    let (pitch_marker, trailer_kind) = match (expected_form, bytes_at::<5>(bytes, after_profile)?) {
        (ThreadPrefix::Standard, [0, 1, 0, 0, 0]) => (1, ThreadTrailerKind::Standard),
        (ThreadPrefix::Standard, [1, 1, 0, 0, 0]) => (0, ThreadTrailerKind::StandardLegacy),
        (ThreadPrefix::Compact, [1, 2, 0, 0, 0]) => (0, ThreadTrailerKind::Compact),
        (ThreadPrefix::Compact, [1, 1, 0, 0, 0]) => (0, ThreadTrailerKind::CompactLegacy),
        _ => return None,
    };
    let major_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MAJOR_DIAMETER)?;
    let minor_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MINOR_DIAMETER)?;
    let pitch = (bytes.get(after_profile + thread_tail::PITCH_MARKER) == Some(&pitch_marker))
        .then(|| View::f64_le_at(bytes, after_profile + thread_tail::PITCH))??;
    let pitch_diameter = View::f64_le_at(bytes, after_profile + thread_tail::PITCH_DIAMETER)?;
    let trailer_at = match trailer_kind {
        ThreadTrailerKind::Standard => thread_tail::STANDARD_TRAILER,
        ThreadTrailerKind::Compact => thread_compact_tail::COMPACT_TRAILER,
        ThreadTrailerKind::StandardLegacy => thread_standard_legacy_tail::LEGACY_TRAILER,
        ThreadTrailerKind::CompactLegacy => thread_compact_legacy_tail::LEGACY_TRAILER,
    };
    let trailer_offset = after_profile.checked_add(trailer_at)?;
    let closing_flag = bytes_at::<4>(bytes, trailer_offset) == Some(&[0, 0, 0, 1]);
    let form = match trailer_kind {
        ThreadTrailerKind::Standard if bytes_at::<2>(bytes, trailer_offset) == Some(&[0, 1]) => {
            DesignThreadForm::Standard
        }
        ThreadTrailerKind::StandardLegacy if closing_flag => DesignThreadForm::StandardLegacy,
        ThreadTrailerKind::CompactLegacy if closing_flag => DesignThreadForm::CompactLegacy,
        ThreadTrailerKind::Compact if closing_flag => DesignThreadForm::Compact(None),
        ThreadTrailerKind::Compact
            if bytes.get(trailer_offset) == Some(&1)
                && zeros_at::<6>(bytes, trailer_offset + 5) =>
        {
            let reference_offset = trailer_offset.checked_add(1)?;
            let record_index =
                std::num::NonZeroU32::new(View::u32_le_at(bytes, reference_offset)?)?;
            DesignThreadForm::Compact(Some(crate::records::identity::Located {
                value: record_index,
                offset: u64_from_index(reference_offset),
            }))
        }
        _ => return None,
    };
    Some(ThreadTail {
        form,
        pitch: cadmpeg_ir::scalar::PositiveReal::new(pitch)?,
        diameters: thread::DesignThreadDiameters::new(
            major_diameter,
            minor_diameter,
            pitch_diameter,
        )?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadTrailerKind {
    Standard,
    Compact,
    StandardLegacy,
    CompactLegacy,
}
