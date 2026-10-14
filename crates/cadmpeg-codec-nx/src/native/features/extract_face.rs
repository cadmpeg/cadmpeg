// SPDX-License-Identifier: Apache-2.0
//! Exact `EXTRACT_FACE` construction lanes with checked wire admission.

use super::{format_feature_history_id, visit_feature_history_operation_records};
use crate::container::Container;
use crate::om::extract_face::{ExtractFaceEnvelope, ExtractFaceLane, PARAMETER_CLASS};
use crate::om::reference_index::FeatureReferenceToken;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU8;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "EnvelopeWire")]
pub(in crate::native) struct FeatureExtractFaceConstructionEnvelope {
    id: String,
    operation_label: String,
    field: ExtractFaceEnvelope,
    source_offset: u64,
}

#[derive(Deserialize)]
struct EnvelopeWire {
    id: String,
    operation_label: String,
    parameter_class_index: NonZeroU8,
    control_words: [u32; 4],
    lanes: [ExtractFaceLane; 2],
    terminal: FeatureReferenceToken,
    navigator_text: String,
    byte_len: u64,
    source_offset: u64,
}

impl TryFrom<EnvelopeWire> for FeatureExtractFaceConstructionEnvelope {
    type Error = &'static str;

    fn try_from(wire: EnvelopeWire) -> Result<Self, Self::Error> {
        let field = ExtractFaceEnvelope::new(
            wire.parameter_class_index,
            wire.control_words,
            wire.lanes,
            wire.terminal,
            wire.navigator_text,
        )?;
        if wire.byte_len != u64_from_index(field.byte_len())
            || wire.source_offset.checked_add(wire.byte_len).is_none()
        {
            return Err(
                "byte_len/source_offset must describe the complete bounded EXTRACT_FACE envelope",
            );
        }
        Ok(Self {
            id: wire.id,
            operation_label: wire.operation_label,
            field,
            source_offset: wire.source_offset,
        })
    }
}

impl Serialize for FeatureExtractFaceConstructionEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct EnvelopeRef<'a> {
            id: &'a str,
            operation_label: &'a str,
            parameter_class_index: NonZeroU8,
            control_words: [u32; 4],
            lanes: &'a [ExtractFaceLane; 2],
            terminal: &'a FeatureReferenceToken,
            navigator_text: &'a str,
            byte_len: u64,
            source_offset: u64,
        }
        EnvelopeRef {
            id: &self.id,
            operation_label: &self.operation_label,
            parameter_class_index: self.field.parameter_class_index,
            control_words: self.field.control_words,
            lanes: &self.field.lanes,
            terminal: &self.field.terminal,
            navigator_text: self.field.navigator_text(),
            byte_len: u64_from_index(self.field.byte_len()),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

pub(in crate::native) fn feature_extract_face_construction_envelopes(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<FeatureExtractFaceConstructionEnvelope>, CodecError> {
    let mut envelopes = Vec::new();
    let mut failure = None;
    visit_feature_history_operation_records(
        ctx,
        container,
        |section, section_key, entry_offset, operation_ordinal, record| {
            if failure.is_some() {
                return;
            }
            let projected = (|| -> Result<Option<_>, CodecError> {
                let Some(field) = ExtractFaceEnvelope::read(ctx, record.payload_view())? else {
                    return Ok(None);
                };
                ctx.charge_work(
                    u64_from_index(PARAMETER_CLASS.len()),
                    "resolve NX EXTRACT_FACE parameter class",
                )?;
                if section
                    .types
                    .get(usize::from(field.parameter_class_index.get()) - 1)
                    .is_none_or(|class| class.name != PARAMETER_CLASS)
                {
                    return Ok(None);
                }
                let Some(source_offset) = entry_offset
                    .checked_add(u64_from_index(record.payload_offset()))
                    .filter(|offset| {
                        offset
                            .checked_add(u64_from_index(field.byte_len()))
                            .is_some()
                    })
                else {
                    return Ok(None);
                };
                let envelope = FeatureExtractFaceConstructionEnvelope {
                    id: format_feature_history_id(
                        ctx,
                        "extract-face-construction-envelope",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    operation_label: format_feature_history_id(
                        ctx,
                        "operation-label",
                        section_key,
                        operation_ordinal,
                        None,
                    )?,
                    field,
                    source_offset,
                };
                ctx.reserve_vec(&mut envelopes, 1, "NX EXTRACT_FACE construction envelopes")?;
                Ok(Some(envelope))
            })();
            match projected {
                Ok(Some(envelope)) => envelopes.push(envelope),
                Ok(None) => {}
                Err(error) => failure = Some(error),
            }
        },
    )?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(envelopes)
}

#[cfg(test)]
mod tests;
