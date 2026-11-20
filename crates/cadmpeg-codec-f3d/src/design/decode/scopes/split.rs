// SPDX-License-Identifier: Apache-2.0

use super::parameter_scope::read_payload_prologue;
use super::shared_frames::marked_record_reference;
use crate::layout::{split_body_extension as extension, split_body_scope_prefix as prefix};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::report::loss::LossNote;

pub(super) fn decode_split_tool_extension(
    bytes: &[u8],
    scope: &mut DesignParameterScope,
    losses: &mut Vec<LossNote>,
) {
    let start = scope.byte_offset();
    let end = scope.reference_count_offset();
    let DesignScopePayloadMut::Split(payload) = scope.payload_mut() else {
        return;
    };
    let extension = read_extension(bytes, start, end);
    payload.extend_tool = extension.map(|(value, _)| value);
    match extension {
        Some((value, property_block_present)) => {
            if !property_block_present {
                losses.push(F3dLossCode::FeatureSplitToolExtensionAssumed.note(format!(
                    "Split scope {} reads the extension Boolean at byte {} in the absent-property form (DR-877).",
                    scope.id, value.offset
                )));
            }
        }
        None => {
            losses.push(
                F3dLossCode::FeatureSplitToolExtensionUnresolved.note(format!(
                    "Split scope {} has no admitted splitting-tool extension control (DR-877).",
                    scope.id
                )),
            );
        }
    }
}

fn read_extension(bytes: &[u8], start: u64, end: u64) -> Option<(Located<bool>, bool)> {
    let frame = bytes.get(usize::try_from(start).ok()?..usize::try_from(end).ok()?)?;
    let mut view = View::over_retained(frame);
    view.seek(prefix::ZERO_RUN)?;
    if view.take(prefix::LEADING_BLOCK_PRESENCE - prefix::ZERO_RUN)? != [0; 8]
        || view.u8()? != prefix::LEADING_BLOCK_PRESENCE_VALUE
    {
        return None;
    }
    let property_block_present = view.u8()? == 1;
    let prologue = read_payload_prologue(frame, prefix::LEADING_BLOCK_PRESENCE, frame.len())?;
    if prologue.property_count != 0 {
        return None;
    }
    let at = prologue.end.checked_add(extension::EXTEND_TOOL)?;
    view.seek(at)?;
    let value = match view.u8()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let word = View::u32_le_at(frame, prologue.end.checked_add(extension::FOLLOWING_WORD)?)?;
    if word != extension::FOLLOWING_WORD_VALUE {
        return None;
    }
    let reference = prologue.end.checked_add(extension::LEN)?;
    match frame.get(reference)? {
        0 => {}
        1 if marked_record_reference(frame, reference).is_some_and(|target| target != 0) => {}
        _ => return None,
    }
    let offset = start.checked_add(u64_from_index(at))?;
    Some((Located { value, offset }, property_block_present))
}

#[cfg(test)]
mod tests;
