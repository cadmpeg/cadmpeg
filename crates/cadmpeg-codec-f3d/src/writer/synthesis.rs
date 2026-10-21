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
    pub(super) preservation_compatible: bool,
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
                    super::generate::write_new(&prepared, &mut bytes)?
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    };
    let native_dispositions = native::coverage(target, &prepared, &mut losses)?;
    let coverage = prepare::dispositions(target, &prepared, coverage, &mut losses);
    if prepared.model.bodies.is_empty() {
        losses.push(F3dLossCode::WriterNoExportableSolids.note(
            "the document has no body representable by the F3D writer; wrote an empty geometry carrier",
        ));
    }
    let typed_native = |ir: &CadIr| {
        ir.native
            .namespace("f3d")
            .map(|namespace| {
                namespace
                    .arenas()
                    .iter()
                    .filter(|(name, records)| name.as_str() != "unknowns" && !records.is_empty())
                    .map(|(name, records)| (name.clone(), records.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    };
    let preservation_compatible =
        target.model == prepared.model && typed_native(target) == typed_native(&prepared);
    Ok(Synthesized {
        bytes,
        coverage,
        losses,
        native_dispositions,
        preservation_compatible,
    })
}

/// Internal encoder limitations are reductions. Invalid records and capacity
/// limits remain preconditions. Native/neutral disagreement is an edit, so it
/// selects neutral regeneration rather than an invalid-document refusal.
pub(super) fn declines(error: &CodecError) -> bool {
    matches!(error, CodecError::UnsupportedContent(_))
}
