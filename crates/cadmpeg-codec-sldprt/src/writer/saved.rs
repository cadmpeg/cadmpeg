// SPDX-License-Identifier: Apache-2.0
//! The feature history of a part read from CADIR JSON, restored from the
//! decode of its retained source and the edits its `design` records state.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::saved::{restore_history, HistoryText};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::design::expr::ExpressionDialect;
use cadmpeg_ir::design::units::Unit;
use cadmpeg_ir::document::{CadIr, Model};
use cadmpeg_ir::features::parameters::DesignParameter;

use crate::design_v11::parameters::LITERAL_UNITS;
use crate::history::literals::{dimension_display, parse_parameter_literal};

/// `ir`, read from CADIR JSON, with the history arenas of `image`, the part
/// it was saved from, and the edits of its `design` records applied to them.
pub(crate) fn restore(ir: &CadIr, image: &[u8]) -> Result<CadIr, CodecError> {
    let decoded = crate::SldprtCodec
        .decode(&mut Cursor::new(image), &DecodeOptions::default())
        .map_err(|failure| match failure {
            DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format_args!(
                "SLDPRT retained source cannot be decoded: {other}"
            )),
        })?;
    restore_history(ir, decoded.ir(), &SolidWorksText)
}

/// The text of a `Keywords` dimension or equation.
struct SolidWorksText;

impl HistoryText for SolidWorksText {
    fn dialect(&self) -> ExpressionDialect {
        ExpressionDialect::SolidWorks
    }

    fn literal(&self, value: f64, unit: &Unit, stored: &str) -> Option<String> {
        let suffix = match unit.0.as_slice() {
            [] => "",
            [factor] if factor.power.get() == 1 => {
                LITERAL_UNITS
                    .iter()
                    .find(|(_, symbol)| *symbol == factor.symbol)?
                    .0
            }
            _ => return None,
        };
        let literal = format!("{value}{suffix}");
        let stored = stored.trim();
        let display = dimension_display(stored);
        let prefixed = display.and_then(|_| {
            let at = stored.char_indices().map(|(at, _)| at).find(|at| {
                let rest = &stored[*at..];
                dimension_display(rest).is_none() && parse_parameter_literal(rest).is_some()
            })?;
            Some(format!("{}{literal}", &stored[..at]))
        });
        Some(
            prefixed
                .filter(|text| dimension_display(text) == display)
                .unwrap_or(literal),
        )
    }

    fn reference(&self, target: &DesignParameter, model: &Model) -> Option<String> {
        let name = match &target.owner {
            Some(owner) => {
                let feature = model.features.iter().find(|feature| feature.id == *owner)?;
                format!("{}@{}", target.name, feature.name.as_deref()?)
            }
            None => target.name.clone(),
        };
        Some(format!("\"{}\"", name.replace('"', "\"\"")))
    }
}

#[cfg(test)]
mod tests;
