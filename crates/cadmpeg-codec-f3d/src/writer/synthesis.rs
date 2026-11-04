// SPDX-License-Identifier: Apache-2.0
//! Synthesis admission and reported reductions. Retained bytes never enter here.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{ArenaDispositions, NativeDispositions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

mod act;
mod appearance;
mod body_keys;
mod design;
mod geometry;
mod native;
mod prepare;
mod streams;
mod topology;

pub(super) struct Synthesized {
    pub(super) bytes: Vec<u8>,
    pub(super) coverage: ArenaDispositions,
    pub(super) losses: Vec<LossNote>,
    pub(super) native_dispositions: NativeDispositions,
}

/// Author an archive from the document, reducing unsupported records locally.
pub(super) fn write(target: &CadIr) -> Result<Synthesized, CodecError> {
    let mut prepared = target.clone();
    let mut losses = Vec::new();
    prepare::auxiliary(&mut prepared, &mut losses);
    appearance::prepare(&mut prepared, &mut losses);
    native::reconcile(&mut prepared, &mut losses)?;
    let mut bytes = Vec::new();
    let coverage = match super::generate::write_new(&prepared, &mut bytes) {
        Ok(coverage) => coverage,
        Err(error) if declines(&error) => {
            prepare::solved_carriers(&mut prepared, &mut losses)?;
            prepare::pcurves(&mut prepared, &mut losses)?;
            bytes.clear();
            match super::generate::write_new(&prepared, &mut bytes) {
                Ok(coverage) => coverage,
                Err(error) if declines(&error) => {
                    prepared = topology::admit_bodies(&prepared, &mut losses)?;
                    native::repair_keys(&mut prepared)?;
                    bytes.clear();
                    match super::generate::write_new(&prepared, &mut bytes) {
                        Ok(coverage) => coverage,
                        Err(error) if declines(&error) => {
                            // §5.0: a limitation the reductions cannot
                            // localize is still not a refusal. Shed the
                            // native design the generator cannot express
                            // and retry; every omission is reported.
                            native::shed_design(&mut prepared)?;
                            bytes.clear();
                            match super::generate::write_new(&prepared, &mut bytes) {
                                Ok(coverage) => coverage,
                                Err(error) if declines(&error) => {
                                    losses.push(F3dLossCode::WriterNoExportableSolids.note(format!(
                                        "unsupported writer content remains after shedding design: {error}; wrote an empty geometry carrier"
                                    )));
                                    prepared = CadIr::empty();
                                    bytes.clear();
                                    match super::generate::write_new(&prepared, &mut bytes) {
                                        Ok(coverage) => coverage,
                                        // Terminal §5.0 reduction: even the empty
                                        // carrier is unwritable. Report it and
                                        // return the empty dispositions; only a
                                        // §5.0 precondition still refuses.
                                        Err(error) if declines(&error) => {
                                            bytes.clear();
                                            losses.push(
                                                F3dLossCode::WriterNoExportableSolids.note(format!(
                                                    "unsupported writer content remains after shedding design: {error}; wrote no exportable geometry"
                                                )),
                                            );
                                            super::generate::coverage(
                                                false,
                                                &prepared,
                                                &crate::native::F3dNative::default(),
                                            )
                                        }
                                        Err(error) => return Err(error),
                                    }
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let native_dispositions = native::coverage(target, &prepared);
    let coverage = prepare::dispositions(target, &prepared, coverage, &mut losses);
    if prepared.model.bodies.is_empty() {
        losses.push(F3dLossCode::WriterNoExportableSolids.note(
            "the document has no body representable by the F3D writer; wrote an empty geometry carrier",
        ));
    }
    Ok(Synthesized {
        bytes,
        coverage,
        losses,
        native_dispositions,
    })
}

/// Internal encoder limitations are reductions. Invalid records and capacity
/// limits remain preconditions. Native/neutral disagreement is an edit, so it
/// selects neutral regeneration rather than an invalid-document refusal.
pub(super) fn declines(error: &CodecError) -> bool {
    matches!(error, CodecError::UnsupportedContent(_))
}
