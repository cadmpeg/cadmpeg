// SPDX-License-Identifier: Apache-2.0
//! The design arenas of a Fusion document read from CADIR JSON, restored
//! from the decode of its retained archive and the edits its `design`
//! records state. The configurations of the `design` records are written
//! into the rows of the configuration tables.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::saved::{restore_history, saved_source, HistoryText};
use cadmpeg_ir::codec::write::target::ResolvedWrite;
use cadmpeg_ir::codec::write::{edit_dropped, EncodeInput};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::design::expr::ExpressionDialect;
use cadmpeg_ir::design::units::Unit;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::parameters::DesignParameter;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntityKind;

use super::primitives::{configuration_targets, decoded_configurations, f3d_native};
use crate::design_v11::configurations::restate;
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
        .and_then(|decoded| {
            let edited = configurations_edited(input.ir, decoded.ir());
            Ok((
                restore_history(input.ir, decoded.ir(), &FusionText)?,
                edited,
            ))
        })
        .and_then(|(mut restored, edited)| {
            let unstated = reproject(&mut restored.ir, edited)?;
            Ok((restored, unstated))
        });
    match restored {
        Ok((restored, unstated)) if restored.unwritten.is_empty() => {
            losses.extend(edit_dropped(
                EntityKind::DesignConfiguration,
                "F3D",
                "drops these configuration edits, which a row of the configuration table cannot state",
                unstated,
            ));
            Ok(Some(restored.ir))
        }
        Ok((restored, _)) => {
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

/// Whether the design configurations of `read`, or the configurations its
/// design scopes state active, differ from those of `decoded`.
fn configurations_edited(read: &CadIr, decoded: &CadIr) -> bool {
    let active = |ir: &CadIr| {
        ir.design
            .scopes
            .iter()
            .map(|scope| (scope.definition.clone(), scope.active_configuration.clone()))
            .collect::<Vec<_>>()
    };
    read.design.configurations != decoded.design.configurations || active(read) != active(decoded)
}

/// Replaces the configurations `ir` took from the archive's decode with the
/// ones the decoder states for the configuration tables `ir` was read with.
/// Where `edited` holds, the configurations of its `design` records are
/// written in. Returns each field of those that no row states.
fn reproject(ir: &mut CadIr, edited: bool) -> Result<Vec<String>, CodecError> {
    let base = match f3d_native(ir)? {
        Some(native) => {
            decoded_configurations(
                &native.design_configurations,
                &configuration_targets(ir, &native),
            )?
            .configurations
        }
        None => Vec::new(),
    };
    if !edited {
        ir.model.configurations = base;
        return Ok(Vec::new());
    }
    let restated = restate(
        &base,
        &ir.design,
        &ir.model.product_definitions,
        &ir.model.features,
    );
    ir.model.configurations = restated.rows;
    Ok(restated.unstated)
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

    fn restates_configurations(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
