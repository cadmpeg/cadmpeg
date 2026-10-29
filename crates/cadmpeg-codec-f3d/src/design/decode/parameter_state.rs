// SPDX-License-Identifier: Apache-2.0
//! Source-owned CommonData parameter-state records.

use super::meta::DesignPrimaryFrame;
use crate::layout::{
    design_parameter_state as state_layout, design_parameter_state_companion as companion_layout,
    indexed_design_record_header as indexed,
};
use crate::{
    container::ContainerScan,
    records::{
        parameter_state::{
            DesignParameterDocumentHeader, DesignParameterState, DesignParameterStateCompanion,
        },
        references::DesignClassTag,
    },
};
use cadmpeg_core::{
    decode::{u64_from_index, DecodeContext, View},
    CodecError,
};
use cadmpeg_ir::{
    source_fidelity::parts::{DecodeParts, PartOutput, PartSpec},
    CodecFormat,
};

use std::collections::BTreeMap;

const STATE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
const COMPANION_GUID: &str = "E276745D-D84E-40D5-88BC-8C94DD4ACCDF";
const ROOT_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";

/// Staged typed records and the exact failed-part homes.
#[derive(Debug, Default)]
pub(crate) struct DecodedStateParts {
    pub(crate) states: Vec<DesignParameterState>,
    pub(crate) companions: Vec<DesignParameterStateCompanion>,
    pub(crate) headers: Vec<DesignParameterDocumentHeader>,
    pub(crate) fidelity: cadmpeg_ir::SourceFidelity,
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

/// Decode supported parameter-state families from their actual primary records.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<DecodedStateParts, CodecError> {
    let mut output = PartOutput::new(CodecFormat::F3d);
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        if !meta.types.iter().any(|ty| {
            ty.type_guid.as_str().eq_ignore_ascii_case(STATE_GUID)
                && ty.version == 4
                && ty.entities.values().len() != 0
        }) {
            continue;
        }
        let source = scan
            .entry_view(&entry.name)
            .ok_or_else(|| CodecError::malformed("parameter state has no source View"))?;
        let frames = supported_frames(ctx, source, &meta)?;
        decode_stream(ctx, source, &entry.name, &frames, &mut output)?;
    }
    let (_, mut native, fidelity, losses) = output.into_parts();
    let namespace = native.namespace_mut("f3d");
    let states = namespace.arena_as_for_decode(ctx, "design_parameter_states")?;
    let companions = namespace.arena_as_for_decode(ctx, "design_parameter_state_companions")?;
    let headers = namespace.arena_as_for_decode(ctx, "design_parameter_document_headers")?;
    Ok(DecodedStateParts {
        states,
        companions,
        headers,
        fidelity,
        losses,
    })
}

fn supported_frames<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    meta: &'a crate::metastream::MetaStream,
) -> Result<Vec<Option<DesignPrimaryFrame<'a>>>, CodecError> {
    let primary = crate::metastream::primary_record_frames(meta, source.window().len())?;
    let mut frames = ctx.alloc_filled(primary.len(), None, "parameter state source frames")?;
    for (slot, physical) in frames.iter_mut().zip(primary) {
        if let Some(ty) = meta.types.iter().find(|ty| {
            ((ty.type_guid.as_str().eq_ignore_ascii_case(STATE_GUID) && ty.version == 4)
                || (ty.type_guid.as_str().eq_ignore_ascii_case(COMPANION_GUID) && ty.version == 1)
                || (ty.type_guid.as_str().eq_ignore_ascii_case(ROOT_GUID) && ty.version == 23))
                && ty.entities.values().any(|id| *id == physical.entity_id)
        }) {
            *slot = Some(DesignPrimaryFrame {
                entity_id: physical.entity_id,
                class_tag: ty.class_tag.clone(),
                start: physical.start,
                member_end: physical.member_end,
                end: physical.end,
                design_type: ty,
            });
        }
    }
    Ok(frames)
}

fn matching(frame: &DesignPrimaryFrame<'_>, guid: &str, version: u32) -> bool {
    frame
        .design_type
        .type_guid
        .as_str()
        .eq_ignore_ascii_case(guid)
        && frame.design_type.version == version
}

fn identity(stream: &str, frame: &DesignPrimaryFrame<'_>) -> String {
    crate::design::decode::text::design_record_id(
        stream,
        if matching(frame, STATE_GUID, 4) {
            ":design-parameter-state#"
        } else if matching(frame, COMPANION_GUID, 1) {
            ":design-parameter-state-companion#"
        } else {
            ":design-parameter-document-header#"
        },
        u64_from_index(frame.start),
    )
}

fn extent<'a>(source: View<'a>, frame: &DesignPrimaryFrame<'_>) -> Result<View<'a>, CodecError> {
    let start = source
        .start()
        .checked_add(frame.start)
        .ok_or_else(|| CodecError::malformed("parameter state source start overflows"))?;
    let end = source
        .start()
        .checked_add(frame.end)
        .ok_or_else(|| CodecError::malformed("parameter state source end overflows"))?;
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("parameter state primary extent exceeds its source"))
}

