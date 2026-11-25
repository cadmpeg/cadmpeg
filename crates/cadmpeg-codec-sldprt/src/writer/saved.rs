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
use cadmpeg_ir::report::loss::LossNote;

use crate::design_v11::parameters::LITERAL_UNITS;
use crate::history::literals::{dimension_display, parse_parameter_literal};
use crate::loss::SldprtLossCode;

/// `input`'s part with the history arenas the CADIR wire omits restored
/// from its retained source, where it was read from CADIR JSON and `target`
/// does not replay it unedited.
///
/// The restored part holds every design edit those arenas state, and
/// `losses` names each other edit by its path. Where the retained source
/// cannot be restored from, the part is left as read and `notes` says why.
pub(crate) fn restore(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
    notes: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<CadIr>, CodecError> {
    let Some(image) = saved_source(input, crate::SOURCE_IMAGE_ID, target.preserves_source())?
    else {
        return Ok(None);
    };
    let restored = crate::SldprtCodec
        .decode(
            &mut Cursor::new(image),
            &DecodeOptions {
                policy: cadmpeg_core::decode::DecodePolicy {
                    limits: input.limits,
                },
                ..DecodeOptions::default()
            },
        )
        .map_err(|failure| match failure {
            DecodeFailure::Codec(error) => error,
            other => CodecError::malformed(format_args!(
                "SLDPRT retained source cannot be decoded: {other}"
            )),
        })
        .and_then(|decoded| restore_history(input.ir, decoded.ir(), &SolidWorksText));
    match restored {
        Ok(restored) => {
            losses.extend(unwritten_losses(&restored.unwritten));
            Ok(Some(restored.ir))
        }
        Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => Err(error),
        Err(error) => {
            notes.push(format!(
                "the history arenas of the document read from CADIR are not restored from its \
                 retained source part: {error}"
            ));
            Ok(None)
        }
    }
}

/// One loss per kind of design record that holds an edit the restored
/// history arenas cannot state, naming each such edit by its path.
fn unwritten_losses(unwritten: &[String]) -> Vec<LossNote> {
    let mut parameters = Vec::new();
    let mut features = Vec::new();
    let mut others = Vec::new();
    for path in unwritten {
        if path.starts_with("design.parameters[") {
            parameters.push(path.as_str());
        } else if path.starts_with("design.features[") {
            features.push(path.as_str());
        } else {
            others.push(path.as_str());
        }
    }
    [
        (
            SldprtLossCode::WriterParameterEditUnwritten,
            "parameters",
            parameters,
        ),
        (
            SldprtLossCode::WriterFeatureEditUnwritten,
            "features",
            features,
        ),
        (SldprtLossCode::WriterDesignEditUnwritten, "records", others),
    ]
    .into_iter()
    .filter(|(_, _, paths)| !paths.is_empty())
    .map(|(code, records, paths)| {
        code.note(format!(
            "the document was read from CADIR, and the feature history of its retained source \
             part cannot state these edits of its design {records}, so the output keeps the \
             stored state: {}",
            paths.join("; ")
        ))
    })
    .collect()
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
