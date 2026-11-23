// SPDX-License-Identifier: Apache-2.0
//! A decoder's projection of its records into the v11 design layer.
//!
//! The projection reads no source byte: it restates records the decoder
//! already read. A projection that fails is given up as one part (§3.3 of
//! the CADIR v11 specification): the design layer stays as it was before the
//! projection, and one `record.unreadable` loss names the layer.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::unknown::part::record_unreadable_kind;

/// Runs `project`, the `codec` decoder's projection of the v11 design layer
/// of `ir`, as one part.
///
/// Where `project` fails with an error that does not end the session, `ir`
/// holds the design records and product definitions it held before the
/// projection, `losses` holds the losses it held before the projection and
/// one `record.unreadable` that names the layer. The geometry and the model
/// records stand.
///
/// # Errors
///
/// An error that ends the session: a refusal in force, a Session-scoped
/// resource refusal, or an I/O error.
pub fn project_layer(
    ctx: &DecodeContext<'_>,
    codec: &str,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    project: impl FnOnce(&mut CadIr, &mut Vec<LossNote>) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let design = ir.design.clone();
    let definitions = ir.model.product_definitions.clone();
    let reported = losses.len();
    if let Err(error) = ctx.run_part(|| project(ir, losses))? {
        ir.design = design;
        ir.model.product_definitions = definitions;
        losses.truncate(reported);
        losses.push(
            LossNote::new(
                record_unreadable_kind(),
                format!("{codec} v11 design layer is not stated: {error}"),
            )
            .with_severity(Severity::Error),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
