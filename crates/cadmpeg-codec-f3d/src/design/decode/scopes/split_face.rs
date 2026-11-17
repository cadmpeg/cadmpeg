use super::parameter_scope::payload_prologue;
use super::point_direction::{
    point_direction_carrier_at, PointDirectionBoundary, PointDirectionCarrier,
};
use crate::bytes::utf16::Utf16View;
use crate::bytes::{take_reference_lossless, Reference};
use crate::design::decode::meta::DesignPrimaryFrame;
use crate::layout::split_face_method_flags as flags;
use crate::layout::split_face_scope_prefix as prefix;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::split_face::{
    DesignSplitFaceCarrierFrame, DesignSplitFaceDirectionReference, DesignSplitFaceMethodControl,
    DesignSplitFaceOperation,
};
use crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn split_face_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    frames: &[DesignPrimaryFrame<'_>],
) -> Result<Option<DesignSplitFaceOperation>, CodecError> {
    if scope.kind() != DesignFeatureKind::SplitFace {
        return Ok(None);
    }
    let Some((body, prologue_at, zero_start)) = (|| {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let end = usize::try_from(scope.reference_count_offset()).ok()?;
        Some((
            bytes.get(..end)?,
            start.checked_add(prefix::LEN)?,
            start.checked_add(prefix::ZERO_RUN)?,
        ))
    })() else {
        return Ok(None);
    };
    let end = body.len();
    if prologue_at > end {
        return Ok(None);
    }
    if body
        .get(zero_start..prologue_at)
        .is_none_or(|zeroes| zeroes.iter().any(|byte| *byte != 0))
        || body.get(prologue_at) != Some(&0)
    {
        return Ok(Some(DesignSplitFaceOperation::unresolved(u64_from_index(
            prologue_at,
        ))));
    }
    let Some(mut cursor) = payload_prologue(body, prologue_at, end) else {
        return Ok(Some(DesignSplitFaceOperation::unresolved(u64_from_index(
            prologue_at,
        ))));
    };
    let direction_reference_at = cursor;
    let Some(reference) = take_reference_lossless(body, &mut cursor) else {
        return Ok(Some(DesignSplitFaceOperation::unresolved(u64_from_index(
            direction_reference_at,
        ))));
    };
    let direction_reference =
        split_face_direction_reference(ctx, &reference, direction_reference_at)?;
    let method_offset = u64_from_index(cursor);
    let control = read_method_control(body, cursor);
    let carrier = direction_carrier(ctx, bytes, &direction_reference, frames)?;
    let mut operation = DesignSplitFaceOperation {
        method_offset,
        control,
        direction_reference,
        carrier_frame: None,
        direction: None,
        inputs: Vec::new(),
        input_count: None,
        inputs_complete: false,
    };
    if let Some((carrier, frame)) = carrier {
        operation.carrier_frame = Some(frame);
        operation.direction = Some(Located {
            value: carrier.direction,
            offset: u64_from_index(carrier.direction_at),
        });
        operation.input_count = carrier.input_count;
        operation.inputs_complete = carrier.inputs_complete;
        let retained = carrier
            .inputs
            .len()
            .checked_mul(std::mem::size_of::<
                Located<DesignSplitFaceDirectionReference>,
            >())
            .and_then(|size| u64::try_from(size).ok())
            .ok_or_else(|| CodecError::Malformed("SplitFace retained inputs overflow".into()))?;
        ctx.retain_bytes(retained, "SplitFace carrier inputs")?;
        let mut inputs = ctx.alloc_filled(
            carrier.inputs.len(),
            Located {
                value: DesignSplitFaceDirectionReference::Unavailable,
                offset: 0,
            },
            "SplitFace carrier inputs",
        )?;
        for (slot, input) in inputs.iter_mut().zip(carrier.inputs) {
            let at = usize::try_from(input.offset).map_err(|_| {
                CodecError::Malformed("SplitFace reference offset exceeds usize".into())
            })?;
            *slot = Located {
                value: split_face_direction_reference(ctx, &input.value, at)?,
                offset: input.offset,
            };
        }
        operation.inputs = inputs;
    }
    Ok(Some(operation))
}

fn read_method_control(bytes: &[u8], at: usize) -> Option<DesignSplitFaceMethodControl> {
    Some(DesignSplitFaceMethodControl {
        family: View::u32_le_at(bytes, at.checked_add(flags::FAMILY)?)?,
        projection: *bytes.get(at.checked_add(flags::PROJECTION)?)?,
        extension: *bytes.get(at.checked_add(flags::EXTENSION)?)?,
    })
}

