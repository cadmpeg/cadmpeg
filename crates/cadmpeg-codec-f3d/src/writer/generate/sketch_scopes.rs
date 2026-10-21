// SPDX-License-Identifier: Apache-2.0
//! Sketch scopes and placements authored from typed values.

use cadmpeg_core::CodecError;

use crate::metastream::RecordIndexEntry;
use crate::native::F3dNative;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::sketch_placement::DesignSketchPlacement;

use super::records::{native_lp_ascii, native_lp_utf16};

pub(super) fn encode(
    out: &mut Vec<u8>,
    primary: &mut Vec<RecordIndexEntry>,
    native: &F3dNative,
) -> Result<(), CodecError> {
    for scope in &native.design_parameter_scopes {
        primary.push(index(scope.record_index, out.len())?);
        encode_scope(out, scope)?;
    }
    for placement in &native.design_sketch_placements {
        primary.push(index(placement.record_index, out.len())?);
        encode_placement(out, placement)?;
    }
    Ok(())
}

fn index(record_index: u32, offset: usize) -> Result<RecordIndexEntry, CodecError> {
    Ok(RecordIndexEntry {
        entity_id: u64::from(record_index),
        bulk_offset: u64::try_from(offset).map_err(|_| {
            CodecError::InvalidInput("generated Sketch record offset exceeds u64".into())
        })?,
    })
}

fn header(out: &mut Vec<u8>, tag: &str, index: u32) -> Result<(), CodecError> {
    native_lp_ascii(out, tag)?;
    out.extend_from_slice(&index.to_le_bytes());
    Ok(())
}

fn encode_scope(out: &mut Vec<u8>, scope: &DesignParameterScope) -> Result<(), CodecError> {
    if crate::design::design_feature_family(&scope.kind())
        != Some(crate::design::DesignFeatureFamily::Sketch)
    {
        return Err(CodecError::UnsupportedContent(
            "generated Sketch scopes require a Sketch feature family".into(),
        ));
    }
    header(out, scope.class_tag.as_str(), scope.record_index)?;
    out.extend_from_slice(&[0; 10]);
    let count = u32::try_from(scope.reference_members().len())
        .map_err(|_| CodecError::InvalidInput("Sketch scope reference count exceeds u32".into()))?;
    out.extend_from_slice(&count.to_le_bytes());
    for reference in scope.reference_members().values() {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
    }
    out.extend_from_slice(&state(scope.history_state_id())?.to_le_bytes());
    native_lp_utf16(out, scope.kind_name())?;
    let mut tail = [0; 78];
    tail[..4].copy_from_slice(&scope.feature_ordinal.get().to_le_bytes());
    tail[31..35].copy_from_slice(&state(scope.previous_history_state_id())?.to_le_bytes());
    out.extend_from_slice(&tail);
    header(out, scope.paired_class_tag.as_str(), scope.record_index)
}

fn state(value: Option<i64>) -> Result<u32, CodecError> {
    match value {
        None => Ok(u32::MAX),
        Some(value) => u32::try_from(value)
            .ok()
            .filter(|value| *value != u32::MAX)
            .ok_or_else(|| {
                CodecError::InvalidInput("Sketch scope state exceeds the encoded u32 domain".into())
            }),
    }
}

fn encode_placement(
    out: &mut Vec<u8>,
    placement: &DesignSketchPlacement,
) -> Result<(), CodecError> {
    if placement.member_run_head() {
        return Err(CodecError::UnsupportedContent(
            "generated scope placements require a scope-owned Sketch frame".into(),
        ));
    }
    let start = out.len();
    header(out, placement.class_tag.as_str(), placement.record_index)?;
    out.resize(start + 55, 0);
    for value in placement.frame.canonical_transform().iter().flatten() {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.resize(start + 329, 0);
    header(
        out,
        placement.paired_class_tag.as_str(),
        placement.record_index,
    )
}