fn at<'a>(source: View<'a>, offset: usize, len: usize) -> Result<View<'a>, CodecError> {
    let start = source
        .start()
        .checked_add(offset)
        .ok_or_else(|| CodecError::malformed("parameter state field start overflows"))?;
    let end = start
        .checked_add(len)
        .ok_or_else(|| CodecError::malformed("parameter state field end overflows"))?;
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("parameter state field exceeds its record"))
}

fn zeroes(source: View<'_>, offset: usize, count: usize) -> Result<(), CodecError> {
    if at(source, offset, count)?
        .window()
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(CodecError::malformed(
            "parameter state reserved bytes are nonzero",
        ));
    }
    Ok(())
}

fn header(
    source: View<'_>,
    frame: &DesignPrimaryFrame<'_>,
) -> Result<(DesignClassTag, u32), CodecError> {
    if View::u32_le_at(source.window(), 0) != Some(3) {
        return Err(CodecError::malformed(
            "parameter state class tag length is not three",
        ));
    }
    let mut fields = at(
        source,
        indexed::CLASS_TAG,
        indexed::LEN - indexed::CLASS_TAG,
    )?;
    let tag = std::str::from_utf8(fields.req_take(3)?)
        .map_err(|_| CodecError::malformed("parameter state class tag is not ASCII"))?;
    let class_tag = DesignClassTag::try_from(tag.to_owned()).map_err(CodecError::malformed)?;
    let index = fields.req_u32_le()?;
    if class_tag != frame.class_tag || u64::from(index) != frame.entity_id {
        return Err(CodecError::malformed(
            "parameter state header differs from its primary registration",
        ));
    }
    Ok((class_tag, index))
}

fn reference(
    source: View<'_>,
    marker: usize,
    target: usize,
    flags: usize,
) -> Result<u64, CodecError> {
    if at(source, marker, 1)?.window() != [1] {
        return Err(CodecError::malformed(
            "parameter state requires a local reference",
        ));
    }
    zeroes(source, flags, 2)?;
    at(source, target, 8)?
        .req_u64_le()
        .map_err(CodecError::from)
}

fn state(
    source: View<'_>,
    frame: &DesignPrimaryFrame<'_>,
) -> Result<(DesignClassTag, u32, u64, u64), CodecError> {
    if source.window().len() != state_layout::LEN {
        return Err(CodecError::malformed(
            "parameter state has an unsupported member extent",
        ));
    }
    let (tag, index) = header(source, frame)?;
    zeroes(source, state_layout::ZERO_RUN_23, 23)?;
    zeroes(source, state_layout::ZERO_RUN_5, 5)?;
    let companion = reference(
        source,
        state_layout::COMPANION_MARKER,
        state_layout::COMPANION_INDEX,
        state_layout::COMPANION_FLAGS,
    )?;
    let root = reference(
        source,
        state_layout::ROOT_MARKER,
        state_layout::ROOT_INDEX,
        state_layout::ROOT_FLAGS,
    )?;
    Ok((tag, index, companion, root))
}

fn companion(
    source: View<'_>,
    frame: &DesignPrimaryFrame<'_>,
) -> Result<(DesignClassTag, u32, u8, u32, u64), CodecError> {
    if source.window().len() != companion_layout::LEN {
        return Err(CodecError::malformed(
            "parameter state companion has an unsupported member extent",
        ));
    }
    let (tag, index) = header(source, frame)?;
    zeroes(source, companion_layout::ZERO_RUN_8, 8)?;
    zeroes(source, companion_layout::ZERO_RUN_9, 9)?;
    let control = at(source, companion_layout::CONTROL, 1)?.req_u8()?;
    let control_word = at(source, companion_layout::CONTROL_WORD, 4)?.req_u32_le()?;
    if control != 1 || control_word != 1 {
        return Err(CodecError::malformed(
            "parameter state companion controls are unsupported",
        ));
    }
    let target = reference(
        source,
        companion_layout::STATE_MARKER,
        companion_layout::STATE_INDEX,
        companion_layout::STATE_FLAGS,
    )?;
    Ok((tag, index, control, control_word, target))
}

fn target<'a, 'b>(
    frames: &'a [Option<DesignPrimaryFrame<'b>>],
    index: u64,
    guid: &str,
    version: u32,
) -> Result<&'a DesignPrimaryFrame<'b>, CodecError> {
    frames
        .iter()
        .flatten()
        .find(|frame| frame.entity_id == index && matching(frame, guid, version))
        .ok_or_else(|| {
            CodecError::malformed(
                "parameter state reference has no live target of its declared type",
            )
        })
}

