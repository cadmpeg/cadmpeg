// SPDX-License-Identifier: Apache-2.0
//! The design arenas of a Fusion document read from CADIR JSON, restored
//! from the decode of its retained archive and the edits its `design`
//! records state.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::saved::{restore_history, saved_source, HistoryText};
use cadmpeg_ir::codec::write::target::ResolvedWrite;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::design::expr::ExpressionDialect;
use cadmpeg_ir::design::units::Unit;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::parameters::DesignParameter;

use crate::{ids, F3dCodec};

/// `input`'s document with the arenas the CADIR wire omits restored from
/// its retained archive, where it was read from CADIR JSON and `target` does
/// not replay it unedited. A restore that the design edits refuse leaves the
/// document as read, which the generator writes from its `design` records,
/// and adds the refusal to `notes`.
pub(crate) fn restore(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
    notes: &mut Vec<String>,
) -> Result<Option<CadIr>, CodecError> {
    let replays = target.entry().is_none() || target.preserves_source();
    let Some(image) = saved_source(input, ids::FILE_SOURCE_IMAGE_ID, replays)? else {
        return Ok(None);
    };
    let restored = F3dCodec
        .decode(&mut Cursor::new(image), &DecodeOptions::default())
        .map_err(|failure| match failure {
            DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format!(
                "the retained F3D archive cannot be decoded: {other}"
            )),
        })
        .and_then(|decoded| restore_history(input.ir, decoded.ir(), &FusionText));
    match restored {
        Ok(restored) if restored.unwritten.is_empty() => Ok(Some(restored.ir)),
        Ok(restored) => {
            notes.push(format!(
                "the document was read from CADIR, and its history arenas cannot state these \
                 design edits: {}",
                restored.unwritten.join("; ")
            ));
            Ok(None)
        }
        Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => Err(error),
        Err(error) => {
            notes.push(error.to_string());
            Ok(None)
        }
    }
}

/// The text of a Fusion parameter expression.
struct FusionText;

impl HistoryText for FusionText {
    fn dialect(&self) -> ExpressionDialect {
        ExpressionDialect::Fusion
    }

    fn literal(&self, value: f64, unit: &Unit, _stored: &str) -> Option<String> {
        match unit.0.as_slice() {
            [] => Some(format!("{value}")),
            [factor] if factor.power.get() == 1 => {
                Some(format!("{value} {}", factor.symbol.name()))
            }
            _ => None,
        }
    }

    fn reference(&self, target: &DesignParameter, _model: &Model) -> Option<String> {
        Some(target.name.clone())
    }
}

#[cfg(test)]
mod tests;
