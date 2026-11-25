// SPDX-License-Identifier: Apache-2.0
//! The design arenas of a Fusion document read from CADIR JSON, restored
//! from the decode of its retained archive and the edits its `design`
//! records state.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::saved::{restore_history, HistoryText};
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::design::expr::ExpressionDialect;
use cadmpeg_ir::design::units::Unit;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::parameters::DesignParameter;

use crate::{ids, F3dCodec};

/// `input`'s document with the arenas the CADIR wire omits restored from its
/// retained archive, where it was read from CADIR JSON and the write reads
/// them: the write patches an edit, or synthesizes. `None` where the
/// document holds them, has no retained archive, or replays it.
pub(super) fn restore(
    input: EncodeInput<'_>,
    preserving: bool,
) -> Result<Option<CadIr>, CodecError> {
    if input.ir.model.omitted_arenas_held() {
        return Ok(None);
    }
    let Some((fidelity, image)) = input.fidelity.and_then(|fidelity| {
        let image = fidelity
            .retained_record(ids::FILE_SOURCE_IMAGE_ID)?
            .data()?;
        Some((fidelity, image))
    }) else {
        return Ok(None);
    };
    if preserving && !fidelity.edited(input.ir)? {
        return Ok(None);
    }
    let decoded = F3dCodec
        .decode(&mut Cursor::new(image), &DecodeOptions::default())
        .map_err(|failure| match failure {
            DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format!(
                "the retained F3D archive cannot be decoded: {other}"
            )),
        })?;
    restore_history(input.ir, decoded.ir(), &FusionText).map(Some)
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
