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
use cadmpeg_ir::report::loss::LossNote;

use super::primitives::{configuration_targets, decoded_configurations, f3d_native};
use crate::loss::F3dLossCode;
use crate::{ids, F3dCodec};

/// `input`'s document with the arenas the CADIR wire omits restored from
/// its retained archive, where it was read from CADIR JSON and `target` does
/// not replay it unedited. A restore that the design edits refuse, or that
/// fails for a reason other than a limit, leaves the document as read, which
/// the generator writes from its `design` records, and adds the reason to
/// `notes`. Where `target` does not replay the archive, the restore also
/// charges the reason as `f3d:writer.history-unrestored` to `losses`; where it
/// does, the patch of the same archive charges it.
pub(crate) fn restore(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
    notes: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
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
        .and_then(|decoded| restore_history(input.ir, decoded.ir(), &FusionText))
        .and_then(|mut restored| {
            reproject(&mut restored.ir)?;
            Ok(restored)
        });
    match restored {
        Ok(restored) if restored.unwritten.is_empty() => Ok(Some(restored.ir)),
        Ok(restored) => {
            let reason = format!(
                "the document was read from CADIR, and its history arenas cannot state these \
                 design edits: {}",
                restored.unwritten.join("; ")
            );
            unrestored(reason, replays, notes, losses);
            Ok(None)
        }
        Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => Err(error),
        Err(error) => {
            unrestored(error.to_string(), replays, notes, losses);
            Ok(None)
        }
    }
}

/// Replaces the configurations `ir` took from the archive's decode with the
/// ones the decoder states for the configuration tables `ir` was read with.
fn reproject(ir: &mut CadIr) -> Result<(), CodecError> {
    ir.model.configurations = match f3d_native(ir)? {
        Some(native) => {
            decoded_configurations(
                &native.design_configurations,
                &configuration_targets(ir, &native),
            )?
            .configurations
        }
        None => Vec::new(),
    };
    Ok(())
}

/// Records why the history arenas were not restored, and charges it where
/// the target does not replay the retained archive.
fn unrestored(reason: String, replays: bool, notes: &mut Vec<String>, losses: &mut Vec<LossNote>) {
    if !replays {
        losses.push(F3dLossCode::WriterHistoryUnrestored.note(format!(
            "the history arenas were not restored from the retained F3D archive ({reason}); \
             the file is written from the design records"
        )));
    }
    notes.push(reason);
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
