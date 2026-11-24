// SPDX-License-Identifier: Apache-2.0
//! The feature history of a part read from CADIR JSON, restored from the
//! decode of its retained source and the edits its `design` records state.

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

use crate::design_v11::parameters::LITERAL_UNITS;
use crate::history::literals::{dimension_display, parse_parameter_literal};

/// `input`'s part with the history arenas the CADIR wire omits restored
/// from its retained source, where it was read from CADIR JSON and `target`
/// does not replay it unedited.
///
/// A restore that the design edits refuse refuses the write, except for a
/// compound-file part that `target` preserves: that part is left as read and
/// the refusal goes to `notes`, so the write is the stored part with the loss
/// that states no edit is written.
pub(crate) fn restore(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
    notes: &mut Vec<String>,
) -> Result<Option<CadIr>, CodecError> {
    let Some(image) = saved_source(input, crate::SOURCE_IMAGE_ID, target.preserves_source())?
    else {
        return Ok(None);
    };
    let restored = crate::SldprtCodec
        .decode(&mut Cursor::new(image), &DecodeOptions::default())
        .map_err(|failure| match failure {
            DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format_args!(
                "SLDPRT retained source cannot be decoded: {other}"
            )),
        })
        .and_then(|decoded| restore_history(input.ir, decoded.ir(), &SolidWorksText));
    match restored {
        Ok(restored) => Ok(Some(restored)),
        Err(error)
            if !matches!(error, CodecError::ResourceLimit(_) | CodecError::Io(_))
                && target.preserves_source()
                && crate::container::is_compound_file(image) =>
        {
            notes.push(error.to_string());
            Ok(None)
        }
        Err(error) => Err(error),
    }
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
