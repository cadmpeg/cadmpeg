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
mod materials;
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
pub(super) fn write(
    target: &CadIr,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<Synthesized, CodecError> {
    let mut prepared = target.clone();
    let mut losses = Vec::new();
    prepare::auxiliary(&mut prepared, &mut losses);
    let original = super::primitives::f3d_native(&prepared)?.unwrap_or_default();
    appearance::prepare(&mut prepared, &original, &mut losses);
    native::reconcile(&mut prepared, original, &mut losses)?;
    let mut bytes = Vec::new();
    let mut regenerated_parameters = std::collections::BTreeSet::new();
    let coverage = match super::generate::write_with_limits(&prepared, &mut bytes, limits) {
        Ok(generated) => {
            losses.extend(generated.losses);
            regenerated_parameters = generated.regenerated_parameters;
            generated.coverage
        }
        Err(error) if declines(&error) => {
            prepare::solved_carriers(&mut prepared, &mut losses)?;
            prepare::unreferenced_composite_curves(&mut prepared, &mut losses)?;
            prepare::pcurves(&mut prepared, &mut losses)?;
            bytes.clear();
            match super::generate::write_with_limits(&prepared, &mut bytes, limits) {
                Ok(generated) => {
                    losses.extend(generated.losses);
                    regenerated_parameters = generated.regenerated_parameters;
                    generated.coverage
                }
                Err(error) if declines(&error) => {
                    prepared = topology::admit_bodies(&prepared, &mut losses, limits)?;
                    native::repair_keys(&mut prepared)?;
                    bytes.clear();
                    match super::generate::write_with_limits(&prepared, &mut bytes, limits) {
                        Ok(generated) => {
                            losses.extend(generated.losses);
                            regenerated_parameters = generated.regenerated_parameters;
                            generated.coverage
                        }
                        Err(error) if declines(&error) => {
                            // §5.0: a limitation the reductions cannot
                            // localize is still not a refusal. Shed the
                            // native design the generator cannot express
                            // and retry; every omission is reported.
                            native::shed_design(&mut prepared)?;
                            bytes.clear();
                            match super::generate::write_with_limits(&prepared, &mut bytes, limits)
                            {
                                Ok(generated) => {
                                    losses.extend(generated.losses);
                                    regenerated_parameters = generated.regenerated_parameters;
                                    generated.coverage
                                }
                                Err(error) if declines(&error) => {
                                    losses.push(F3dLossCode::WriterNoExportableSolids.note(format!(
                                        "unsupported writer content remains after shedding design: {error}; wrote an empty geometry carrier"
                                    )));
                                    prepared = CadIr::empty();
                                    bytes.clear();
                                    match super::generate::write_with_limits(
                                        &prepared, &mut bytes, limits,
                                    ) {
                                        Ok(generated) => {
                                            losses.extend(generated.losses);
                                            regenerated_parameters =
                                                generated.regenerated_parameters;
                                            generated.coverage
                                        }
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
    let native = super::primitives::f3d_native(&prepared)?.unwrap_or_default();
    let attributes = super::generate::attributes::represented_attributes(&prepared, &native);
    let invalidated_timestamps = native
        .creation_timestamps
        .iter()
        .filter(|timestamp| {
            prepared.model.attributes.iter().any(|attribute| {
                attributes.contains(attribute.id.as_str())
                    && cadmpeg_asm::brep::attributes::attribute_key(attribute)
                        == timestamp.record_index.to_string()
                    && crate::brep::link_target_for(
                        &prepared.model.bodies,
                        &prepared.model.faces,
                        &prepared.model.edges,
                        &prepared.model.vertices,
                        &prepared.model.coedges,
                        &attribute.target,
                    )
                    .is_some_and(|(row, coedge)| {
                        row == timestamp.target.as_str() && coedge == timestamp.target_coedge
                    })
                    && crate::brep::creation_timestamp_value(attribute).is_none()
            })
        })
        .map(|timestamp| timestamp.id.clone())
        .collect();
    let native_dispositions = native::coverage(
        target,
        &prepared,
        &invalidated_timestamps,
        &regenerated_parameters,
    );
    let coverage = prepare::dispositions(target, &prepared, coverage, &attributes, &mut losses);
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