fn validate_pair(
    source: View<'_>,
    frames: &[Option<DesignPrimaryFrame<'_>>],
    frame: &DesignPrimaryFrame<'_>,
) -> Result<(), CodecError> {
    let (state_frame, companion_frame, root_frame) = if matching(frame, STATE_GUID, 4) {
        let (_, _, other, root) = state(extent(source, frame)?, frame)?;
        (
            frame,
            target(frames, other, COMPANION_GUID, 1)?,
            target(frames, root, ROOT_GUID, 23)?,
        )
    } else {
        let (_, _, _, _, other) = companion(extent(source, frame)?, frame)?;
        let state_frame = target(frames, other, STATE_GUID, 4)?;
        let (_, _, companion_index, root) = state(extent(source, state_frame)?, state_frame)?;
        if companion_index != frame.entity_id {
            return Err(CodecError::malformed(
                "parameter state companion backlink is not reciprocal",
            ));
        }
        (state_frame, frame, target(frames, root, ROOT_GUID, 23)?)
    };
    let (_, _, _, _, backlink) = companion(extent(source, companion_frame)?, companion_frame)?;
    if backlink != state_frame.entity_id {
        return Err(CodecError::malformed(
            "parameter state backlink differs from the linked state",
        ));
    }
    header(extent(source, root_frame)?, root_frame)?;
    Ok(())
}

fn decode_stream(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    stream: &str,
    frames: &[Option<DesignPrimaryFrame<'_>>],
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    for frame in frames.iter().flatten().filter(|frame| {
        matching(frame, STATE_GUID, 4)
            || matching(frame, COMPANION_GUID, 1)
            || matching(frame, ROOT_GUID, 23)
    }) {
        let full = extent(source, frame)?;
        let root = matching(frame, ROOT_GUID, 23);
        let view = if root {
            at(full, 0, indexed::LEN)?
        } else {
            full
        };
        ctx.decode_part(output, full, PartSpec::new("f3d.bulk_record"), |stage| {
            if !root {
                validate_pair(source, frames, frame)?;
            }
            let id = identity(stream, frame);
            if root {
                let (class_tag, record_index) = header(view, frame)?;
                stage.insert_native(
                    ctx,
                    "design_parameter_document_headers",
                    DesignParameterDocumentHeader {
                        id: id.clone(),
                        class_tag,
                        record_index,
                    },
                )?;
            } else if matching(frame, STATE_GUID, 4) {
                let (class_tag, record_index, other, root) = state(view, frame)?;
                stage.insert_native(
                    ctx,
                    "design_parameter_states",
                    DesignParameterState {
                        id: id.clone(),
                        class_tag,
                        record_index,
                        companion: identity(stream, target(frames, other, COMPANION_GUID, 1)?),
                        document_root: identity(stream, target(frames, root, ROOT_GUID, 23)?),
                    },
                )?;
            } else {
                let (class_tag, record_index, control, control_word, state_record_index) =
                    companion(view, frame)?;
                stage.insert_native(
                    ctx,
                    "design_parameter_state_companions",
                    DesignParameterStateCompanion {
                        id: id.clone(),
                        class_tag,
                        record_index,
                        control,
                        control_word,
                        state_record_index,
                    },
                )?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Link each typed parameter to the state its tail reference names.
///
/// A link forms only when the named entity is a typed state that has a
/// current home.
pub(crate) fn link_parameters(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    parameters: &mut [crate::records::parameters::DesignParameter],
    states: &[DesignParameterState],
    targets: &BTreeMap<String, u64>,
) -> Result<(), CodecError> {
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        if !meta.types.iter().any(|ty| {
            ty.type_guid.as_str().eq_ignore_ascii_case(STATE_GUID)
                && ty.version == 4
                && ty.entities.values().len() != 0
        }) {
            continue;
        }
        let source = scan
            .entry_view(&entry.name)
            .ok_or_else(|| CodecError::malformed("parameter tail has no source View"))?;
        let frames = supported_frames(ctx, source, &meta)?;
        let primary = crate::metastream::primary_record_frames(&meta, source.window().len())?;
        for parameter in parameters.iter_mut() {
            let Some(offset) = parameter.byte_offset() else {
                continue;
            };
            if parameter.id != crate::ids::native_design_parameter_id(&entry.name, offset) {
                continue;
            }
            if !primary.iter().any(|frame| {
                u64_from_index(frame.start) == offset
                    && frame.entity_id == u64::from(parameter.record_index)
            }) {
                continue;
            }
            let Some(target) = targets.get(&parameter.id) else {
                continue;
            };
            if let Some(state) = linked_state(&entry.name, *target, &frames, states) {
                parameter.set_parameter_state(state);
            }
        }
    }
    Ok(())
}

/// The id of the typed state at local entity index `target`, when it has a
/// current home.
fn linked_state(
    stream: &str,
    target: u64,
    frames: &[Option<DesignPrimaryFrame<'_>>],
    states: &[DesignParameterState],
) -> Option<String> {
    let state_frame = frames
        .iter()
        .flatten()
        .find(|frame| frame.entity_id == target && matching(frame, STATE_GUID, 4))?;
    let state = identity(stream, state_frame);
    states
        .iter()
        .any(|candidate| candidate.id == state)
        .then_some(state)
}

#[cfg(test)]
mod tests;
