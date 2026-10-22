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
    source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec},
    CodecFormat,
};

use std::mem::size_of;

const STATE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
const COMPANION_GUID: &str = "E276745D-D84E-40D5-88BC-8C94DD4ACCDF";
const ROOT_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";

/// Staged typed records and the exact failed-part homes.
#[derive(Debug, Default)]
pub(crate) struct DecodedStateParts {
    pub(crate) states: Vec<DesignParameterState>,
    pub(crate) companions: Vec<DesignParameterStateCompanion>,
    pub(crate) headers: Vec<DesignParameterDocumentHeader>,
    pub(crate) unknowns: Vec<cadmpeg_ir::native::NativeRecord>,
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
    let (_, mut native, fidelity, losses) = output.into_parts(ctx)?.into_parts();
    let namespace = native.namespace_mut("f3d");
    let states = namespace.arena_as_for_decode(ctx, "design_parameter_states")?;
    let companions = namespace.arena_as_for_decode(ctx, "design_parameter_state_companions")?;
    let headers = namespace.arena_as_for_decode(ctx, "design_parameter_document_headers")?;
    let unknowns = namespace
        .arenas_mut()
        .remove("unknowns")
        .unwrap_or_default();
    Ok(DecodedStateParts {
        states,
        companions,
        headers,
        unknowns,
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
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    frames: &[Option<DesignPrimaryFrame<'_>>],
    frame: &DesignPrimaryFrame<'_>,
) -> Result<(), CodecError> {
    let _reads = ctx.read_transaction()?;
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
    let level = PartLevel::parse("f3d.bulk_record")
        .ok_or_else(|| CodecError::malformed("F3D bulk record level is not declared"))?;
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
        ctx.decode_part(output, full, PartSpec::new(level, Vec::new()), |stage| {
            if !root {
                validate_pair(ctx, source, frames, frame)?;
            }
            let id = identity(stream, frame);
            ctx.record(&id, view, || {
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
                    let (class_tag, record_index, control, control_word, other) =
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
                            state: identity(stream, target(frames, other, STATE_GUID, 4)?),
                        },
                    )?;
                }
                Ok(())
            })
        })?;
    }
    Ok(())
}

/// Read the actual tail link only after its typed state has a current home.
pub(crate) fn link_parameters(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    parameters: &mut [crate::records::parameters::DesignParameter],
    states: &[DesignParameterState],
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
            let Some(physical) = primary.iter().find(|frame| {
                u64_from_index(frame.start) == offset
                    && frame.entity_id == u64::from(parameter.record_index)
            }) else {
                continue;
            };
            let start = source
                .start()
                .checked_add(physical.start)
                .ok_or_else(|| CodecError::malformed("parameter source start overflows"))?;
            let end = source
                .start()
                .checked_add(physical.end)
                .ok_or_else(|| CodecError::malformed("parameter source end overflows"))?;
            let view = source.child(start, end).ok_or_else(|| {
                CodecError::malformed("parameter primary extent exceeds its source")
            })?;
            capture_tail(ctx, view, &entry.name, parameter, &frames, states)?;
        }
    }
    Ok(())
}

fn capture_tail(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    stream: &str,
    parameter: &mut crate::records::parameters::DesignParameter,
    frames: &[Option<DesignPrimaryFrame<'_>>],
    states: &[DesignParameterState],
) -> Result<(), CodecError> {
    let Some(tail) = super::parameters::typed_parameter_tail_start(view.window()) else {
        return Ok(());
    };
    let target_offset = tail
        .checked_add(2)
        .ok_or_else(|| CodecError::malformed("parameter tail target offset overflows"))?;
    let probe = ctx.read_transaction()?;
    let target_index = at(view, target_offset, 8)?.req_u64_le()?;
    drop(probe);
    let Some(state_frame) = frames
        .iter()
        .flatten()
        .find(|frame| frame.entity_id == target_index && matching(frame, STATE_GUID, 4))
    else {
        return Ok(());
    };
    let state_id = identity(stream, state_frame);
    if !states.iter().any(|state| state.id == state_id) {
        return Ok(());
    }
    let actual_state = ctx.record(&parameter.id, view, || {
        let actual = at(view, target_offset, 8)?.req_u64_le()?;
        Ok(identity(stream, target(frames, actual, STATE_GUID, 4)?))
    })?;
    parameter.set_parameter_state(actual_state);
    Ok(())
}

pub(crate) fn install_unknowns(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    records: Vec<cadmpeg_ir::native::NativeRecord>,
) -> Result<(), CodecError> {
    if records.is_empty() {
        return Ok(());
    }
    match ir
        .native
        .namespace_mut("f3d")
        .arenas_mut()
        .entry("unknowns".to_owned())
    {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(records);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let destination = entry.get_mut();
            let required = destination
                .len()
                .checked_add(records.len())
                .ok_or_else(|| {
                    CodecError::malformed("parameter state unknown composition count overflows")
                })?;
            if required > destination.capacity() {
                let bytes = required
                    .abs_diff(destination.capacity())
                    .checked_mul(size_of::<cadmpeg_ir::native::NativeRecord>())
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "parameter state unknown composition storage overflows",
                        )
                    })?;
                ctx.retain_bytes(u64_from_index(bytes), "parameter state unknown composition")?;
                destination.try_reserve_exact(records.len()).map_err(|_| {
                    cadmpeg_core::decode::ResourceLimit::allocation_failed(
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                        ctx.policy().limits.max_retained_bytes,
                        u64_from_index(bytes),
                        "parameter state unknown composition",
                    )
                })?;
            }
            destination.extend(records);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