fn split_face_direction_reference(
    ctx: &DecodeContext<'_>,
    reference: &Reference<&str, Utf16View<'_>>,
    at: usize,
) -> Result<DesignSplitFaceDirectionReference, CodecError> {
    if matches!(reference, Reference::Null) {
        return Ok(DesignSplitFaceDirectionReference::Null);
    }
    let target_at = at
        .checked_add(1)
        .map(u64_from_index)
        .ok_or_else(|| CodecError::Malformed("SplitFace reference offset overflows".into()))?;
    let target = |value| Located {
        value,
        offset: target_at,
    };
    let guid = |text: &str| copy_reference_text(ctx, text.len(), text.chars());
    let text = |value: Utf16View<'_>| copy_reference_text(ctx, value.len(), value.chars());
    Ok(match reference {
        Reference::Null => DesignSplitFaceDirectionReference::Null,
        Reference::Local {
            target: value,
            inline_type_guid,
        } => DesignSplitFaceDirectionReference::Local {
            target: target(*value),
            inline_type_guid: inline_type_guid.map(guid).transpose()?,
        },
        Reference::CrossSegment {
            target: value,
            inline_type_guid,
            segment,
            asset_guid,
        } => DesignSplitFaceDirectionReference::CrossSegment {
            target: target(*value),
            inline_type_guid: inline_type_guid.map(guid).transpose()?,
            segment: *segment,
            same_document_asset: asset_guid.map(text).transpose()?,
        },
        Reference::CrossDocument {
            target: value,
            inline_type_guid,
            segment,
            asset_guid,
            target_type_guid,
            link_name,
            version,
        } => DesignSplitFaceDirectionReference::CrossDocument {
            target: target(*value),
            inline_type_guid: inline_type_guid.map(guid).transpose()?,
            segment: *segment,
            asset_guid: text(*asset_guid)?,
            target_type_guid: guid(target_type_guid.or(*inline_type_guid).ok_or_else(|| {
                CodecError::Malformed("SplitFace document reference has no target type".into())
            })?)?,
            external_type_guid_present: target_type_guid.is_some(),
            link_name: text(*link_name)?,
            version: version
                .map(|(key, value)| Ok::<_, CodecError>((text(key)?, text(value)?)))
                .transpose()?,
        },
    })
}

fn copy_reference_text(
    ctx: &DecodeContext<'_>,
    length: usize,
    chars: impl Iterator<Item = char>,
) -> Result<String, CodecError> {
    ctx.retain_bytes(u64_from_index(length), "SplitFace reference text")?;
    let mut bytes = ctx.alloc_filled(length, 0u8, "SplitFace reference text")?;
    let mut at = 0;
    for character in chars {
        let mut encoded = [0u8; 4];
        let text = character.encode_utf8(&mut encoded);
        bytes[at..at + text.len()].copy_from_slice(text.as_bytes());
        at += text.len();
    }
    String::from_utf8(bytes).map_err(|error| CodecError::Malformed(error.to_string()))
}

fn direction_carrier<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    reference: &DesignSplitFaceDirectionReference,
    frames: &[DesignPrimaryFrame<'_>],
) -> Result<Option<(PointDirectionCarrier<'a>, DesignSplitFaceCarrierFrame)>, CodecError> {
    let DesignSplitFaceDirectionReference::Local { target, .. } = reference else {
        return Ok(None);
    };
    let Ok(record_index) = u32::try_from(target.value) else {
        return Ok(None);
    };
    if record_index == 0 {
        return Ok(None);
    }
    let mut matching = frames
        .iter()
        .filter(|frame| frame.entity_id == target.value);
    let (Some(frame), None) = (matching.next(), matching.next()) else {
        return Ok(None);
    };
    if frame.member_end >= frame.end
        || !frame
            .design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(POINT_DIRECTION_TYPE_GUID)
    {
        return Ok(None);
    }
    let carrier = point_direction_carrier_at(
        ctx,
        bytes,
        frame.start,
        PointDirectionBoundary::Registered(frame.member_end),
        record_index,
        frame.design_type.version,
    )?;
    Ok(carrier.map(|carrier| {
        (
            carrier,
            DesignSplitFaceCarrierFrame {
                primary_offset: u64_from_index(frame.start),
                secondary_offset: u64_from_index(frame.member_end),
                end_offset: u64_from_index(frame.end),
                version: frame.design_type.version,
            },
        )
    }))
}

#[cfg(test)]
mod tests;
