// SPDX-License-Identifier: Apache-2.0
//! The design arenas of a Fusion document read from CADIR JSON, restored
//! from the decode of its retained archive and the edits its `design`
//! records state. The rows of the configuration tables are the ones the
//! retained archive projects; the writer writes the configurations of the
//! `design` records into them.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::saved::{restore_history, saved_source, HistoryText};
use cadmpeg_ir::codec::write::target::ResolvedWrite;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::design::expr::ExpressionDialect;
use cadmpeg_ir::design::units::Unit;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::parameters::{DesignConfiguration, DesignParameter};
use cadmpeg_ir::report::loss::LossNote;

use super::primitives::{configuration_targets, decoded_configurations, f3d_native};
use crate::ids;
use crate::loss::F3dLossCode;

/// `input`'s document with the arenas the CADIR wire omits restored from
/// its retained archive, where it was read from CADIR JSON and `target` does
/// not replay it unedited. A restore that the design edits refuse, or that
/// fails for a reason other than a limit, leaves the document as read, which
/// the generator writes from its `design` records, adds the reason to
/// `notes`, and charges it as `f3d:writer.history-unrestored` to `losses`.
/// A restored document comes with the decode of the archive, which the patch
/// compares it with.
pub(crate) fn restore<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'a>,
    input: EncodeInput<'a>,
    target: &ResolvedWrite<'_>,
    notes: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<(CadIr, super::RetainedArchive<'a>)>, CodecError> {
    let replays = target.entry().is_none() || target.preserves_source();
    let Some(image) = saved_source(ctx, input, ids::FILE_SOURCE_IMAGE_ID, replays)? else {
        return Ok(None);
    };
    let restored = super::RetainedArchive::read(ctx, image).and_then(|source| {
        let mut restored = restore_history(input.ir, source.decoded.ir(), &FusionText)?;
        restored.ir.model.configurations = reproject(ctx, &restored.ir)?;
        Ok((restored, source))
    });
    match restored {
        Ok((restored, source)) if restored.unwritten.is_empty() => Ok(Some((restored.ir, source))),
        Ok((restored, _)) => {
            let reason = format!(
                "the document was read from CADIR, and its history arenas cannot state these \
                 design edits: {}",
                restored.unwritten.join("; ")
            );
            unrestored(reason, notes, losses);
            Ok(None)
        }
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            unrestored(error.to_string(), notes, losses);
            Ok(None)
        }
    }
}

/// The rows of the configuration tables `ir` was read with, as the decoder
/// states them.
fn reproject(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<DesignConfiguration>, CodecError> {
    Ok(match f3d_native(ctx, ir)? {
        Some(native) => {
            decoded_configurations(
                &native.design_configurations,
                &configuration_targets(ir, &native),
            )?
            .configurations
        }
        None => Vec::new(),
    })
}

/// Records and charges why the history arenas were not restored.
fn unrestored(reason: String, notes: &mut Vec<String>, losses: &mut Vec<LossNote>) {
    losses.push(F3dLossCode::WriterHistoryUnrestored.note(format!(
        "the history arenas were not restored from the retained F3D archive ({reason}); \
         the file is written from the design records"
    )));
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

    fn restates_configurations(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
